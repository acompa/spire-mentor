"""One referee worker: a headless host process in referee mode.

One process plays one episode (the game keeps run state in singletons). The
host speaks newline-delimited JSON on stdin and stdout
(``spire-bench/PROTOCOL.md``, "Worker lifecycle").
"""

from __future__ import annotations

import asyncio
import hashlib
import json
import os
from typing import Any

import sts2_client

# The host refuses every other command in referee mode; the gateway only ever
# sends these.
_ALLOWED = {"start_run", "action", "quit"}


def engine_hash(version: str | None = None) -> str:
    """sha256 over the game DLL and the host assembly that run the episodes."""
    ver = version or sts2_client.GAME_VERSION
    h = hashlib.sha256()
    host_dir = os.path.dirname(sts2_client.headless_bin(ver))
    for path in (
        os.path.join(sts2_client.lib_dir(ver), "sts2.dll"),
        os.path.join(host_dir, "Sts2Headless.dll"),
    ):
        with open(path, "rb") as fh:
            h.update(hashlib.sha256(fh.read()).digest())
    return h.hexdigest()


class WorkerError(RuntimeError):
    """The host died, stopped answering, or did not start.

    ``kind`` is ``"exited"`` (the process ended or its pipe closed),
    ``"timeout"`` (no reply in time) or ``"start"`` (it did not come up in
    referee mode). ``spire_bench.verify`` replays a failure only of the same
    kind.
    """

    def __init__(self, message: str, kind: str) -> None:
        super().__init__(message)
        self.kind = kind


class RefereeWorker:
    """An async handle on one referee host process."""

    def __init__(self, proc: asyncio.subprocess.Process, reply_timeout: float) -> None:
        self._proc = proc
        self._timeout = reply_timeout
        self.world_errors = 0
        self._stderr_task = asyncio.create_task(self._drain_stderr())

    @classmethod
    async def spawn(cls, reply_timeout: float | None = None) -> RefereeWorker:
        env = os.environ.copy()
        env["STS2_REFEREE"] = "1"
        # Operator diagnostics stay off on a scoring worker.
        env.pop("STS2_REFEREE_DIAG", None)
        proc = await asyncio.create_subprocess_exec(
            *sts2_client.headless_cmd(),
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            env=env,
            cwd=sts2_client.REPO_ROOT,
            limit=64 * 1024 * 1024,
        )
        worker = cls(proc, reply_timeout or sts2_client.reply_timeout())
        ready = await worker._read()
        if ready.get("type") != "ready" or ready.get("referee") is not True:
            await worker.close()
            raise WorkerError(f"host did not start in referee mode: {ready}", "start")
        return worker

    async def _drain_stderr(self) -> None:
        stream = self._proc.stderr
        assert stream is not None
        while line := await stream.readline():
            if sts2_client.is_world_error_line(line.decode("utf-8", "replace")):
                self.world_errors += 1

    async def _read(self) -> dict[str, Any]:
        stream = self._proc.stdout
        assert stream is not None
        while True:
            try:
                line = await asyncio.wait_for(stream.readline(), self._timeout)
            except TimeoutError as exc:
                raise WorkerError("host did not answer in time", "timeout") from exc
            if not line:
                raise WorkerError("host exited", "exited")
            text = line.decode("utf-8", "replace").strip()
            if text.startswith("{"):
                return json.loads(text)

    async def send(self, cmd: dict[str, Any]) -> dict[str, Any]:
        if cmd.get("cmd") not in _ALLOWED:
            raise ValueError(f"command not allowed on a referee worker: {cmd.get('cmd')}")
        stdin = self._proc.stdin
        assert stdin is not None
        try:
            stdin.write((json.dumps(cmd) + "\n").encode())
            await stdin.drain()
        except (BrokenPipeError, ConnectionResetError) as exc:
            raise WorkerError("host pipe closed", "exited") from exc
        return await self._read()

    async def close(self) -> None:
        if self._proc.returncode is None:
            try:
                assert self._proc.stdin is not None
                self._proc.stdin.write(b'{"cmd":"quit"}\n')
                await self._proc.stdin.drain()
                await asyncio.wait_for(self._proc.wait(), 10)
            except (OSError, TimeoutError, AssertionError):
                self._proc.kill()
                await self._proc.wait()
        try:
            await asyncio.wait_for(self._stderr_task, 5)
        except TimeoutError:
            self._stderr_task.cancel()
