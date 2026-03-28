#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_ROOT="$(cd "$ROOT_DIR/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/src-tauri/resources/bin}"
TARGET_TRIPLE="${IRONCLAW_TARGET_TRIPLE:-${CARGO_BUILD_TARGET:-}}"
BIN_PATH_OVERRIDE="${IRONCLAW_BIN_PATH:-}"

if [ -n "$TARGET_TRIPLE" ] && [[ "$TARGET_TRIPLE" == *windows* ]]; then
  BIN_NAME="ironclaw.exe"
else
  BIN_NAME="ironclaw"
fi

if [ -n "$BIN_PATH_OVERRIDE" ]; then
  BIN_PATH="$BIN_PATH_OVERRIDE"
  if [ ! -f "$BIN_PATH" ]; then
    echo "IRONCLAW_BIN_PATH not found: $BIN_PATH" >&2
    exit 1
  fi
else
  BUILD_CMD=(cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml")
  if [ -n "$TARGET_TRIPLE" ]; then
    BUILD_CMD+=(--target "$TARGET_TRIPLE")
  fi
  "${BUILD_CMD[@]}"

  if [ -n "$TARGET_TRIPLE" ]; then
    BIN_PATH="$PROJECT_ROOT/target/$TARGET_TRIPLE/release/$BIN_NAME"
  else
    BIN_PATH="$PROJECT_ROOT/target/release/$BIN_NAME"
  fi
fi

if [ ! -f "$BIN_PATH" ]; then
  echo "built binary not found: $BIN_PATH" >&2
  exit 1
fi

mkdir -p "$OUTPUT_DIR"
cp "$BIN_PATH" "$OUTPUT_DIR/$BIN_NAME"
if [[ "$BIN_NAME" != *.exe ]]; then
  chmod +x "$OUTPUT_DIR/$BIN_NAME"
fi

echo "bundled ironclaw binary ready: $OUTPUT_DIR/$BIN_NAME"
