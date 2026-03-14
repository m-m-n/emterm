# Feature: OSC Comprehensive Support

## Overview

Comprehensive implementation of OSC (Operating System Command) escape sequences for eMterm. This covers completing stub implementations (OSC 4/10/11), adding widely-used sequences (OSC 12/52/9/22/104/110-112), enhancing OSC 8 hyperlinks with per-cell storage, and supporting iTerm2 protocol extensions (OSC 1337;File, OSC 1337;SetUserVar).

## Objectives

- Achieve OSC parity with major modern terminal emulators (kitty, WezTerm, foot, Alacritty)
- Enable SSH-transparent clipboard operations (OSC 52)
- Support dark/light mode auto-detection via color queries (OSC 10/11/12 `?`)
- Complete OSC 8 hyperlink support with proper per-cell storage
- Support iTerm2 inline image protocol for ecosystem compatibility

## Current State

### Fully Implemented
| OSC | Function | Status |
|-----|----------|--------|
| 0 | Set window title and icon name | Complete |
| 1 | Set icon name | Complete |
| 2 | Set window title | Complete |
| 7 | Set working directory | Complete |
| 133 | Semantic prompt (shell integration) | Complete |
| 777 | eMterm extensions (markdown/image/download/fold) | Complete |

### Stub / Partial
| OSC | Function | Status |
|-----|----------|--------|
| 4 | Color palette set | Parsed, handler is no-op |
| 8 | Hyperlink | Parsed, state tracked, **not stored per-cell** |
| 10 | Foreground color set | Parsed, handler is no-op |
| 11 | Background color set | Parsed, handler is no-op |

### Not Implemented
| OSC | Function |
|-----|----------|
| 12 | Cursor color set/query |
| 22 | Mouse cursor shape |
| 52 | Clipboard operations |
| 9 | Desktop notification / progress bar |
| 104 | Color palette reset |
| 110 | Foreground color reset |
| 111 | Background color reset |
| 112 | Cursor color reset |
| 1337;File | iTerm2 inline image |
| 1337;SetUserVar | User variable storage |

## Technical Requirements

### Functional Requirements

- **FR1: Color palette set/query (OSC 4)** — Set individual palette entries (0-255) and respond to `?` queries with current color values via PTY write-back.
- **FR2: Default foreground color set/query (OSC 10)** — Set/query the default foreground color. Support chaining with OSC 11/12.
- **FR3: Default background color set/query (OSC 11)** — Set/query the default background color. Critical for dark/light mode detection.
- **FR4: Cursor color set/query (OSC 12)** — Set/query the text cursor color.
- **FR5: Clipboard operations (OSC 52)** — Read from and write to system clipboard. Support `c` (clipboard) and `p` (primary/selection) targets. Reading is configurable (default: enabled).
- **FR6: Hyperlink per-cell storage (OSC 8)** — Store hyperlink associations in terminal cells. Render hyperlinks with underline on hover. Open on Ctrl+click. OSC 8 links take priority over auto-detected URLs.
- **FR7: Desktop notification (OSC 9)** — Display OS-native desktop notifications. Support progress bar via `OSC 9;4;state;progress ST`.
- **FR8: Mouse cursor shape (OSC 22)** — Change the mouse cursor shape within the terminal area. Support set, reset, push/pop stack.
- **FR9: Color reset (OSC 104/110/111/112)** — Reset palette entries, foreground, background, and cursor colors to defaults (user-configured theme values, not hardcoded defaults).
- **FR10: iTerm2 inline image (OSC 1337;File)** — Parse iTerm2 File protocol, decode base64 image data, display using existing fullscreen image viewer infrastructure.
- **FR11: User variables (OSC 1337;SetUserVar)** — Store key-value pairs (base64-decoded values) per terminal session. Accessible for future shell integration / status bar features.

### Non-Functional Requirements

- **NFR1 - Performance:** Color set/reset operations must not introduce measurable latency in the PTY data processing hot path. Clipboard operations must be asynchronous and never block the render loop.
- **NFR2 - Security:** OSC 52 read operations must be configurable (settings toggle). OSC 52 write data size should have an upper bound (configurable, default 10MB). OSC 1337;File inherits existing image processing security policies (size limits, format validation).
- **NFR3 - Compatibility:** All OSC sequences must work through tmux DCS passthrough. Color specifications must support `rgb:rr/gg/bb` and `#RRGGBB` formats. Both BEL (0x07) and ST (ESC \) terminators must work (already handled by parser).
- **NFR4 - Platform:** All features must work on both Linux and Windows.

