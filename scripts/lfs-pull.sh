#!/usr/bin/env bash
# Serialised `git lfs pull` (GH #1162). The SessionStart hooks
# (claude_code_web_init.sh and the data unpack.sh scripts) start in parallel,
# and concurrent pulls race on .git/index.lock: one exits 128 and the caller's
# `|| true` hides it, leaving pointer stubs behind. Every hook pulls through
# this wrapper, which holds one lock per repository.
#
# Cloud sessions: the agent proxy adds the GitHub credentials to LFS API
# requests, but only to requests LFS actually sends. Once git-lfs has cached
# `lfs.<url>.access = basic` in .git/config (it records that after a 401), it
# asks a credential helper for a username BEFORE sending anything, finds none
# ("could not read Username ... terminal prompts disabled" / "Git credentials
# for ... not found") and gives up, so every later pull fails and the DLLs
# stay ~130-byte pointer stubs. In a cloud session we therefore drop any
# cached `access` for the remote before pulling, and never prompt. Local
# checkouts are left alone: there `access = basic` is real and LFS re-learns
# it anyway.
#
# Usage: scripts/lfs-pull.sh <dir-in-repo> [git lfs pull args...]
set -u
DIR="$1"; shift
command -v git-lfs >/dev/null 2>&1 || exit 0
COMMON="$(git -C "$DIR" rev-parse --path-format=absolute --git-common-dir 2>/dev/null)" || exit 0
if [ "${CLAUDE_CODE_REMOTE:-}" = "true" ]; then
  export GIT_TERMINAL_PROMPT=0
  git -C "$DIR" config --get-regexp '^lfs\..*\.access$' 2>/dev/null \
    | while read -r key _; do git -C "$DIR" config --unset-all "$key" || true; done
fi
if command -v flock >/dev/null 2>&1; then
  exec flock "$COMMON/lfs-pull.lock" git -C "$DIR" lfs pull "$@"
fi
exec git -C "$DIR" lfs pull "$@"
