#!/bin/bash
# Run E2E tests in Docker container
# Usage: ./scripts/run-e2e-docker.sh [spec-file]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# Create screenshots directory if not exists
mkdir -p e2e-tests/screenshots

# Build and run
if [ -n "$1" ]; then
    # Run specific spec file
    echo "Running E2E test: $1"
    docker compose -f docker-compose.e2e.yml run --rm e2e-test \
        sh -c "Xvfb :99 -screen 0 1280x720x24 & sleep 2 && cd e2e-tests && npx wdio run wdio.docker.conf.js --spec specs/$1"
else
    # Run all tests
    echo "Running all E2E tests..."
    docker compose -f docker-compose.e2e.yml up --build --abort-on-container-exit
fi

echo "Done. Screenshots saved to: e2e-tests/screenshots/"