## Implementation Approach

### Architecture

The existing 3-layer architecture is preserved:

```
PTY (Rust) → Binary Channel → WASM Parser → Callbacks → TypeScript Handlers
                                                              ↓
                                                    Canvas Renderer / Tauri IPC
```

**New data flows:**
- Color query response: TS handler → Tauri `pty_write` command → PTY
- Clipboard write: TS handler → Tauri clipboard API → System clipboard
- Clipboard read: TS handler → Tauri clipboard API → base64 encode → `pty_write`
- Notification: TS handler → Tauri notification plugin → OS notification
- Image (1337;File): TS handler → existing image viewer infrastructure

### OSC Specifications

#### OSC 4 — Color Palette Set/Query

**Format:**
```
ESC ] 4 ; c ; spec ST    — Set color c (0-255) to spec
ESC ] 4 ; c ; ? ST       — Query color c
```

Multiple pairs can be chained: `ESC ] 4 ; c1 ; spec1 ; c2 ; spec2 ST`

**Color spec formats:**
- `rgb:rr/gg/bb` (hexadecimal, 1-4 digits per channel, scaled to 8-bit)
- `#RRGGBB` (CSS hex, 6 digits)
- `#RGB` (CSS hex, 3 digits, each digit doubled)

**Query response:**
```
ESC ] 4 ; c ; rgb:rrrr/gggg/bbbb ST
```
Response uses 16-bit channel values (4 hex digits each, e.g., `rgb:ffff/0000/0000` for red).

**Storage:** Runtime palette overlay array (256 entries, nullable). When set, overrides the theme palette. Reset (OSC 104) clears the overlay.

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 10 — Default Foreground Color Set/Query

**Format:**
```
ESC ] 10 ; spec ST    — Set foreground color
ESC ] 10 ; ? ST       — Query foreground color
```

Supports chaining: `ESC ] 10 ; spec1 ; spec2 ; spec3 ST` sets foreground (10), background (11), cursor color (12) in sequence. Fewer params are valid (e.g., 2 params sets fg and bg only).

**Query response:**
```
ESC ] 10 ; rgb:rrrr/gggg/bbbb ST
```

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 11 — Default Background Color Set/Query

**Format:**
```
ESC ] 11 ; spec ST    — Set background color
ESC ] 11 ; ? ST       — Query background color
```

**Query response:**
```
ESC ] 11 ; rgb:rrrr/gggg/bbbb ST
```

This is the most important query for dark/light mode detection. Applications compare the luminance of the returned color to determine if the terminal is light or dark.

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 12 — Cursor Color Set/Query

**Format:**
```
ESC ] 12 ; spec ST    — Set cursor color
ESC ] 12 ; ? ST       — Query cursor color
```

**Query response:**
```
ESC ] 12 ; rgb:rrrr/gggg/bbbb ST
```

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 52 — Clipboard Operations

**Format:**
```
ESC ] 52 ; Pc ; Pd ST
```

- `Pc`: Clipboard selection character(s)
  - `c` = system clipboard
  - `p` = primary selection (X11, Linux only). On Windows, `p` is silently treated as `c` (system clipboard fallback)
  - `s` = select (alias for primary, same Windows fallback as `p`)
  - Multiple can be combined: `cp`
- `Pd`: Payload
  - `?` = query (read clipboard)
  - base64-encoded UTF-8 text = write to clipboard
  - empty string = clear clipboard

**Query response:**
```
ESC ] 52 ; Pc ; base64-data ST
```

**Security:**
- Write: Always allowed (up to configurable size limit, default 10MB)
- Read: Controlled by `clipboard_read_osc52` setting (default: enabled)
- When read is disabled, `?` queries are silently ignored (no response)

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 8 — Hyperlinks

**Format:**
```
ESC ] 8 ; params ; URI ST    — Start hyperlink
ESC ] 8 ; ; ST               — End hyperlink
```

