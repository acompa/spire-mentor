#!/usr/bin/env bash
# e2e-phone.sh — `just e2e-phone [spec…]`: the server-backed phone and tablet
# Playwright suites (GH #1440). No gate ran them before, and 14 of their tests
# were red on main before anyone noticed (fixed in #1436).
#
# Playwright's `webServer` (web/playwright.config.ts) starts the Python
# backend and vite. This script gives both a free port, so a run never reuses
# another worktree's backend or vite, and runs the specs serially (one backend
# holds one live session, so no parallel workers). The whole set takes about
# 50-75 minutes. It queues behind other heavy suites (scripts/heavy-suite.sh).
#
# No server outlives the run. Playwright stops its servers when it exits
# normally, but not when it is killed: a backend left behind that way held a
# Python process and its venv open for hours. So on exit the script stops
# Playwright's whole process tree and whatever still listens on its two
# ports. It records the ports while it runs, so the next run stops what a run
# killed too hard to clean up left behind. A Playwright interrupted along with
# the session that started it was also seen hanging for good, holding the
# heavy-suite lock and both servers, so a run whose output stalls for
# E2E_PHONE_STALL_S (default 900; one test times out at 120 s) is stopped.
#
# With no arguments it runs the six phone/tablet specs; arguments replace
# that list and pass through to `playwright test` (`just e2e-phone
# e2e/phone-fight.spec.ts -g Bash`).
set -euo pipefail

pycli="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
root="$(cd "$pycli/.." && pwd)"
# Per run: `<pid>.ports` holding "<backend port> <vite port>", and
# `<pid>.out`, Playwright's output (for the stall check).
state_dir="${STS2_E2E_PHONE_STATE_DIR:-/tmp/sts2-e2e-phone}"
stall_s="${E2E_PHONE_STALL_S:-900}"

# Stop the process listening on PORT if its command line contains PATTERN
# (the backend's -X marker, or vite), so a port some other program took
# since is left alone.
stop_listener() {
  local port=$1 pattern=$2 pid
  [ "$port" -gt 0 ] || return 0
  if is_windows; then
    powershell.exe -NoProfile -NonInteractive -Command "
      Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue | ForEach-Object {
        \$p = Get-CimInstance Win32_Process -Filter \"ProcessId=\$(\$_.OwningProcess)\"
        if (\$p -and \$p.CommandLine -and \$p.CommandLine.Contains('$pattern')) {
          Write-Output \"e2e-phone: stopping \$(\$p.Name) \$(\$p.ProcessId) on :$port\"
          Stop-Process -Id \$p.ProcessId -Force -ErrorAction SilentlyContinue
        }
      }" || true
  else
    for pid in $(ss -Hltnp "sport = :$port" 2>/dev/null | grep -o 'pid=[0-9]*' | cut -d= -f2 | sort -u); do
      if tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null | grep -qF -- "$pattern"; then
        echo "e2e-phone: stopping pid $pid on :$port"
        kill "$pid" 2>/dev/null || true
      fi
    done
  fi
}

is_windows() {
  case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) return 0 ;; esac
  return 1
}

# Stop Playwright and everything it started (runner, workers, browsers,
# servers): a killed `npx` alone leaves the runner behind.
kill_tree() { # PID of this shell's background child
  local w
  if is_windows; then
    w=$(cat "/proc/$1/winpid" 2>/dev/null) && taskkill //T //F //PID "$w" >/dev/null 2>&1
  else
    kill -TERM -- "-$1" 2>/dev/null || kill -TERM "$1" 2>/dev/null
  fi
  return 0
}

stop_servers() { # BACKEND_PORT VITE_PORT
  stop_listener "$1" sts2_e2e_backend
  stop_listener "$2" vite
}

if [ "${E2E_PHONE_STAGE:-}" != run ]; then
  # The backend imports sts2_rs from .venv: bring it up to date with this
  # checkout first, or the run tests whatever engine the venv last built.
  (cd "$pycli" && uv sync --frozen --quiet)
  cd "$pycli/web"
  [ -d node_modules ] || npm ci
  # Playwright's own Chromium, unless the host ships one the configs use
  # instead (web/playwright.chromium.ts; a cloud container must not install).
  # A no-op when it is already installed.
  if [ -z "${PLAYWRIGHT_CHROMIUM_PATH:-}" ] && [ ! -e /opt/pw-browsers/chromium ]; then
    npx playwright install chromium
  fi
  # The rest runs under the lock, which can wait for hours: ports picked
  # before it might be taken by the time the run starts.
  export E2E_PHONE_STAGE=run
  exec "$root/scripts/heavy-suite.sh" bash "$pycli/scripts/e2e-phone.sh" "$@"
fi

specs=(
  e2e/phone-chrome.spec.ts
  e2e/phone-fight.spec.ts
  e2e/phone-layout.spec.ts
  e2e/phone-screens.spec.ts
  e2e/phone-portrait-split.spec.ts
  e2e/tablet-landscape.spec.ts
)
if [ "$#" -gt 0 ]; then
  specs=("$@")
fi
cd "$pycli/web"

# What an earlier run left behind: its pid is gone, its servers may not be.
mkdir -p "$state_dir"
for f in "$state_dir"/*.ports; do
  [ -e "$f" ] || continue
  pid=$(basename "$f" .ports)
  kill -0 "$pid" 2>/dev/null && continue # still running (the lock was off)
  read -r bport vport <"$f" || true
  [ -n "${bport:-}" ] && stop_servers "$bport" "${vport:-0}"
  rm -f "$f" "${f%.ports}.out"
done

free_port() {
  node -e 'const s = require("net").createServer();
s.listen(0, "127.0.0.1", () => { console.log(s.address().port); s.close(); });'
}
# Ports someone set are theirs: Playwright reuses a server already there,
# and this script must not stop it.
own_backend="" own_vite=""
if [ -z "${STS2_E2E_BACKEND_PORT:-}" ]; then
  STS2_E2E_BACKEND_PORT=$(free_port)
  own_backend=$STS2_E2E_BACKEND_PORT
fi
if [ -z "${STS2_E2E_BASE:-}" ]; then
  vite_port=$(free_port)
  while [ "$vite_port" = "$STS2_E2E_BACKEND_PORT" ]; do vite_port=$(free_port); done
  STS2_E2E_BASE="http://127.0.0.1:$vite_port"
  own_vite=$vite_port
fi
export STS2_E2E_BACKEND_PORT STS2_E2E_BASE
state="$state_dir/$$.ports"
echo "${own_backend:-0} ${own_vite:-0}" >"$state"
echo "e2e-phone: backend :$STS2_E2E_BACKEND_PORT, vite $STS2_E2E_BASE, ${#specs[@]} spec arg(s)"

child=""
cleanup() {
  [ -n "$child" ] && kill_tree "$child"
  stop_servers "${own_backend:-0}" "${own_vite:-0}"
  rm -f "$state" "${out:-}"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# In the background (its own process group on Linux, for kill_tree), so a
# signal to this script is acted on at once: bash runs a trap only after a
# foreground child returns.
out="$state_dir/$$.out"
: >"$out"
is_windows || set -m
npx playwright test --workers 1 "${specs[@]}" > >(tee -a "$out") 2>&1 &
child=$!
set +m
while kill -0 "$child" 2>/dev/null; do
  if (($(date +%s) - $(stat -c %Y "$out") > stall_s)); then
    echo "e2e-phone: no output from Playwright for ${stall_s} s: stopping it" >&2
    exit 124
  fi
  sleep 5 &
  wait $! || true
done
rc=0
wait "$child" || rc=$?
child=""
exit "$rc"
