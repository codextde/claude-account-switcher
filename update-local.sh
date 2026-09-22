#!/bin/zsh
# Build the app, quit the running copy, move the new bundle into /Applications, and relaunch it.
set -euo pipefail
cd "$(dirname "$0")"

APP="Claude Account Switcher.app"
BUNDLE_ID="de.codext.claude-account-switcher"
BIN="claude-account-switcher"
BUILT="src-tauri/target/release/bundle/macos/$APP"
DEST="/Applications/$APP"

export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/claude-account-switcher.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat ~/.tauri/claude-account-switcher.password)"
pnpm tauri build --bundles app

[[ -d "$BUILT" ]] || { echo "build output not found: $BUILT" >&2; exit 1; }

# Quit the running app: ask nicely first, then kill anything left over.
if pgrep -x "$BIN" >/dev/null; then
  echo "Quitting running $APP"
  osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1 || true
  for _ in {1..20}; do
    pgrep -x "$BIN" >/dev/null || break
    sleep 0.25
  done
  pkill -x "$BIN" 2>/dev/null || true
  for _ in {1..20}; do
    pgrep -x "$BIN" >/dev/null || break
    sleep 0.25
  done
  if pgrep -x "$BIN" >/dev/null; then
    echo "$APP is still running; aborting install" >&2
    exit 1
  fi
fi

echo "Installing to $DEST"
rm -rf "$DEST"
mv "$BUILT" "$DEST"
open "$DEST"