- `params`: Semicolon-separated key=value pairs (e.g., `id=foo`)
  - `id`: Groups cells that belong to the same link (for multi-line wrapping)
- `URI`: Any valid URI

**Per-cell storage design:**

Add a `hyperlink_id` field to the Cell structure. The ID references an entry in a hyperlink table. Both the ID allocation and the hyperlink table are owned by WASM (`terminal_core.rs`) to ensure that hyperlink_id is available synchronously during `process_pty_data` (OSC callbacks are queued and processed after WASM returns, so TS-side allocation cannot be used for cells printed in the same PTY chunk).

WASM (`terminal_core.rs`):
- Add `hyperlink_id: u16` field to Cell (0 = no hyperlink, 1-65535 = index into hyperlink table)
- Cell size increases by 2 bytes (32 -> 34 bytes)
- Maintain hyperlink table: `Vec<Option<HyperlinkEntry>>` where `HyperlinkEntry = { params: String, uri: String }`
- ID allocation: monotonically increasing counter per WASM core (primary and alternate each have their own table and counter)
- `osc_handler.rs` processes OSC 8 inline: allocates ID, registers entry, sets active hyperlink_id. No callback to TS for state mutation
- OSC 8 callback to TS is still fired for metadata mirroring (hover tooltip, click handler URI lookup)
- Table reset on RIS / full clear. u16 exhaustion is not expected in normal operation

TypeScript:
- `CellAttributes` gets `hyperlinkId: number`
- TS mirrors the hyperlink table from WASM for rendering purposes (read-only): `hyperlinkTable: Map<number, {params: string, uri: string}>`
- `pack_row_abs()` の binary format に `hyperlink_id` (2 bytes, LE) を追加

**Rendering:**
- Hover detection checks cell hyperlink_id in addition to URL auto-detect
- OSC 8 hyperlinks take priority over auto-detected URLs
- Underline style and click behavior matches existing URL detection

**Reference:** https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda

#### OSC 9 — Desktop Notification / Progress Bar

**Format (notification):**
```
ESC ] 9 ; message ST
```

**Format (progress bar):**
```
ESC ] 9 ; 4 ; state ; progress ST
```

Progress states:
- `0` = remove/reset progress
- `1` = normal (green)
- `2` = error (red)
- `3` = indeterminate (pulsing)
- `4` = warning (yellow)

`progress`: 0-100 (percentage)

**Implementation:**
- Notification: Use Tauri `plugin-notification` for OS-native notifications
- Progress: Display in tab title or window title bar (e.g., `[42%] eMterm`)
- Progress state changes fire an event to the tab bar for visual indicator

**Reference:** https://conemu.github.io/en/AnsiEscapeCodes.html#ConEmu_specific_OSC

#### OSC 22 — Mouse Cursor Shape

**Format:**
```
ESC ] 22 ; cursor-name ST     — Set cursor shape
ESC ] 22 ; ST                 — Reset to default
ESC ] 22 ; >cursor-name ST    — Push cursor to stack and set
ESC ] 22 ; < ST               — Pop cursor from stack
```

**Supported cursor names** (CSS cursor values):
`default`, `none`, `pointer`, `text`, `wait`, `crosshair`, `move`, `not-allowed`, `grab`, `grabbing`, `progress`, `help`, `cell`, `vertical-text`, `copy`, `no-drop`, `all-scroll`, `col-resize`, `row-resize`, `n-resize`, `s-resize`, `e-resize`, `w-resize`, `ne-resize`, `nw-resize`, `se-resize`, `sw-resize`, `ew-resize`, `ns-resize`

**Implementation:**
- Map cursor name to CSS cursor property on the terminal root element
- Maintain a stack (max depth 10) for push/pop operations
- Unknown cursor names are silently ignored

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 104 — Color Palette Reset

**Format:**
```
ESC ] 104 ; c ST    — Reset color c to default
ESC ] 104 ST        — Reset all palette colors to default
```

Multiple colors can be specified: `ESC ] 104 ; c1 ; c2 ST`

Resets to the theme-configured palette values (not hardcoded defaults).

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 110 — Foreground Color Reset

**Format:**
```
ESC ] 110 ST
```

Resets to theme-configured foreground color.

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 111 — Background Color Reset

