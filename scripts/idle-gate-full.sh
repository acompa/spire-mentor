#!/usr/bin/env bash
# idle-gate-full.sh — run `just gate-full` on origin/main when the box is idle
# (GH #1465). The per-PR gate leaves out the training tests, the slow tiers,
# the other game builds and the regressions (#1416); this is what still runs
# them. scripts/systemd/idle-gate-full.timer starts it every 30 min, and the
# service runs it at idle CPU and IO priority under a memory cap.
#
# One pass:
#   1. fetch; stop if origin/main is the last green or last red sha, or if a
#      gate-full already passed on its tree on this machine (land.gated-trees)
#   2. stop if the box is busy: 5-min load above IDLE_GATE_MAX_LOAD, less than
#      IDLE_GATE_MIN_MEM_GB available, less than IDLE_GATE_MIN_DISK_GB free,
#      or another session holds the heavy-suite lock
#   3. move a dedicated checkout to the sha and run `just gate-full` there
#   4. green: record the sha, close the open red issue. Red: open one (or
#      comment on the open one) with the failing stage's log tail and the
#      commits since the last green sha.
#
# A stop in step 1 or 2 is not a failure: the next tick tries again. A busy
# box only delays the run.
#
# Env (all optional):
#   IDLE_GATE_DIR          state, logs, checkout   (default ~/runs/idle-gate)
#   IDLE_GATE_REPO         the repo to fetch from  (default: this script's)
#   IDLE_GATE_TARGET_DIR   CARGO_TARGET_DIR        (default: the repo's
#                          sts2-rs/target; a second one costs 20-30 GB)
#   IDLE_GATE_MAX_LOAD     (default: nproc / 2)
#   IDLE_GATE_MIN_MEM_GB   (default 48)
#   IDLE_GATE_MIN_DISK_GB  (default 40)
#   IDLE_GATE_CMD          what to run in the checkout (default: just gate-full)
#   IDLE_GATE_FORCE=1      skip the idle checks and the already-tested check
#   IDLE_GATE_DRY_RUN=1    say what it would do, run nothing
#   IDLE_GATE_NO_ISSUE=1   print the issue body instead of filing it
set -uo pipefail

repo="${IDLE_GATE_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
dir="${IDLE_GATE_DIR:-$HOME/runs/idle-gate}"
checkout="$dir/checkout"
mkdir -p "$dir/logs"
log() { echo "idle-gate: $*"; }

exec 9>"$dir/lock"
flock -n 9 || { log "another pass is running"; exit 0; }

git -C "$repo" fetch -q origin main || { log "fetch failed"; exit 0; }
sha=$(git -C "$repo" rev-parse origin/main)
tree=$(git -C "$repo" rev-parse "origin/main^{tree}")
last_green=$(cat "$dir/last_green" 2>/dev/null || true)
last_red=$(cat "$dir/last_red" 2>/dev/null || true)
common=$(cd "$repo" && cd "$(git rev-parse --git-common-dir)" && pwd)

if [ "${IDLE_GATE_FORCE:-0}" != 1 ]; then
  if [ "$sha" = "$last_green" ] || [ "$sha" = "$last_red" ]; then
    log "origin/main ${sha:0:9} already tested"; exit 0
  fi
  if grep -qx "gate-full $tree" "$common/land.gated-trees" 2>/dev/null; then
    log "gate-full already passed on ${sha:0:9}'s tree here; recording it green"
    [ "${IDLE_GATE_DRY_RUN:-0}" = 1 ] || echo "$sha" >"$dir/last_green"
    exit 0
  fi
  cores=$(nproc)
  max_load="${IDLE_GATE_MAX_LOAD:-$((cores / 2))}"
  load=$(cut -d' ' -f2 /proc/loadavg)
  if awk -v l="$load" -v m="$max_load" 'BEGIN { exit !(l > m) }'; then
    log "busy: 5-min load $load > $max_load"; exit 0
  fi
  mem_gb=$(awk '/MemAvailable/ { print int($2 / 1048576) }' /proc/meminfo)
  if [ "$mem_gb" -lt "${IDLE_GATE_MIN_MEM_GB:-48}" ]; then
    log "busy: ${mem_gb} GB available < ${IDLE_GATE_MIN_MEM_GB:-48}"; exit 0
  fi
  disk_gb=$(df -BG --output=avail "$dir" | tail -1 | tr -dc 0-9)
  if [ "$disk_gb" -lt "${IDLE_GATE_MIN_DISK_GB:-40}" ]; then
    log "low disk: ${disk_gb} GB free < ${IDLE_GATE_MIN_DISK_GB:-40}"; exit 0
  fi
  if [ -d /tmp/sts2-heavy-suite.lock ]; then
    log "busy: another session holds the heavy-suite lock"; exit 0
  fi
