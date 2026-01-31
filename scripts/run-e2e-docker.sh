#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

mkdir -p e2e-tests/screenshots

COMPOSE="docker compose -f docker-compose.e2e.yml"

case "${1:-}" in
  install)
    echo "Installing dependencies..."
    $COMPOSE run --rm install
    ;;
  build)
    echo "Building application..."
    $COMPOSE run --rm build
    ;;
  test)
    if [ -n "$2" ]; then
      echo "Running E2E test: $2"
      $COMPOSE run --rm e2e-test \
        sh -c "cp -n src-tauri/icons/128x128.png /tmp/test.png 2>/dev/null || true \
        && Xvfb :99 -screen 0 1280x720x24 & sleep 2 \
        && cd e2e-tests && npx wdio run wdio.docker.conf.js --spec specs/$2"
    else
      echo "Running all E2E tests..."
      $COMPOSE run --rm e2e-test
    fi
    ;;
  clean)
    echo "Removing volumes and containers..."
    $COMPOSE down -v
    ;;
  "")
    # Full cycle: install → build → test
    echo "=== Full E2E cycle ==="
    $COMPOSE run --rm install
    $COMPOSE run --rm build
    $COMPOSE run --rm e2e-test
    ;;
  *)
    echo "Usage: $0 [install|build|test [spec]|clean]"
    echo ""
    echo "  (no args)  Full cycle: install → build → test"
    echo "  install    Install dependencies"
    echo "  build      Build application"
    echo "  test       Run all E2E tests"
    echo "  test foo   Run specific spec file"
    echo "  clean      Remove all volumes"
    exit 1
    ;;
esac

echo "Done."
