#!/usr/bin/env bash
# Build the standalone Thread executable.
#
# This is the binary to double-click. Unlike a debug build, it embeds the UI, so
# it needs no dev server -- and it runs without a console window.
#
# Pass --bundle to also produce the NSIS installer.
set -euo pipefail

cd "$(dirname "$0")/.."

# shellcheck source=scripts/_npm.sh
source "$(dirname "$0")/_npm.sh"

if [[ "${1:-}" == "--bundle" ]]; then
  npm_run run tauri -- build
else
  npm_run run tauri -- build --no-bundle
fi

echo
echo "==> standalone executable:"
echo "    $(pwd)/target/release/thread.exe"
