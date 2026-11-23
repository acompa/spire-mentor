#!/usr/bin/env bash
# gate.sh — the per-PR gate (`just gate`): the Rust and Python lanes side by
# side, then check-public.
#
# Run one after the other the lanes took ~8 min (2026-11-16, 48 busy cores):
# Rust ~4.5 (fmt, clippy, nextest, doctests, fuzz and wasm checks), Python
# ~3.5 (static checks, which build the `sts2_rs` wheel, then pytest). They
# share no build lock: the wheel builds in uv's cache, not in sts2-rs/target.
#
# A third lane, scripts/perf-gate.sh (~1 min warm, own target dir), checks
# W1's instructions per action against sts2-rs/perf/w1-instructions.json.
#
# Each lane writes its own log; a lane that fails prints its log's tail. The
# logs stay in $GATE_LOG_DIR (default: a fresh temp dir, printed at the start).
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
logs="${GATE_LOG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/gate.XXXXXX")}"
mkdir -p "$logs"
echo "gate: logs in $logs"

rust_lane() {
  set -e
  scripts/cargo-fmt-check.sh
  cd sts2-rs
  cargo clippy --workspace --all-targets -- -D warnings
  cargo nextest run --workspace
  cargo test --workspace --doc
  cargo check --manifest-path fuzz/Cargo.toml
  cd ../sts2-pycli
  just wasm-check
}

python_lane() {
  set -e
  cd sts2-pycli
  just gate
}

start=$SECONDS
(rust_lane) >"$logs/rust.log" 2>&1 &
rust_pid=$!
(python_lane) >"$logs/python.log" 2>&1 &
py_pid=$!
scripts/perf-gate.sh >"$logs/perf.log" 2>&1 &
perf_pid=$!

fail=0
report() {
  local name=$1 status=$2
  if [ "$status" -eq 0 ]; then
    echo "gate: $name lane passed ($((SECONDS - start)) s)"
  else
    fail=1
    echo "gate: $name lane FAILED ($((SECONDS - start)) s); last 60 lines of $logs/$name.log:"
    tail -n 60 "$logs/$name.log"
  fi
}
wait "$rust_pid"; report rust $?
wait "$py_pid"; report python $?
wait "$perf_pid"; report perf $?
tail -n 1 "$logs/perf.log"

just check-public >"$logs/check-public.log" 2>&1
report check-public $?

[ "$fail" -eq 0 ] || exit 1
echo "gate: passed in $((SECONDS - start)) s"
