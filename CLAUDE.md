# CLAUDE.md

eMterm is a cross-platform terminal emulator built with Tauri, featuring rich rendering capabilities including inline images and Markdown display.

## What (Technology Stack)

**Primary Technologies:**
- Rust (Tauri backend) - PTY handling, ANSI parsing, app logic
- Vanilla TypeScript (frontend) - Terminal UI, WebView rendering
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

**⚠️ テスト実行はDocker環境を優先すること。** ホスト環境の設定ファイルやキャッシュを破損するリスクを避けるため、テストは原則Docker内で実行する。

**Docker経由（推奨）:**
```bash
# Rust テスト
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"

# TypeScript テスト
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"

# TypeScript 型チェック
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"

# E2E テスト（フルサイクル）
./scripts/run-e2e-docker.sh
```

**ホスト直接実行（開発者が明示的に許可した場合のみ）:**
```bash
cargo test --manifest-path src-tauri/Cargo.toml
bun test
bun run typecheck
```

### Project Structure

```
src-tauri/         - Rust backend (Tauri core, PTY, ANSI parser)
src/               - TypeScript frontend (terminal UI, rendering)
tmp/               - Temporary files and drafts
```

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
