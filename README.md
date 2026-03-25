# eMterm

A terminal emulator for Linux and Windows, built with Tauri, featuring rich rendering capabilities including inline images and Markdown display.

## Features

- **Core Terminal**
  - Full ANSI/VT100/VT220/xterm control sequence support
  - Multi-tab terminal with independent PTY sessions
  - WASM-based terminal core for high-performance grid rendering
  - Unified ring buffer with full-buffer reflow on resize
  - Canvas 2D renderer with dirty-row tracking
  - Background Color Erase (BCE) support

- **Rich Content Display**
  - Inline Markdown rendering via custom OSC 777 extension (CommonMark, GFM, syntax highlighting, Mermaid diagrams)
  - Large document support: no file size limit, session timeout resets per chunk
  - Fullscreen Markdown viewer with outline panel (table of contents), zoom, Space/Shift+Space scrolling, and keyboard navigation
  - Mermaid diagram rendering in Markdown (flowcharts, sequence diagrams, etc.) with Chart/Code toggle toolbar
  - Inline image rendering (Kitty Graphics Protocol and SIXEL)
  - Kitty protocol compatibility: works with kitten icat, ratatui-image, treemd, and other external tools
  - Fullscreen image viewer (pixel-perfect and fit-to-window modes, pan, wheel scroll, Space/Shift+Space scrolling)
  - Viewers render within the terminal content area; tab bar remains accessible during viewing
  - CLI commands: `emterm markdown` and `emterm image` (work over SSH, CLI-only build available)
  - File download via OSC 777: streaming I/O with no file size limit, save dialog at transfer start

- **Terminal Multiplexer**
  - `emterm mux` starts a native multiplexer daemon; GUI receives raw PTY bytes (no double-parse overhead)
  - Detach (`prefix+d`) / reattach (`emterm mux attach`) with full screen state restoration
  - Pane split (`prefix+%` vertical, `prefix+"` horizontal), resize, and zoom (`prefix+z`)
  - Multiple windows per session with tab group UI
  - Copy mode with vi/emacs keybindings and WASM-based search
  - tmux.conf import: prefix key, keybindings, base-index, mouse, status-position

- **Input and IME**
  - High-throughput key input (event-based binary IPC, zero JSON serialization)
  - Full IME support: EditContext API (Chromium) and hidden textarea fallback (WebKit)
  - IME position auto-adjustment for TUI applications (cursor-hidden mode positions IME at bottom-left)
  - Capture-phase clipboard shortcuts (Ctrl+Shift+C/V) compatible with IME
  - Middle-click paste (configurable)
  - Shift+Enter as Alt+Enter for multiline input in AI interfaces (configurable, default ON)
  - Word selection drag (double-click and drag to extend by word)
  - Comprehensive special key mapping (Ctrl+symbols, modified arrow keys, F-keys, Shift+Tab)

- **Navigation**
  - OSC 133 semantic prompt jump (Ctrl+Up / Ctrl+Down)
  - Incremental text search with match highlighting (Ctrl+F)
  - Command output folding (collapse/expand)
  - File path Ctrl+click to open in editor (hover-only underline)
  - URL Ctrl+click to open in browser (hover-only underline)
  - Right-click context menu for terminal area, tabs, and tab bar

- **Settings and Appearance**
  - Settings panel with collapsible icon-rail navigation and seven categories
  - Dark/light/system theme with five accent color presets (Purple, Blue, Green, Orange, Pink)
  - Terminal color schemes: built-in presets plus fully user-customizable palette with horizontal layout
  - ANSI bold-brightens-color behavior (bold + color 0-7 uses bright variant, configurable)
  - Three-field font configuration (primary, CJK/secondary, emoji) with system font picker and clear button
  - Separate UI font setting for the settings panel
  - Terminal profiles: named shell configurations with shell, args, env vars, and working directory
  - SSH connection management: auto-detect ssh command, import from ~/.ssh/config, manage connections
  - SFTP file upload: drag and drop files onto SSH tabs to upload; non-SSH tabs paste file paths
  - WSL distribution detection, import, and profile integration (Windows only)
  - Configurable cursor shape, scrollbar, opacity, line height, scrollback, shell, and more
  - All keyboard shortcuts configurable

