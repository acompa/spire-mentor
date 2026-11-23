#!/usr/bin/env bash
# heavy-suite.sh — run a memory-heavy test suite under a machine-wide lock.
#
# Two `pytest -n auto` suites at once exhaust memory on the dev box: on
# 2026-11-14 a gate, a second suite and another session's test-public-shape
# overlapped, swap filled, and the OOM killer SIGTERMed ~30 Python workers.
# That reads as scattered test failures, not as an OOM (GH #1362). The gates
# wrap every pytest stage in this script, so suites from different sessions
# and worktrees queue instead of overlapping.
#
# Usage:
#   scripts/heavy-suite.sh uv run --frozen pytest tests/ -q
#
# Env:
#   STS2_HEAVY_SUITE_LOCK=0        run without the lock
#   STS2_HEAVY_SUITE_LOCK_DIR      lock dir (default: /tmp/sts2-heavy-suite.lock;
#                                  fixed, not $TMPDIR, so every session shares it)
#   STS2_HEAVY_SUITE_TIMEOUT       max seconds to wait (default: 14400; a full
#                                  slow suite holds the lock for a long time)
#
# The lock is build-mutex.sh's (atomic mkdir, stale holder reaped by PID), on
# its own dir: a suite and a cargo build may run side by side.
set -euo pipefail

if [ "$#" -eq 0 ]; then
  echo "heavy-suite: no command given" >&2
  exit 2
fi

if [ "${STS2_HEAVY_SUITE_LOCK:-1}" = 0 ]; then
  exec "$@"
fi

lock="${STS2_HEAVY_SUITE_LOCK_DIR:-/tmp/sts2-heavy-suite.lock}"
if [ -d "$lock" ]; then
  holder="$(cat "$lock/pid" 2>/dev/null || echo '?')"
  echo "heavy-suite: waiting for $lock (held by PID $holder: $(ps -o args= -p "$holder" 2>/dev/null | sed 's/.*-u BUILD_MUTEX_WARN //' | cut -c1-160 || true))" >&2
fi

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export BUILD_MUTEX_DIR="$lock"
export BUILD_MUTEX_TIMEOUT="${STS2_HEAVY_SUITE_TIMEOUT:-14400}"
export BUILD_MUTEX_WARN=600
# The suite itself must not inherit the lock's settings: a cargo build it
# starts through build-mutex.sh would wait on this very lock (deadlock).
exec "$here/../sts2-rs/scripts/build-mutex.sh" \
  env -u BUILD_MUTEX_DIR -u BUILD_MUTEX_TIMEOUT -u BUILD_MUTEX_WARN "$@"
