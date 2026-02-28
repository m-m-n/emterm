# CLAUDE.md

eMterm is a terminal emulator for Linux and Windows, built with Tauri, featuring rich rendering capabilities including inline images and Markdown display.

## What (Technology Stack)

**Primary Technologies:**
- Rust (Tauri backend) - PTY management, image processing, IPC
- Rust/WebAssembly (WASM) - ANSI parsing, grid state, Unicode processing
- Vanilla TypeScript (frontend) - Terminal UI, Canvas rendering, event handling
- Bun - Package manager and bundler

**Project Type:** Desktop application (Tauri)

**Key Features:**
- Full ANSI control sequence support
- Kitty Graphics Protocol / SIXEL for inline images
- Custom OSC extension for Markdown rendering
- WebView-based rich content display

## Why (Project Purpose)

A modern terminal emulator that combines traditional terminal reliability with rich content rendering. It enables displaying images and formatted Markdown directly in the terminal via control sequences, while maintaining low-latency typing performance.

**Design Philosophy:**
- AI-first: Built for the AI era, prioritizing compatibility with AI coding tools like Claude Code
- Explicit display commands only (no auto-detection)
- Stateless CLI design (works over SSH)
- Robust isolation (XSS protection, resource management)

## How (Development Workflow)

### Setup

```bash
bun install
```

### Running the Project

**Development:**
```bash
bun tauri dev
```

**Build:**
```bash
bun tauri build
```

### Testing & Verification

**⚠️ Prefer Docker for test execution.** Running tests on the host risks corrupting local config files and caches.

**Docker (recommended):**
```bash
# Rust tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"

# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# TypeScript typecheck
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"

# E2E tests (full cycle)
./scripts/run-e2e-docker.sh

# Reset build caches (after Dockerfile.e2e changes or stale state)
docker compose -f docker-compose.e2e.yml down -v
```

**Host execution (only when explicitly permitted by the developer):**
```bash
cargo test --manifest-path src-tauri/Cargo.toml
bun test
bun run typecheck
```

**E2E verification (Docker + tauri-driver):**

Always use Docker for UI verification during implementation and debugging. Never run GUI tests on the host.

```bash
# Build Docker image (first time or after Dockerfile.e2e changes)
./scripts/run-e2e-docker.sh build

# Run all E2E tests
./scripts/run-e2e-docker.sh test

# Run a specific spec (primary method during development)
./scripts/run-e2e-docker.sh test terminal.e2e.js

# Full cycle (build image → run tests)
./scripts/run-e2e-docker.sh
```

Architecture: `Xvfb (virtual display)` → `tauri-driver` → `WebKitWebDriver` → `WebdriverIO`. Screenshots are saved to `e2e-tests/screenshots/`.

Notes for writing E2E specs:
- Place spec files in `e2e-tests/specs/` as `*.e2e.js`
- Docker config: `e2e-tests/wdio.docker.conf.js`
- Timeouts are set to 180s for the Docker environment
- `tauri:options` points to the pre-built debug binary inside the container

### Project Structure

```
src-tauri/         - Rust backend (Tauri core, PTY, image processing)
src/               - TypeScript frontend (terminal UI, rendering)
wasm/              - Rust WASM module (ANSI parser, grid, Unicode)
tmp/               - Temporary files and drafts
```

### Architecture & Development Policy

**Backend (Rust / `src-tauri/`)**

The backend handles OS-level operations and resource-intensive processing that requires native performance or system access:

- **PTY management** - Session lifecycle, shell spawning, lock-free writer threads via channels
- **Image processing** - Kitty/SIXEL decoding, LRU cache (320MB quota), animation frames
- **IPC bridge** - Tauri commands/events, binary PTY data streaming via Channel
- **CLI commands** - `emterm markdown`, `emterm image` (stateless, pipeable)
- **Settings & i18n** - Config persistence, validation with `rust-i18n`

