# eMterm

A cross-platform terminal emulator built with Tauri, featuring rich rendering capabilities including inline images and Markdown display.

## Features

- [ ] Full ANSI control sequence support
- [ ] Kitty Graphics Protocol support
- [ ] SIXEL graphics support
- [ ] Inline Markdown rendering (custom OSC extension)
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

## Project Structure

```
emterm/
├── src/                    # Frontend (TypeScript)
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

