#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
SKILLS_SRC_DIR="${SKILLS_SRC_DIR:-$ROOT_DIR/../skills}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/src-tauri/resources/skills}"

if [ ! -d "$SKILLS_SRC_DIR" ]; then
  echo "skills source dir not found: $SKILLS_SRC_DIR" >&2
  exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

for skill_dir in "$SKILLS_SRC_DIR"/*; do
  if [ ! -d "$skill_dir" ]; then
    continue
  fi
  skill_name="$(basename "$skill_dir")"
  cp -R "$skill_dir" "$OUTPUT_DIR/$skill_name"
done

echo "bundled skills ready: $OUTPUT_DIR"
