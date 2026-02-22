#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

mkdir -p e2e-tests/screenshots

COMPOSE="docker compose -f docker-compose.e2e.yml"

case "${1:-}" in
  build)
    echo "Building Docker image..."
    $COMPOSE build
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
    echo "Removing containers..."
    $COMPOSE down --rmi local
    ;;
  "")
    # Full cycle: build image → test
    echo "=== Full E2E cycle ==="
    $COMPOSE build
    $COMPOSE run --rm e2e-test
    ;;
  *)
    echo "Usage: $0 [build|test [spec]|clean]"
    echo ""
    echo "  (no args)  Full cycle: build image → test"
    echo "  build      Rebuild Docker image"
    echo "  test       Run all E2E tests"
    echo "  test foo   Run specific spec file"
    echo "  clean      Remove containers and images"
    exit 1
    ;;
esac

echo "Done."
