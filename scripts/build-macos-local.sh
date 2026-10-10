#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This build script requires macOS." >&2
  exit 1
fi

project_root="$(cd "$(dirname "$0")/.." && pwd)"
app_path="$project_root/src-tauri/target/release/bundle/macos/KeyClaim.app"

cd "$project_root"
node scripts/build-release.mjs --bundles app

if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  codesign \
    --force \
    --deep \
    --sign - \
    --identifier com.feige.keyclaim \
    -r='designated => identifier "com.feige.keyclaim"' \
    "$app_path"
fi

codesign --verify --deep --strict "$app_path"
codesign -d -r- "$app_path" 2>&1