**Format:**
```
ESC ] 111 ST
```

Resets to theme-configured background color.

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 112 — Cursor Color Reset

**Format:**
```
ESC ] 112 ST
```

Resets to theme-configured cursor color.

**Reference:** https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h3-Operating-System-Commands

#### OSC 1337;File — iTerm2 Inline Image

**Format:**
```
ESC ] 1337 ; File=[args] : base64-data ST
```

**Args** (semicolon-separated key=value):
- `name=base64-filename` — File name (base64 encoded)
- `size=N` — File size in bytes (optional, hint for progress)
- `width=N[px|%]` — Display width (optional, `auto` if omitted)
- `height=N[px|%]` — Display height (optional, `auto` if omitted)
- `preserveAspectRatio=0|1` — Default 1
- `inline=0|1` — 1 = display inline, 0 = download. Default 0

**Implementation:**
- Parse args and base64 data
- Decode image data
- Display using existing fullscreen image viewer (same infrastructure as Kitty Graphics)
- For `inline=0`, trigger download flow (existing download infrastructure)
- Ignore unsupported args silently

**Reference:** https://iterm2.com/documentation-escape-codes.html

#### OSC 1337;SetUserVar — User Variables

**Format:**
```
ESC ] 1337 ; SetUserVar=key=base64-value ST
```

- `key`: Variable name (plain text)
- `base64-value`: Variable value (base64 encoded)

**Implementation:**
- Store in `Map<string, string>` on TerminalState (values decoded from base64)
- Overwrite existing key if present
- No size limit per variable (bounded by OSC buffer limit 16MB)
- Expose via a future API for shell integration / status bar

**Reference:** https://iterm2.com/documentation-escape-codes.html

### Color Specification Parsing

All color-related OSCs (4, 10, 11, 12) share a common color spec parser:

**Supported formats:**
1. `rgb:r/g/b` — 1-4 hex digits per channel, scaled to 8-bit
   - `rgb:f/f/f` → `(0xFF, 0xFF, 0xFF)`
   - `rgb:ff/ff/ff` → `(0xFF, 0xFF, 0xFF)`
   - `rgb:ffff/ffff/ffff` → `(0xFF, 0xFF, 0xFF)`
2. `#RGB` — 3 hex digits, each doubled: `#F0A` → `(0xFF, 0x00, 0xAA)`
3. `#RRGGBB` — 6 hex digits: `#FF00AA` → `(0xFF, 0x00, 0xAA)`
4. `#RRRRGGGGBBBB` — 12 hex digits (16-bit), truncated to 8-bit

**Query response format:**
All queries respond with `rgb:rrrr/gggg/bbbb` (16-bit per channel, lowercase hex), matching xterm behavior. The 8-bit internal value `0xFF` is reported as `ffff`.

### PTY Write-back Path

Color queries and clipboard reads require writing response sequences back to the PTY. This uses the existing `pty_write` Tauri command:

```typescript
// TypeScript handler
async function respondToQuery(sessionId: string, response: string) {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("pty_write", { sessionId, data: response });
}
```

The response must be sent as raw bytes (not user-visible text).

**Size constraint:** The existing `pty_write` command has a 1MB payload limit. Color query responses are small (< 100 bytes) and unaffected. OSC 52 read responses can be large if clipboard contents are large; the response must be truncated or chunked if the base64-encoded payload exceeds 1MB. In practice, the `clipboard_max_size_osc52` setting (default 10MB) governs the maximum clipboard data size; the OSC 52 read response is capped at the same limit, and the base64-encoded response (~1.37x original size) is sent via a single `pty_write` call. If the encoded response exceeds 1MB, it must be split into multiple `pty_write` calls.

### Settings

New settings fields:

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `clipboard_read_osc52` | bool | true | Allow OSC 52 clipboard read queries |
| `clipboard_max_size_osc52` | number | 10485760 | Max clipboard write size in bytes (base64 decoded) |

### Dependencies

**Internal Dependencies:**
- WASM parser (already handles OSC dispatch)
- Canvas renderer (for hyperlink underline drawing)
- PTY write path (for query responses)
- Image viewer (for OSC 1337;File)
- Settings service (for OSC 52 configuration)

