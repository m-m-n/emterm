#!/bin/sh
set -e

FORCE_REBUILD=0
if [ "$1" = "--rebuild" ]; then
    FORCE_REBUILD=1
    shift
fi

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

# Tauri app binary: build if missing or --rebuild
BINARY="/app/src-tauri/target/debug/emterm"
if [ "$FORCE_REBUILD" = "1" ] || [ ! -f "$BINARY" ]; then
    if [ "$FORCE_REBUILD" = "1" ]; then
        echo "[entrypoint] Rebuilding Tauri app (--rebuild)..."
    else
        echo "[entrypoint] Building Tauri app..."
    fi
    bun tauri build --debug --no-bundle
fi

exec "$@"
