#!/usr/bin/env bash
# perf-gate.sh — the engine-speed lane of `just gate`: user-space instructions
# per W1 action (site_random_bench, the workload behind the published "fights
# per second" figure) against the committed baseline.
#
#   scripts/perf-gate.sh            check (fails on > TOLERANCE_PCT more)
#   scripts/perf-gate.sh --update   measure and rewrite the baseline
#
# Why instructions, not time: the count is deterministic (two runs of one
# binary differ by < 0.01%), so the check needs no quiet host and cannot
# flake, while a timing gate on a shared 48-core box would. Between
# 2026-10-14 and 2026-11-17 W1 went from ~6,540 to ~9,465 instructions per
# action (1.30 M -> 0.84 M actions/s) with an identical digest, in ~300 merges
# of +20..+580 each; nothing caught it because no gate measured it.
#
# The baseline pins the W1 digest and the rustc version too:
# - another digest means the engine now plays W1 differently (a fidelity fix
#   that changes Act-1 behaviour); the count is then not comparable, so the
#   gate fails until the PR rebaselines with --update and says why;
# - another rustc moves the count by a few percent of codegen; a toolchain
#   bump rebaselines in the same PR.
# A drop of more than TOLERANCE_PCT only prints a note: rebaseline to lock the
# win in.
#
# Builds in its own target dir (sts2-rs/target/perf-gate) so it never waits on
# the Rust lane's cargo lock. Skips (exit 0, with a warning) where `perf` or
# the hardware counter is unavailable (cloud sessions, macOS, Windows).
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
baseline="$root/sts2-rs/perf/w1-instructions.json"
target_dir="${PERF_GATE_TARGET_DIR:-$root/sts2-rs/target/perf-gate}"
tolerance="${PERF_GATE_TOLERANCE_PCT:-1.0}"
update=0
[ "${1:-}" = "--update" ] && update=1

skip() {
  echo "perf-gate: SKIPPED: $1"
  exit 0
}
command -v perf >/dev/null 2>&1 || skip "perf not installed"
perf stat -x, -e instructions:u -- true >/dev/null 2>&1 || skip "no user-space instruction counter (perf_event_paranoid / VM)"

cd "$root/sts2-rs"
CARGO_TARGET_DIR="$target_dir" RUSTFLAGS= cargo build --release --quiet \
  -p sts2-bench --example site_random_bench || { echo "perf-gate: build FAILED"; exit 1; }
bin="$target_dir/release/examples/site_random_bench"

out="$(perf stat -x, -e instructions:u -- "$bin" --seeds 2200 --reps 50 2>&1)" \
  || { echo "perf-gate: bench FAILED"; echo "$out" | tail -20; exit 1; }
instructions="$(echo "$out" | awk -F, '/instructions/ {print $1}')"
json="$(echo "$out" | grep '^{')"
rustc="$(rustc --version | awk '{print $2}')"

exec python3 -I - "$baseline" "$update" "$tolerance" "$instructions" "$rustc" "$json" <<'PY'
import json, sys
path, update, tol, instructions, rustc, line = sys.argv[1:]
run = json.loads(line)
per_action = int(instructions) / run["decisions"]
now = {
    "workload": "site_random_bench --seeds 2200 --reps 50 (release, RUSTFLAGS empty)",
    "instructions_per_action": round(per_action, 1),
    "digest": run["digest"],
    "fights": run["fights"],
    "wins": run["wins"],
    "decisions": run["decisions"],
    "rustc": rustc,
}
if update == "1":
    with open(path, "w", encoding="utf-8") as fh:
        json.dump(now, fh, indent=2)
        fh.write("\n")
    print(f"perf-gate: baseline written: {per_action:,.1f} instructions/action, digest {run['digest']}")
    sys.exit(0)
try:
    with open(path, encoding="utf-8") as fh:
        base = json.load(fh)
except FileNotFoundError:
    print(f"perf-gate: FAILED: no baseline at {path}; run scripts/perf-gate.sh --update")
    sys.exit(1)
fix = "if this PR means it, run `just perf-gate --update`, commit the baseline and say why in the PR"
if run["digest"] != base["digest"]:
    print(f"perf-gate: FAILED: W1 digest {run['digest']} != baseline {base['digest']}: "
          f"the engine now plays W1 differently, so the count is not comparable; {fix}")
    sys.exit(1)
if rustc != base["rustc"]:
    print(f"perf-gate: FAILED: rustc {rustc} != baseline {base['rustc']}; {fix}")
    sys.exit(1)
ref = base["instructions_per_action"]
pct = 100.0 * (per_action - ref) / ref
msg = f"{per_action:,.1f} instructions/action vs baseline {ref:,.1f} ({pct:+.2f}%, tolerance +{float(tol):.1f}%)"
if pct > float(tol):
    print(f"perf-gate: FAILED: W1 got slower: {msg}; find the cost (perf record -e instructions:u) "
          f"or, if it is the price of a fix: {fix}")
    sys.exit(1)
note = "; faster: rebaseline (--update) to lock it in" if pct < -float(tol) else ""
print(f"perf-gate: passed: {msg}{note}")
PY
