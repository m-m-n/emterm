#!/bin/sh
set -e

# node_modules が空なら bun install
if [ ! -d "/app/node_modules/typescript" ]; then
    echo "[entrypoint] Installing dependencies..."
    bun install
fi

# e2e-tests/node_modules が空なら npm ci
if [ ! -d "/app/e2e-tests/node_modules/.package-lock.json" ]; then
    echo "[entrypoint] Installing E2E dependencies..."
    cd /app/e2e-tests && npm ci --legacy-peer-deps
    cd /app
fi

# wasm/pkg が空なら wasm-pack build
if [ ! -f "/app/wasm/pkg/emterm_wasm.js" ]; then
    echo "[entrypoint] Building WASM package..."
    cd /app/wasm && wasm-pack build --target web --out-dir pkg
    cd /app
fi

exec "$@"