- **Notifications**
  - Activity dot indicator on inactive tabs when new output or process events occur
  - OS desktop notifications when the window is not focused (configurable)
  - Notification throttling to prevent spam during high-frequency output

- **Internationalization**
  - English and Japanese UI (auto-detected from OS locale)
  - Unicode 17.0 and Emoji 17.0 character width support
  - Correct ambiguous-width (EAW=A) character rendering for TUI compatibility
  - Text presentation mode forced for non-emoji Extended_Pictographic characters

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
# Build for production (Linux: deb/rpm, Windows: nsis)
make build
```

The built application will be in `src-tauri/target/release/bundle/`.

### CLI-Only Build

The CLI-only build is useful for installing on remote servers. For example, running `emterm markdown` or `emterm image` over SSH outputs control sequences that the eMterm client renders as rich content — no GUI dependencies needed on the server side.

Build only the CLI commands (`emterm markdown`, `emterm image`) without the GUI application:

```bash
cargo build --manifest-path src-tauri/Cargo.toml --release --no-default-features
```

The `gui` feature flag is enabled by default. Using `--no-default-features` excludes all GUI dependencies (Tauri, WebView, etc.) and produces a lightweight CLI binary. A pre-built `emterm-cli` deb package is also available in [Releases](https://github.com/m-m-n/emterm/releases).

### Windows Cross-Compilation (from Linux)

Cross-compile for Windows using [cargo-xwin](https://github.com/rust-cross/cargo-xwin):

```bash
make win-build
```

This runs `bun tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc`.

**Prerequisites:**

```bash
# Install cargo-xwin
cargo install cargo-xwin

# Install system dependencies (Ubuntu/Debian)
sudo apt install clang lld llvm nsis librsvg2-bin
```

- `clang`, `lld` — C/C++ cross-compiler and linker (`clang-cl`, `lld-link`)
- `llvm` — Resource compiler (`llvm-rc`)
- `nsis` — NSIS installer generator (`makensis`)
- `librsvg2-bin` — SVG to PNG icon conversion (`rsvg-convert`)

## CLI Commands

eMterm provides helper CLI commands for outputting control sequences:

```bash
# Display Markdown in terminal
emterm markdown <file.md>

# Display image in terminal
emterm image <image.png>
```

## tmux Usage Notes

### CLI Commands in tmux

Inside tmux, CLI commands (`emterm markdown`, `emterm image`) automatically wrap control sequences in DCS passthrough. Requires `allow-passthrough` enabled in tmux config:

```bash
set -g allow-passthrough on
```

### SFTP Upload Destination in tmux

When uploading files via drag & drop to an SSH tab, eMterm uses [OSC 7](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands) (working directory notification) to determine the remote upload destination. However, tmux intercepts OSC 7 from inner panes and does not forward it to the outer terminal. This causes the upload destination to default to the home directory.

To get the correct working directory in tmux, add the following to your remote shell configuration:

**bash** (`~/.bashrc`):
```bash
if [ -n "$TMUX" ]; then
  _osc7_dcs() {
    printf '\ePtmux;\e\e]7;%s\e\e\\\e\\' "$PWD"
  }
  PROMPT_COMMAND="_osc7_dcs${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
fi
```

**zsh** (`~/.zshrc`):
```zsh
if [[ -n "$TMUX" ]]; then
  _osc7_dcs() {
    printf '\ePtmux;\e\e]7;%s\e\e\\\e\\' "$PWD"
  }
  precmd_functions+=(_osc7_dcs)