**External Dependencies:**
- Tauri `plugin-notification` (for OSC 9 notifications, already included)
- Tauri clipboard API (for OSC 52)

### File Structure

Changes span the following files:

```
wasm/src/
├── osc_handler.rs          # Add new OSC number routing
├── cell.rs                 # Add hyperlink_id field
├── ring_buffer.rs          # pack_row_abs() packed format extension (hyperlink_id)
├── color_spec.rs           # (NEW) Color specification parser
├── parser.rs               # No changes needed (OSC parsing is generic)

src/terminal/
├── osc-colors.ts             # (NEW) Runtime palette overlay, color set/query/reset handlers
├── osc-clipboard.ts          # (NEW) OSC 52 clipboard handler
├── osc-notification.ts       # (NEW) OSC 9 notification and progress bar handler
├── osc-cursor-shape.ts       # (NEW) OSC 22 mouse cursor shape handler
├── osc-iterm2.ts             # (NEW) OSC 1337;File and SetUserVar handlers
├── state.ts                  # Add hyperlink table, user vars, palette overlay
├── canvas-renderer.ts        # Integrate OSC 8 hyperlink rendering
├── colors.ts                 # Add runtime palette overlay support
├── attributes.ts             # Add hyperlinkId to CellAttributes

src/terminal-app/
├── index.ts                  # Register new OSC callbacks (handleOscCallback dispatch)
├── handlers/link.ts          # Integrate OSC 8 click handling

src/settings/
├── types.ts                  # Add OSC 52 settings
```

## Test Scenarios

### Unit Tests
- [ ] Color spec parsing: `rgb:rr/gg/bb`, `#RRGGBB`, `#RGB`, `#RRRRGGGGBBBB`
- [ ] Color query response formatting (16-bit hex)
- [ ] OSC 4 set/query for individual palette entries
- [ ] OSC 10/11/12 set/query
- [ ] OSC 52 base64 decode and clipboard target parsing
- [ ] OSC 8 hyperlink data parsing (params;uri split)
- [ ] OSC 9 notification message parsing
- [ ] OSC 9;4 progress bar state/percentage parsing
- [ ] OSC 22 cursor name validation
- [ ] OSC 22 push/pop stack operations
- [ ] OSC 104 single and bulk palette reset
- [ ] OSC 110/111/112 individual color reset
- [ ] OSC 1337;File args parsing (name, size, width, height, inline)
- [ ] OSC 1337;SetUserVar key/value parsing and base64 decode

### Integration Tests
- [ ] OSC 4 set → query returns set value
- [ ] OSC 10 set → OSC 110 reset → query returns theme default
- [ ] OSC 52 write → clipboard contains expected text
- [ ] OSC 52 read → response contains clipboard contents
- [ ] OSC 8 start → print chars → OSC 8 end → cells have hyperlink_id
- [ ] OSC 22 set → cursor shape changes → reset → cursor returns to default

### E2E Tests
**Existing E2E tests**: `e2e-tests/` directory with `./scripts/run-e2e-docker.sh`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression
- [ ] OSC 8 hyperlink: visible underline on hover, click opens URL
- [ ] OSC 52 write: clipboard content verifiable
- [ ] OSC 11 query: response received by test application

### Edge Cases
- [ ] OSC 4 with invalid color number (>255) — silently ignored
- [ ] OSC 4 with chained multiple set/query pairs
- [ ] OSC 10/11 chaining (single sequence sets both)
- [ ] OSC 52 with empty base64 data — clears clipboard
- [ ] OSC 52 with oversized data — rejected per max size setting
- [ ] OSC 52 read when disabled in settings — no response
- [ ] OSC 8 with empty URI — clears hyperlink
- [ ] OSC 8 spanning multiple lines (soft-wrapped)
- [ ] OSC 22 pop on empty stack — no-op
- [ ] OSC 22 with unknown cursor name — silently ignored
- [ ] OSC 104 with no argument — resets all 256 colors
- [ ] OSC 1337;File with inline=0 — triggers download, not display
- [ ] OSC 1337;File with invalid base64 — error logged, no crash
- [ ] All OSC sequences through tmux DCS passthrough

## Security Considerations

