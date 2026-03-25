#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
REMOTE_HOST="${REMOTE_HOST:-ubuntu@test.axiayun.com}"
REMOTE_DIR="${REMOTE_DIR:-/var/www/ironclaw-ui}"
BUILD_DIR="${BUILD_DIR:-$ROOT_DIR/dist}"
CLIENT_BUNDLE_DIR="${CLIENT_BUNDLE_DIR:-$ROOT_DIR/src-tauri/target/release/bundle}"
WINDOWS_CLIENT_BUNDLE_DIR="${WINDOWS_CLIENT_BUNDLE_DIR:-$ROOT_DIR/src-tauri/target/x86_64-pc-windows-msvc/release/bundle}"
CLIENT_REMOTE_DIR="${CLIENT_REMOTE_DIR:-$REMOTE_DIR/client}"
BUILD_CLIENT="${BUILD_CLIENT:-1}"

cd "$ROOT_DIR"
npm run build
if [ "$BUILD_CLIENT" = "1" ]; then
  npm run tauri build
fi

if [ ! -d "$BUILD_DIR" ]; then
  echo "build output not found: $BUILD_DIR" >&2
  exit 1
fi
if [ "$BUILD_CLIENT" = "1" ] && [ ! -d "$CLIENT_BUNDLE_DIR" ]; then
  echo "client bundle output not found: $CLIENT_BUNDLE_DIR" >&2
  exit 1
fi

ssh "$REMOTE_HOST" "mkdir -p '$REMOTE_DIR' '$CLIENT_REMOTE_DIR'"
rsync -avz --delete "$BUILD_DIR"/ "$REMOTE_HOST:$REMOTE_DIR"/
if [ "$BUILD_CLIENT" = "1" ]; then
  rsync -avz --delete "$CLIENT_BUNDLE_DIR"/ "$REMOTE_HOST:$CLIENT_REMOTE_DIR"/
  if [ -d "$WINDOWS_CLIENT_BUNDLE_DIR" ]; then
    rsync -avz --delete "$WINDOWS_CLIENT_BUNDLE_DIR"/ "$REMOTE_HOST:$CLIENT_REMOTE_DIR/windows"/
  fi
fi

echo "web deploy complete: $REMOTE_HOST:$REMOTE_DIR"
if [ "$BUILD_CLIENT" = "1" ]; then
  echo "client deploy complete: $REMOTE_HOST:$CLIENT_REMOTE_DIR"
fi
