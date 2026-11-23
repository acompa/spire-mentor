#!/usr/bin/env bash
# gate-full.sh — `just gate-full`: the per-PR gate plus everything it leaves
# out (#1416). Every stage runs even after one fails, so a red run lists all
# its failures at once. Before, the first failure ended the run: the idle
# job's first run (#1471) stopped at a Rust `slow_` test and never reached
# the training tests, the other builds, the regressions or the slow suite.
#
# Each stage writes $GATE_LOG_DIR/full-<stage>.log (default: a fresh temp
# dir); the summary at the end names the failed stages and prints the tail of
# each one's log. Exits non-zero if any stage failed.
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
logs="${GATE_LOG_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/gate-full.XXXXXX")}"
mkdir -p "$logs"
export GATE_LOG_DIR="$logs"
echo "gate-full: logs in $logs"

stages=(
  "gate|scripts/gate.sh"
  "rust-slow|cd sts2-rs && cargo nextest run --workspace --profile slow"
  "other-builds|cd sts2-pycli && just test-other-builds"
  "training|cd sts2-pycli && just test-training"
  "regression|cd sts2-pycli && just regression-builds"
  "event-parity|cd sts2-pycli && just event-parity"
  # Agent-mode lane (GH #1610): 20 Ironclad runs, ~70 s; fails on a NEW
  # offered-action difference or a broken lockstep.
  "agent-parity|cd sts2-pycli && just parity-agent 20 Ironclad v0.111.0 \"$GATE_LOG_DIR/agent-parity\""
  "slow|cd sts2-pycli && just test-slow"
  "public-shape|just test-public-shape"
  "e2e-phone|cd sts2-pycli && just e2e-phone"
)

start=$SECONDS
failed=()
for s in "${stages[@]}"; do
  name=${s%%|*}
  cmd=${s#*|}
  t=$SECONDS
  if (bash -c "$cmd") >"$logs/full-$name.log" 2>&1; then
    echo "gate-full: $name passed ($((SECONDS - t)) s)"
  else
    echo "gate-full: $name FAILED ($((SECONDS - t)) s)"
    failed+=("$name")
  fi
done

if [ "${#failed[@]}" -eq 0 ]; then
  echo "gate-full: passed in $(((SECONDS - start) / 60)) min"
  exit 0
fi
echo
echo "gate-full: FAILED stages: ${failed[*]} ($(((SECONDS - start) / 60)) min)"
for name in "${failed[@]}"; do
  echo
  echo "── $name: last 40 lines of $logs/full-$name.log ──"
  tail -n 40 "$logs/full-$name.log"
done
exit 1
