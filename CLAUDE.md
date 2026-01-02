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

**Rust Tests:**
```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

**TypeScript Tests:**
```bash
bun test
```

**Type Check:**
```bash
bun run typecheck
```

### Project Structure

```
src-tauri/         - Rust backend (Tauri core, PTY, ANSI parser)
src/               - TypeScript frontend (terminal UI, rendering)
tmp/               - Temporary files and drafts
```

## CLI Commands

The application provides helper CLI commands:
- `emterm` - Terminal application
- `emterm markdown` - Output Markdown display sequences to stdout
- `emterm image` - Output image display sequences to stdout
