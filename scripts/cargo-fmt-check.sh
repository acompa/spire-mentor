#!/usr/bin/env bash
# `cargo fmt --check` for the sts2-rs workspace, one package at a time.
#
# A single `cargo fmt --check` hands rustfmt every target root of every
# package in one command line. The workspace has outgrown the Windows limit
# (32 767 characters): there it dies with "os error 206" (file name or
# extension too long) before checking anything, which stopped `just gate` at
# its first line on every Windows checkout. Per package, each command line
# stays short; the files checked are the same.
set -euo pipefail
cd "$(dirname "$0")/../sts2-rs"
packages=$(cargo metadata --no-deps --format-version 1 \
    | uv run --no-project python -c "import json, sys; print(' '.join(p['name'] for p in json.load(sys.stdin)['packages']))")
status=0
for p in $packages; do
    cargo fmt --check -p "$p" || status=1
done
exit $status
