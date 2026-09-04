#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
app_path="$project_root/src-tauri/target/release/bundle/macos/KeyClaim.app"

cd "$project_root"
npm run tauri build -- --bundles app

codesign \
  --force \
  --deep \
  --sign - \
  --identifier com.feige.keyclaim \
  -r='designated => identifier "com.feige.keyclaim"' \
  "$app_path"

codesign --verify --deep --strict "$app_path"
codesign -d -r- "$app_path" 2>&1