fi

if [ "${IDLE_GATE_DRY_RUN:-0}" = 1 ]; then
  log "would run '${IDLE_GATE_CMD:-just gate-full}' on ${sha:0:9}"; exit 0
fi

# The checkout is this job's alone: nobody edits it, so forcing it onto the
# sha loses nothing.
if [ ! -d "$checkout/.git" ] && [ ! -f "$checkout/.git" ]; then
  git -C "$repo" worktree add -q --detach "$checkout" "$sha" || { log "worktree add failed"; exit 1; }
else
  git -C "$checkout" checkout -q --detach --force "$sha" || { log "checkout failed"; exit 1; }
fi

stamp=$(date +%Y%m%d-%H%M)
out="$dir/logs/$stamp-${sha:0:9}.log"
log "running '${IDLE_GATE_CMD:-just gate-full}' on ${sha:0:9}; log $out"
start=$SECONDS
(
  cd "$checkout" &&
    CARGO_TARGET_DIR="${IDLE_GATE_TARGET_DIR:-$repo/sts2-rs/target}" \
      GATE_LOG_DIR="$dir/logs/$stamp-lanes" \
      bash -c "${IDLE_GATE_CMD:-just gate-full}"
) >"$out" 2>&1
status=$?
mins=$(((SECONDS - start) / 60))
# Keep the 30 newest runs' logs.
ls -1t "$dir/logs"/*.log 2>/dev/null | tail -n +31 | xargs -r rm -f
ls -1dt "$dir/logs"/*-lanes 2>/dev/null | tail -n +31 | xargs -r rm -r

title="gate-full is red on main"
open_issue() {
  gh issue list -R tfriedel/spire-mentor-private --state open --search "in:title \"$title\"" \
    --json number,title --jq ".[] | select(.title == \"$title\") | .number" | head -1
}

if [ "$status" -eq 0 ]; then
  echo "$sha" >"$dir/last_green"
  log "green on ${sha:0:9} (${mins} min)"
  n=$(open_issue)
  if [ -n "$n" ] && [ "${IDLE_GATE_NO_ISSUE:-0}" != 1 ]; then
    gh issue close "$n" -R tfriedel/spire-mentor-private -c "gate-full is green again on ${sha} (${mins} min, idle-gate-full.sh)."
  fi
  exit 0
fi

echo "$sha" >"$dir/last_red"
log "RED on ${sha:0:9} (${mins} min, exit $status)"
body="$dir/issue-body.md"
{
  echo "\`just gate-full\` failed on \`main\` at ${sha} (idle-gate-full.sh, GH #1465; ${mins} min, exit ${status})."
  echo
  echo "Full log on the dev box: \`$out\`"
  echo
  if [ -n "$last_green" ]; then
    echo "Commits since the last green run (${last_green:0:9}); one of them broke it:"
    echo
    echo '```text'
    git -C "$repo" log --oneline --first-parent "$last_green..$sha" | head -40
    echo '```'
  else
    echo "No earlier green run is recorded, so this may predate the job."
  fi
  echo
  # gate-full.sh ends with a summary: the failed stages and each one's log
  # tail. Without one (it died early), the log's last 80 lines.
  if grep -q '^gate-full: FAILED stages' "$out"; then
    echo "Failed stages, with each one's log tail:"
    echo
    echo '```text'
    sed -n '/^gate-full: FAILED stages/,$p' "$out" | head -n 400
    echo '```'
  else
    echo "Last 80 lines:"
    echo
    echo '```text'
    tail -n 80 "$out"
    echo '```'
  fi
  echo
  echo "🤖 Filed by scripts/idle-gate-full.sh"
} >"$body"

if [ "${IDLE_GATE_NO_ISSUE:-0}" = 1 ]; then
  cat "$body"
else
  n=$(open_issue)
  if [ -n "$n" ]; then
    gh issue comment "$n" -R tfriedel/spire-mentor-private --body-file "$body"
  else
    gh issue create -R tfriedel/spire-mentor-private --title "$title" --body-file "$body"
  fi
fi
exit 1
