#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v npm >/dev/null 2>&1; then
  echo "npm is required but not found"
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required but not found"
  exit 1
fi

echo "[sillyvpn] Installing npm dependencies..."
cd "$ROOT_DIR"
npm install

echo "[sillyvpn] Building AppImage..."
export LINUXDEPLOY_AUTO_STRIP=0
export STRIP=:
export NO_STRIP=1
STRIP_WRAPPER_DIR="$(mktemp -d)"
cat <<'STRIP' > "${STRIP_WRAPPER_DIR}/strip"
#!/usr/bin/env sh
exit 0
STRIP
chmod +x "${STRIP_WRAPPER_DIR}/strip"
export PATH="${STRIP_WRAPPER_DIR}:$PATH"
npm run tauri build

echo "[sillyvpn] Done. Output: src-tauri/target/release/bundle/appimage/"
