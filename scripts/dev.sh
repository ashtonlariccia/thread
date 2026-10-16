#!/usr/bin/env bash
# Launch Tide in dev mode (Vite HMR + Tauri).
set -euo pipefail

cd "$(dirname "$0")/.."

# shellcheck source=scripts/_npm.sh
source "$(dirname "$0")/_npm.sh"

npm_run run tauri -- dev