fi
```

This wraps OSC 7 in a DCS passthrough sequence so tmux forwards it to eMterm.

### OSC Sequences in tmux

tmux 3.4+ natively supports OSC 8 (hyperlinks), OSC 52 (clipboard), and other standard OSC sequences. To enable these features, add the `hyperlinks` terminal feature:

```bash
set -ga terminal-features ",xterm-256color:hyperlinks"
```

> **Note:** `terminal-features` is evaluated when a client connects. After changing this setting, you must detach (`Ctrl+b d`) and reattach (`tmux attach`) for it to take effect. You can verify with `tmux display -p '#{client_termfeatures}'` — it should include `hyperlinks`.

For eMterm's custom extensions (OSC 777 for Markdown/download, OSC 1337 for iTerm2 images), use `allow-passthrough` as described above. The `emterm markdown` and `emterm image` CLI commands handle DCS wrapping automatically.

#### OSC 133 Semantic Prompt in tmux

tmux 3.4+ consumes OSC 133 markers internally for its own prompt navigation (`next-prompt`/`previous-prompt`) and does not forward them to the outer terminal. This means eMterm's Ctrl+Up/Down prompt jump and command output folding do not work inside tmux by default.

To pass OSC 133 markers through to eMterm via DCS passthrough, add the following to your shell configuration. The regular OSC 133 emitted by your shell continues to work for tmux's own prompt navigation.

**bash** (`~/.bashrc`):
```bash
if [ -n "$TMUX" ]; then
  _emterm_osc133() { printf '\ePtmux;\e\e]133;%s\e\e\\\e\\' "$1"; }
  _emterm_first=1
  _emterm_precmd() {
    local ec=$?
    if [ -z "$_emterm_first" ]; then
      _emterm_osc133 "D;$ec"
    fi
    _emterm_first=
    _emterm_osc133 "A"
  }
  PROMPT_COMMAND="_emterm_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
  _emterm_b=$'\ePtmux;\e\e]133;B\e\e\\\e\\'
  PS1="${PS1}\[${_emterm_b}\]"
  _emterm_c=$'\ePtmux;\e\e]133;C\e\e\\\e\\'
  PS0="${PS0}${_emterm_c}"
fi
```

**zsh** (`~/.zshrc`):
```zsh
if [[ -n "$TMUX" ]]; then
  _emterm_osc133() { printf '\ePtmux;\e\e]133;%s\e\e\\\e\\' "$1" }
  _emterm_first=1
  _emterm_precmd() {
    local ec=$?
    if [[ -z "$_emterm_first" ]]; then
      _emterm_osc133 "D;$ec"
    fi
    _emterm_first=
    _emterm_osc133 "A"
  }
  _emterm_preexec() { _emterm_osc133 "C" }
  precmd_functions+=(_emterm_precmd)
  preexec_functions+=(_emterm_preexec)
  PS1="${PS1}%{$(printf '\ePtmux;\e\e]133;B\e\e\\\\\e\\\\')%}"
fi
```

Requires `allow-passthrough on` in tmux config.

## OSC Sequence Support

eMterm supports the following OSC (Operating System Command) sequences:

| OSC | Name | Description |
|-----|------|-------------|
| 0 | SetTitleAndIcon | Set window title and icon name |
| 1 | SetIconName | Set icon name |
| 2 | SetTitle | Set window title |
| 4 | SetColorPalette | Query/set color palette entries |
| 7 | SetWorkingDirectory | Set current working directory (used for SFTP upload destination) |
| 8 | Hyperlink | Clickable hyperlinks (`Ctrl+click` to open) |
| 9 | Notification / Progress | Desktop notifications and progress indicator (ConEmu-compatible) |
| 10 | SetForegroundColor | Query/set default foreground color |
| 11 | SetBackgroundColor | Query/set default background color |
| 12 | SetCursorColor | Query/set cursor color |
| 22 | CursorShape | Push/pop cursor shape stack |
| 52 | Clipboard | Read/write system clipboard (configurable) |
| 104 | ResetColorPalette | Reset color palette entries to defaults |
| 110 | ResetForegroundColor | Reset default foreground color |
| 111 | ResetBackgroundColor | Reset default background color |
| 112 | ResetCursorColor | Reset cursor color |
| 133 | SemanticPrompt | Prompt/command/output zone markers (used for Ctrl+Up/Down jump, output folding) |
| 777 | eMterm Extension | Inline Markdown rendering, file download, output folding |
| 1337 | iTerm2 Protocol | Inline image display, user variables |

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

