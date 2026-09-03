#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-run}"
APP_NAME="KeyClaim"
PROCESS_NAME="keyclaim"
BUNDLE_ID="com.feige.keyclaim"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_BUNDLE="$ROOT_DIR/src-tauri/target/release/bundle/macos/$APP_NAME.app"
APP_BINARY="$ROOT_DIR/src-tauri/target/release/$PROCESS_NAME"

pkill -x "$PROCESS_NAME" >/dev/null 2>&1 || true

cd "$ROOT_DIR"
npm run tauri build -- --bundles app

# 本机没有 Developer ID 时也使用稳定的 designated requirement，避免每次构建都
# 生成以二进制哈希为身份的临时签名。正式分发仍应使用 Developer ID 并完成公证。
if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  /usr/bin/codesign --force --deep --sign - \
    --identifier "$BUNDLE_ID" \
    --requirements "=designated => identifier \"$BUNDLE_ID\"" \
    "$APP_BUNDLE"
fi
/usr/bin/codesign --verify --deep --strict "$APP_BUNDLE"

open_app() {
  /usr/bin/open -n "$APP_BUNDLE"
}

case "$MODE" in
  run)
    open_app
    ;;
  --debug|debug)
    lldb -- "$APP_BINARY"
    ;;
  --logs|logs)
    open_app
    /usr/bin/log stream --info --style compact --predicate "process == \"$PROCESS_NAME\""
    ;;
  --telemetry|telemetry)
    open_app
    /usr/bin/log stream --info --style compact --predicate "subsystem == \"$BUNDLE_ID\""
    ;;
  --verify|verify)
    open_app
    sleep 1
    pgrep -x "$PROCESS_NAME" >/dev/null
    ;;
  *)
    echo "usage: $0 [run|--debug|--logs|--telemetry|--verify]" >&2
    exit 2
    ;;
esac
