#!/usr/bin/env bash
# A per-checkout venv for Pascal GPUs (sm_6x: TITAN Xp, GTX 10xx).
#
# The locked torch is a CUDA 13 build, compiled for sm_75 and newer: on a
# Pascal card every CUDA kernel fails with "no kernel image is available for
# execution on the device". PyTorch's CUDA 12.6 build of the same version
# still ships sm_50/sm_60 kernels. This script keeps such a torch in
# `.venv-pascal` (next to the normal `.venv`, which it never touches):
#
#   1. `uv sync --frozen --inexact` into `.venv-pascal` with torch and its CUDA
#      runtime wheels left out, so the rest matches the lock (sts2_rs is
#      rebuilt after a Rust change, as on every sync);
#   2. if the venv's torch is not the locked version from the CUDA 12.6 index,
#      install it (`uv pip install`, which brings the matching cu12 wheels).
#
# Re-running is cheap and idempotent, so each command goes through it. A
# one-off `uv pip install` into `.venv` does not work: the next `uv run`
# re-syncs the CUDA wheels to the lock and torch stops importing. Commands then
# run with UV_NO_SYNC=1 and UV_PROJECT_ENVIRONMENT=.venv-pascal, so nested
# `uv run` calls use this venv as it is. Every worktree builds its own (it is
# gitignored, like `.venv`).
#
# Usage (from sts2-pycli/, or through `just pascal`):
#   scripts/pascal-venv.sh                      # build or refresh the venv
#   scripts/pascal-venv.sh uv run pytest ...    # run a command in it
#   scripts/pascal-venv.sh --auto CMD ...       # in it on a Pascal machine,
#                                               # else CMD as is
#
# `--auto` is what the justfile's pytest recipes use, so `just test-gate` & co.
# pick the venv by themselves. It takes .venv-pascal when some GPU is older
# than sm_75 (what the locked CUDA 13 torch needs) and none is sm_100 or newer
# (the cu126 build has no Blackwell kernels); with no `nvidia-smi` (macOS, CI)
# it just runs CMD. STS2_PASCAL_VENV=1 / =0 forces the choice.
#
# PASCAL_TORCH_INDEX overrides the index (default: PyTorch's cu126 wheels).
# When a torch upgrade drops the cu126 build, the install step fails here.
set -euo pipefail
cd "$(dirname "$0")/.."

# 1 when this machine needs .venv-pascal, else 0.
needs_pascal_venv() {
  case "${STS2_PASCAL_VENV:-}" in
    1) echo 1; return ;;
    0) echo 0; return ;;
  esac
  local caps
  caps=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null) || caps=""
  awk -F. 'NF { c = $1 * 10 + $2; if (c < 75) old = 1; if (c >= 100) new = 1 }
           END { print (old && !new) ? 1 : 0 }' <<<"$caps"
}

if [[ "${1:-}" == "--auto" ]]; then
  shift
  if [[ "$(needs_pascal_venv)" != 1 ]]; then
    exec "$@"
  fi
  echo "pascal-venv: pre-sm_75 GPU, running in .venv-pascal (STS2_PASCAL_VENV=0 to skip)" >&2
fi

venv=.venv-pascal
index=${PASCAL_TORCH_INDEX:-https://download.pytorch.org/whl/cu126}

# torch, its locked version and the CUDA runtime packages it pulls in
# (transitively: nvidia-*, cuda-*, triton), all read from uv.lock with uv's
# own interpreter (no system python3 / tomllib needed).
mapfile -t skip < <(
  uv run --no-project --quiet python - <<'PY'
import re
import tomllib

with open("uv.lock", "rb") as f:
    pkgs = {p["name"]: p for p in tomllib.load(f)["package"]}
gpu = re.compile(r"^(nvidia-|cuda-|triton$)")
seen, todo = set(), ["torch"]
while todo:
    name = todo.pop()
    for d in pkgs.get(name, {}).get("dependencies", []):
        if gpu.match(d["name"]) and d["name"] not in seen:
            seen.add(d["name"])
            todo.append(d["name"])
print(pkgs["torch"]["version"])
print("torch")
print("\n".join(sorted(seen)))
PY
)
torch_version=${skip[0]}
skip=("${skip[@]:1}")
export UV_PROJECT_ENVIRONMENT=$venv

no_install=()
for p in "${skip[@]}"; do no_install+=(--no-install-package "$p"); done
uv sync --frozen --inexact --quiet "${no_install[@]}"

# Healthy: the wanted torch imports and, with a GPU, runs a kernel. The version
# alone is not enough: the cu12 and cu13 nvidia-* wheels have different names
# but write the same files (nvidia/nccl/lib/libnccl.so.2, …), so a locked cu13
# wheel landing in this venv (a plain `uv sync` into it) breaks a cu126 torch
# that still reports the right version ("undefined symbol: ncclCommResume").
want="$torch_version+${index##*/}"
healthy() {
  "$venv/bin/python" - "$want" >/dev/null 2>&1 <<'PY'
import sys

import torch

assert torch.__version__ == sys.argv[1], torch.__version__
if torch.cuda.is_available():
    torch.ones(1, device="cuda").sum().item()
PY
}
if ! healthy; then
  have=$("$venv/bin/python" -c 'import torch; print(torch.__version__)' 2>/dev/null || true)
  echo "pascal-venv: torch ${have:-none} (or its CUDA wheels) -> $want ($index)" >&2
  # Drop every torch / CUDA runtime wheel there is, cu12 and cu13 alike, so
  # the install below lays down exactly the cu126 build's own set.
  mapfile -t stale < <(uv pip list -p "$venv/bin/python" --format freeze 2>/dev/null |
    cut -d= -f1 | grep -E '^(torch|triton|nvidia-.*|cuda-.*)$' || true)
  if ((${#stale[@]})); then
    uv pip uninstall --quiet -p "$venv/bin/python" "${stale[@]}"
  fi
  uv pip install --quiet -p "$venv/bin/python" --index-url "$index" "torch==$torch_version"
  healthy || { echo "pascal-venv: torch from $index still fails to run" >&2; exit 1; }
fi

if (($#)); then
  export UV_NO_SYNC=1
  exec "$@"
fi
"$venv/bin/python" - <<'PY'
import torch

print("torch", torch.__version__, "arches", " ".join(torch.cuda.get_arch_list()))
for i in range(torch.cuda.device_count()):
    print(f"  cuda:{i}", torch.cuda.get_device_name(i), torch.cuda.get_device_capability(i))
PY
