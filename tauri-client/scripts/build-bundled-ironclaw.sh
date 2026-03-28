#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT_ROOT="$(cd "$ROOT_DIR/.." && pwd)"
BIN_ROOT_DIR="${BIN_ROOT_DIR:-$ROOT_DIR/src-tauri/resources/bin}"
BUILD_MODE="${IRONCLAW_BUILD_MODE:-bundle}"

target_bin_name() {
  local target="$1"
  if [[ "$target" == *windows* ]]; then
    echo "ironclaw.exe"
  else
    echo "ironclaw"
  fi
}

target_artifact_dir() {
  local target="$1"
  case "$target" in
    x86_64-unknown-linux-gnu) echo "linux-amd64" ;;
    aarch64-apple-darwin) echo "macos-arm64" ;;
    x86_64-pc-windows-msvc|x86_64-pc-windows-gnu) echo "windows-amd64" ;;
    *) echo "$target" ;;
  esac
}

resolve_current_target() {
  local env_target="${IRONCLAW_TARGET_TRIPLE:-${CARGO_BUILD_TARGET:-${TAURI_ENV_TARGET_TRIPLE:-}}}"
  if [ -n "$env_target" ]; then
    echo "$env_target"
    return
  fi
  local os_name
  os_name="$(uname -s)"
  local arch_name
  arch_name="$(uname -m)"
  case "$os_name/$arch_name" in
    Darwin/arm64) echo "aarch64-apple-darwin" ;;
    Linux/x86_64) echo "x86_64-unknown-linux-gnu" ;;
    MINGW*/x86_64|MSYS*/x86_64|CYGWIN*/x86_64) echo "x86_64-pc-windows-msvc" ;;
    *) echo "" ;;
  esac
}

resolve_host_target() {
  local os_name
  os_name="$(uname -s)"
  local arch_name
  arch_name="$(uname -m)"
  case "$os_name/$arch_name" in
    Darwin/arm64) echo "aarch64-apple-darwin" ;;
    Linux/x86_64) echo "x86_64-unknown-linux-gnu" ;;
    MINGW*/x86_64|MSYS*/x86_64|CYGWIN*/x86_64) echo "x86_64-pc-windows-msvc" ;;
    *) echo "" ;;
  esac
}

build_target_binary() {
  local target="$1"
  local bin_name
  bin_name="$(target_bin_name "$target")"
  local artifact_dir
  artifact_dir="$(target_artifact_dir "$target")"
  local artifact_path="$BIN_ROOT_DIR/$artifact_dir/$bin_name"
  local host_target
  host_target="$(resolve_host_target)"
  local override_env_name="IRONCLAW_BIN_PATH_$(echo "$artifact_dir" | tr '[:lower:]-' '[:upper:]_')"
  local override_path="${!override_env_name:-}"

  if [ -n "$override_path" ]; then
    if [ ! -f "$override_path" ]; then
      echo "$override_env_name not found: $override_path" >&2
      exit 1
    fi
    mkdir -p "$BIN_ROOT_DIR/$artifact_dir"
    cp "$override_path" "$artifact_path"
    if [[ "$bin_name" != *.exe ]]; then
      chmod +x "$artifact_path"
    fi
    return
  fi

  if [ -f "$artifact_path" ]; then
    echo "skip building $target, existing binary found: $artifact_path"
    return
  fi

  if [ "$target" != "$host_target" ]; then
    echo "cross-platform compilation is disabled: target=$target host=$host_target" >&2
    echo "build this target on its own platform or provide $override_env_name" >&2
    exit 1
  fi

  cargo build --release --manifest-path "$PROJECT_ROOT/Cargo.toml" --target "$target"

  local built_path="$PROJECT_ROOT/target/$target/release/$bin_name"
  if [ ! -f "$built_path" ]; then
    echo "built binary not found: $built_path" >&2
    exit 1
  fi
  mkdir -p "$BIN_ROOT_DIR/$artifact_dir"
  cp "$built_path" "$artifact_path"
  if [[ "$bin_name" != *.exe ]]; then
    chmod +x "$artifact_path"
  fi
}

build_current_target_binary() {
  local current_target
  current_target="$(resolve_current_target)"
  if [ -z "$current_target" ]; then
    echo "cannot resolve target triple, set IRONCLAW_TARGET_TRIPLE explicitly" >&2
    exit 1
  fi
  build_target_binary "$current_target"
}

bundle_target_binary() {
  local target="$1"
  local bin_name
  bin_name="$(target_bin_name "$target")"
  local artifact_dir
  artifact_dir="$(target_artifact_dir "$target")"
  local artifact_path="$BIN_ROOT_DIR/$artifact_dir/$bin_name"

  if [ ! -f "$artifact_path" ]; then
    if [ -n "${IRONCLAW_BIN_PATH:-}" ]; then
      mkdir -p "$BIN_ROOT_DIR/$artifact_dir"
      cp "${IRONCLAW_BIN_PATH}" "$artifact_path"
      if [[ "$bin_name" != *.exe ]]; then
        chmod +x "$artifact_path"
      fi
    else
      build_target_binary "$target"
    fi
  fi
  echo "bundled ironclaw binary ready: $artifact_path"
}

if [ "$BUILD_MODE" = "all" ]; then
  build_current_target_binary
  echo "ironclaw binaries ready in $BIN_ROOT_DIR"
else
  TARGET_TRIPLE="$(resolve_current_target)"
  if [ -z "$TARGET_TRIPLE" ]; then
    echo "cannot resolve target triple, set IRONCLAW_TARGET_TRIPLE explicitly" >&2
    exit 1
  fi
  bundle_target_binary "$TARGET_TRIPLE"
fi