Key conventions:
- Synchronous Tauri commands for hot paths (e.g., `pty_write`) to avoid async overhead
- `Arc<RwLock<HashMap>>` for session registries (read-optimized)
- Atomic operations for race-free session creation/removal
- `portable-pty` for PTY abstraction (Linux/Windows)

**Frontend (TypeScript + WASM / `src/` + `wasm/`)**

The frontend is split between WASM (performance-critical data processing) and TypeScript (UI, events, coordination).

WASM module (`wasm/src/`) owns:
- **ANSI/VT100 parser** - Full state machine for escape sequences (CSI, ESC, OSC, APC, DCS)
- **Terminal grid** - Ring buffer storage, viewport, cursor state, dirty row tracking
- **Unicode processing** - Codepoint width, emoji detection, grapheme classification
- **Control sequence handlers** - C0, CSI cursor/screen/edit/scroll/modes, ESC, OSC

TypeScript (`src/`) owns:
- **Canvas 2D rendering** - Differential drawing based on WASM dirty flags
- **Event handling** - Keyboard, mouse, IME, resize
- **UI components** - Tab bar, settings panel, image viewer, markdown renderer, selection
- **Application orchestration** - TerminalApp, TerminalState, PTY client
- **WASM integration layer** - `src/terminal/wasm/` (loader, proxies, adapters)

Data flow: `PTY (Rust) → Binary Channel → PtyClient (TS) → process_pty_data (WASM) → callbacks (TS) → Canvas render (TS)`

**WASM Adoption Criteria**

Use WASM when:
- Processing runs on **every byte of PTY output** (parser, grid updates, Unicode width)
- The operation is **CPU-bound with tight loops** (escape sequence state machine, cell iteration)
- **Data stays within WASM** across multiple operations (grid read/write without crossing boundary)
- The logic is **algorithmically complex** and benefits from Rust's type safety (parser state machine, ring buffer)

Keep in TypeScript when:
- The code **interacts with DOM/Canvas/Browser APIs** (rendering, clipboard, notifications)
- The operation is **event-driven and infrequent** (user input, settings, tab management)
- It requires **Tauri API access** (IPC commands, file dialogs, system tray)
- The data **crosses the WASM boundary per-call** with no batching benefit (one-shot lookups)

### UI Design Guidelines

UI implementation MUST follow `doc/UI-DESIGN-GUIDELINES.yaml`. This file defines:
- Design tokens (shape, motion, color-roles, typography, spacing)
- Component specifications (classes, properties, states, variants)
- Z-index scale
- Known issues

When implementing or modifying UI components, always refer to this file for correct token values, component dimensions, and state styles.

When UI design changes are made (new components, modified styles, updated tokens), `doc/UI-DESIGN-GUIDELINES.yaml` MUST be updated to reflect the changes. Run `/gen-design-guidelines` to update.

### Logging

Unified logging format with origin labels (`[LEVEL][ORIGIN]`):

**Frontend (TypeScript)** - brighter colors:
- `console.debug()` → `[DEBUG][FRONTEND]` (bright gray)
- `console.log()` → `[LOG][FRONTEND]` (bright green)
- `console.info()` → `[INFO][FRONTEND]` (bright cyan)
- `console.warn()` → `[WARN][FRONTEND]` (bright yellow, stderr)
- `console.error()` → `[ERROR][FRONTEND]` (bright red, stderr)

**Backend (Rust)** - dimmer colors:
- `log::debug!()` → `[DEBUG][BACKEND]` (dim gray)
- `log::info!()` → `[INFO][BACKEND]` (cyan)
- `log::warn!()` → `[WARN][BACKEND]` (yellow, stderr)
- `log::error!()` → `[ERROR][BACKEND]` (red, stderr)

## CLI Commands

The application provides helper CLI commands:
- `emterm` - Terminal application
- `emterm markdown` - Output Markdown display sequences to stdout
- `emterm image` - Output image display sequences to stdout

**tmux support:** Inside tmux, CLI commands automatically wrap sequences in DCS passthrough (`ESC P tmux; ... ESC \`). Requires `set -g allow-passthrough on` in tmux config.
