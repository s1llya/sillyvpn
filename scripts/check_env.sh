#!/usr/bin/env bash
set -euo pipefail

missing=0

check() {
  local name="$1"
  if ! command -v "$name" >/dev/null 2>&1; then
    echo "[missing] $name"
    missing=1
  else
    echo "[ok] $name"
  fi
}

check npm
check cargo
check wg
check wg-quick
check ip
check iptables
check pkexec

echo ""
if [[ "$missing" -ne 0 ]]; then
  echo "Missing dependencies detected. Install them and re-run."
  exit 1
fi

echo "All required CLI tools are available."
