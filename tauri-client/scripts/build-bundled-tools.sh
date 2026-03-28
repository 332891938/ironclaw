#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TOOLS_SRC_DIR="${TOOLS_SRC_DIR:-$ROOT_DIR/../tools-src}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/src-tauri/resources/tools}"

if [ ! -d "$TOOLS_SRC_DIR" ]; then
  echo "tools source dir not found: $TOOLS_SRC_DIR" >&2
  exit 1
fi

rustup target add wasm32-wasip2 >/dev/null

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

for tool_dir in "$TOOLS_SRC_DIR"/*; do
  if [ ! -d "$tool_dir" ]; then
    continue
  fi
  if [ ! -f "$tool_dir/Cargo.toml" ]; then
    continue
  fi

  tool_slug="$(basename "$tool_dir")"
  echo "building tool: $tool_slug"
  (cd "$tool_dir" && cargo build --target wasm32-wasip2 --release)

  wasm_file="$(find "$tool_dir/target/wasm32-wasip2/release" -maxdepth 1 -type f -name '*.wasm' -print | head -n 1)"
  if [ -z "$wasm_file" ]; then
    echo "missing wasm output for tool: $tool_slug" >&2
    exit 1
  fi

  cap_file="$(find "$tool_dir" -maxdepth 1 -type f -name '*.capabilities.json' -print | head -n 1)"
  if [ -z "$cap_file" ]; then
    echo "missing capabilities output for tool: $tool_slug" >&2
    exit 1
  fi

  cp "$wasm_file" "$OUTPUT_DIR/$tool_slug.wasm"
  cp "$cap_file" "$OUTPUT_DIR/$tool_slug.capabilities.json"
done

echo "bundled tools ready: $OUTPUT_DIR"
