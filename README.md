# eMterm

A terminal emulator for Linux and Windows, built with Tauri, featuring rich rendering capabilities including inline images and Markdown display.

## Features

- **Core Terminal**
  - Full ANSI/VT100/VT220/xterm control sequence support
  - Multi-tab terminal with independent PTY sessions
  - WASM-based terminal core for high-performance grid rendering
  - Unified ring buffer with full-buffer reflow on resize
  - Canvas 2D renderer with dirty-row tracking

- **Rich Content Display**
  - Inline Markdown rendering via custom OSC 777 extension (CommonMark, GFM, syntax highlighting, Mermaid diagrams)
  - Large document support: no file size limit, session timeout resets per chunk
  - Fullscreen Markdown viewer with outline panel (table of contents), zoom, and keyboard navigation
  - Mermaid diagram rendering in Markdown (flowcharts, sequence diagrams, etc.) with Chart/Code toggle toolbar
  - Inline image rendering (Kitty Graphics Protocol and SIXEL)
  - Kitty protocol compatibility: works with kitten icat, ratatui-image, treemd, and other external tools
  - Fullscreen image viewer (pixel-perfect and fit-to-window modes, pan, wheel scroll)
  - CLI commands: `emterm markdown` and `emterm image` (work over SSH)

- **Input and IME**
  - High-throughput key input (event-based binary IPC, zero JSON serialization)
  - Full IME support: EditContext API (Chromium) and hidden textarea fallback (WebKit)
  - Capture-phase clipboard shortcuts (Ctrl+Shift+C/V) compatible with IME
  - Middle-click paste (configurable)
  - Shift+Enter as Alt+Enter for multiline input in AI interfaces (configurable, default ON)
  - Word selection drag (double-click and drag to extend by word)

- **Navigation**
  - OSC 133 semantic prompt jump (Ctrl+Up / Ctrl+Down)
  - Incremental text search with match highlighting (Ctrl+F)
  - Command output folding (collapse/expand)
  - File path Ctrl+click to open in editor
  - URL Ctrl+click to open in browser

- **Settings and Appearance**
  - Settings panel with collapsible icon-rail navigation and seven categories
  - Dark/light/system theme with four accent color presets (Purple, Blue, Green, Orange)
  - Terminal color schemes: built-in presets plus fully user-customizable palette
  - Three-field font configuration (primary, CJK/secondary, emoji) with system font picker
  - Separate UI font setting for the settings panel
  - Terminal profiles: named shell configurations with shell, args, env vars, and working directory
  - Configurable cursor shape, scrollbar, opacity, line height, scrollback, shell, and more
  - All keyboard shortcuts configurable

- **Notifications**
  - Activity dot indicator on inactive tabs when new output or process events occur
  - OS desktop notifications when the window is not focused (configurable)
  - Notification throttling to prevent spam during high-frequency output

- **Internationalization**
  - English and Japanese UI (auto-detected from OS locale)
  - Unicode 17.0 and Emoji 17.0 character width support

## Requirements

- [Rust](https://rustup.rs/) 1.85+
- [Bun](https://bun.sh/) 1.0+
- System dependencies for Tauri (see [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/))

### Linux (Ubuntu/Debian)

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### Windows

Install [Microsoft Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) and [WebView2](https://developer.microsoft.com/en-us/microsoft-edge/webview2/).

## Installation

```bash
# Clone the repository
git clone https://github.com/yourusername/emterm.git
cd emterm

# Install dependencies
bun install
```

## Development

```bash
# Start the development server and Tauri app
bun run tauri:dev
```

## Build

```bash
# Build for production
bun run tauri:build
```

The built application will be in `src-tauri/target/release/`.

## CLI Commands

eMterm provides helper CLI commands for outputting control sequences:

```bash
# Display Markdown in terminal
emterm markdown <file.md>

# Display image in terminal
emterm image <image.png>
```

## Markdown Display

eMterm supports inline Markdown rendering via a custom OSC 777 extension protocol.

### Supported Features

- **CommonMark** and **GitHub Flavored Markdown (GFM)** formats
- Syntax highlighting with highlight.js (180+ languages)
- Mermaid diagram rendering (flowcharts, sequence diagrams, etc.)
- XSS protection via DOMPurify sanitization
- Theme synchronization with terminal colors (dark/light mode)
- Virtual scrolling for large documents

### Limits

| Parameter | Value |
|-----------|-------|
| Chunk size | 128 KB (Base64-encoded) |
| WASM OSC buffer limit | 16 MB per sequence |
| Session timeout | 30 seconds (reset on each chunk) |
| Maximum concurrent sessions | 10 |

### Protocol

Markdown content is transferred using OSC 777 control sequences:

```
ESC ] 777 ; emterm ; markdown ; <verb> ; <params...> ST
```

**Verbs:**
- `begin` - Start a new session (`id=<uuid>`, `format=commonmark|gfm`)
- `chunk` - Send Base64-encoded content (`id=<uuid>`, `seq=<n>`, `data=<base64>`)
- `end` - Complete and render (`id=<uuid>`)

### Usage Examples

**From command line:**
```bash
emterm markdown document.md
```

**Programmatic (works over SSH):**
```bash
#!/bin/bash
ID=$(uuidgen)
echo -ne "\e]777;emterm;markdown;begin;id=$ID;format=gfm\e\\"
echo -ne "\e]777;emterm;markdown;chunk;id=$ID;seq=0;data=$(base64 -w0 < doc.md)\e\\"
echo -ne "\e]777;emterm;markdown;end;id=$ID\e\\"
```

## Project Structure

```
emterm/
├── src/                    # Frontend (TypeScript)
│   ├── terminal/           # Terminal emulator core
│   ├── markdown/           # Markdown rendering module
│   ├── index.html
│   ├── main.ts
│   └── styles.css
├── src-tauri/              # Backend (Rust)
│   ├── src/
│   │   ├── main.rs
│   │   └── lib.rs
│   ├── Cargo.toml
│   └── tauri.conf.json
├── serve.ts                # Development server
├── package.json
└── tsconfig.json
```

## Scripts

| Command | Description |
|---------|-------------|
| `bun run dev` | Start frontend development server |
| `bun run tauri:dev` | Start Tauri app in development mode |
| `bun run tauri:build` | Build for production |
| `bun run icons` | Generate Tauri icon assets |
| `bun run typecheck` | Run TypeScript type checking |
| `bun test` | Run tests |