- **Clipboard Read (OSC 52):** Controlled by setting. When enabled, any program writing to the PTY can read clipboard contents. Users must be aware of this risk. Default is enabled for maximum compatibility.
- **Clipboard Write (OSC 52):** Size-limited to prevent memory exhaustion from malicious sequences. The limit is configurable.
- **Image Display (OSC 1337;File):** Inherits existing image security: format validation, size limits, memory bounds from the image LRU cache (320MB quota).
- **Color Queries:** Response is written to PTY. No sensitive data is exposed (only terminal color configuration).
- **Input Validation:** All OSC data is validated before processing. Malformed data is silently dropped.

### Error Handling Contract

All OSC handlers follow a consistent error handling policy:

| Error Case | Behavior |
|-----------|----------|
| Malformed OSC data (invalid format, unparseable params) | Silently ignored (no crash, no response, no log at default level). Debug-level log only |
| Invalid color number (>255 for OSC 4) | Silently ignored |
| Unknown clipboard target (not `c`/`p`/`s`) | Silently ignored |
| Invalid base64 in OSC 52 / OSC 1337 | Silently ignored, debug-level log |
| OSC 52 oversized write | Silently rejected (no clipboard write, no response) |
| OSC 52 read when disabled | Silently ignored (no response) |
| `pty_write` failure (query response) | Error logged, operation aborted. No retry. Application does not crash |
| Notification permission denied (OSC 9) | Error logged, notification not shown |
| Image decode failure (OSC 1337;File) | Error logged, no crash. No placeholder displayed |
| Unknown OSC 22 cursor name | Silently ignored |
| Unknown OSC 1337 subcommand | Silently ignored |
| Session not found during PTY write-back | Error logged, response discarded |

## Implementation Phases

### Phase 1: Color Infrastructure
**Goals:** Complete color set/query/reset foundation
**Deliverables:**
- Color spec parser (shared utility)
- PTY write-back mechanism for query responses
- OSC 4 full implementation (set + query)
- OSC 10/11/12 full implementation (set + query)
- OSC 104/110/111/112 reset implementation
- Runtime palette overlay in color system

### Phase 2: Clipboard (OSC 52)
**Goals:** SSH-transparent clipboard operations
**Deliverables:**
- OSC 52 write (base64 decode → clipboard)
- OSC 52 read (clipboard → base64 encode → PTY response)
- Settings for read permission and size limits
- WASM routing for OSC 52

### Phase 3: Hyperlinks (OSC 8)
**Goals:** Per-cell hyperlink storage and rendering
**Deliverables:**
- Cell structure extension (hyperlink_id)
- Hyperlink table management
- Renderer integration (underline drawing, hover detection)
- Click handler integration (priority over auto-detect)

### Phase 4: Notifications & UI (OSC 9, OSC 22)
**Goals:** Desktop notifications and mouse cursor control
**Deliverables:**
- OSC 9 notification via Tauri plugin
- OSC 9;4 progress bar (tab/title display)
- OSC 22 cursor shape mapping
- OSC 22 push/pop stack

### Phase 5: iTerm2 Protocol (OSC 1337)
**Goals:** iTerm2 ecosystem compatibility
**Deliverables:**
- OSC 1337;File parsing and image display
- OSC 1337;SetUserVar storage
- WASM routing for OSC 1337

## Open Questions

> **Note**: Unresolved requirements are tracked as `status: tbd` in sdd.yaml.

- [ ] OSC 99 (kitty extended notification): Deferred to future work

## References

- XTerm Control Sequences: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
- OSC 8 Hyperlinks Specification: https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda
- iTerm2 Proprietary Escape Codes: https://iterm2.com/documentation-escape-codes.html
- kitty Terminal Protocol Extensions: https://sw.kovidgoyal.net/kitty/protocol-extensions/
- WezTerm Escape Sequences: https://wezfurlong.org/wezterm/escape-sequences.html
- Alacritty Escape Support: https://github.com/alacritty/alacritty/blob/master/docs/escape_support.md
- FinalTerm Semantic Prompts: https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md
- foot Terminal Control Sequences: https://codeberg.org/dnkl/foot/wiki/Control-Sequences
- ConEmu ANSI Escape Codes: https://conemu.github.io/en/AnsiEscapeCodes.html#ConEmu_specific_OSC
