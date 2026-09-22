#!/bin/zsh
# Build the app and replace the copy in /Applications, then relaunch it.
set -euo pipefail
cd "$(dirname "$0")"
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/claude-account-switcher.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/claude-account-switcher.password)"
pnpm tauri build --bundles app
APP="Claude Account Switcher.app"
pkill -x claude-account-switcher || true
rm -rf "/Applications/$APP"
cp -R "src-tauri/target/release/bundle/macos/$APP" /Applications/
open "/Applications/$APP"
