# eMterm

A cross-platform terminal emulator built with Tauri, featuring rich rendering capabilities including inline images and Markdown display.

## Features

- [ ] Full ANSI control sequence support
- [ ] Kitty Graphics Protocol support
- [ ] SIXEL graphics support
- [x] Inline Markdown rendering (custom OSC extension)
- [ ] Low-latency typing performance
- [ ] Cross-platform (Linux, macOS, Windows)

## Requirements

- [Rust](https://rustup.rs/) 1.85+
- [Bun](https://bun.sh/) 1.0+
- System dependencies for Tauri (see [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/))

### Linux (Ubuntu/Debian)

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

### macOS

```bash
xcode-select --install
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

### Limitations

| Limit | Value |
|-------|-------|
| Maximum document size | 2 MB per session |
| Session timeout | 30 seconds |
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
| `bun run typecheck` | Run TypeScript type checking |
| `bun test` | Run tests |

