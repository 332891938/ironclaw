#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CHANNELS_SRC_DIR="${CHANNELS_SRC_DIR:-$ROOT_DIR/../channels-src}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/src-tauri/resources/channels}"
CHANNELS=(discord feishu slack telegram whatsapp)

if [ ! -d "$CHANNELS_SRC_DIR" ]; then
  echo "channels source dir not found: $CHANNELS_SRC_DIR" >&2
  exit 1
fi

if ! command -v wasm-tools >/dev/null 2>&1; then
  echo "wasm-tools not found, installing..."
  cargo install wasm-tools --locked
fi

rustup target add wasm32-wasip2 >/dev/null

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

for channel in "${CHANNELS[@]}"; do
  CHANNEL_DIR="$CHANNELS_SRC_DIR/$channel"
  if [ ! -d "$CHANNEL_DIR" ]; then
    echo "skip missing channel dir: $CHANNEL_DIR"
    continue
  fi
  echo "building channel: $channel"
  (cd "$CHANNEL_DIR" && bash ./build.sh)
  WASM_FILE="$CHANNEL_DIR/$channel.wasm"
  CAP_FILE="$CHANNEL_DIR/$channel.capabilities.json"
  if [ ! -f "$WASM_FILE" ]; then
    echo "missing wasm output: $WASM_FILE" >&2
    exit 1
  fi
  if [ ! -f "$CAP_FILE" ]; then
    echo "missing capabilities output: $CAP_FILE" >&2
    exit 1
  fi
  cp "$WASM_FILE" "$OUTPUT_DIR/$channel.wasm"
  cp "$CAP_FILE" "$OUTPUT_DIR/$channel.capabilities.json"
done

echo "bundled channels ready: $OUTPUT_DIR"
