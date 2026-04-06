# Feature: Mux Status Bar

## Overview

Enable the mux daemon to periodically execute registered commands, resolve template strings, and push the results to the GUI's status bar OSC layer via the StatusUpdate (0x16) IPC message. This brings tmux-like status bar functionality to eMterm's mux mode.

## Objectives

- Add `mux.statusbar` settings section with left/right templates and command definitions
- Implement daemon-side command execution with periodic scheduling and timeout
- Replace the existing `StatusUpdateMsg` format with `{ left, right }` strings
- Display StatusUpdate content in the app status bar's OSC layer
- Auto-clear the OSC layer when exiting mux mode

## User Stories

### US1: Display system info in status bar
As a mux user, I want to see custom command output in the status bar, so that I can monitor system state without switching windows.

**Acceptance Criteria:**
- [ ] Registered commands execute at configured intervals
- [ ] Command stdout appears in the correct status bar position (left/right)
- [ ] Commands that exceed 5 seconds are killed and previous value is retained

### US2: Configure status bar content
As a mux user, I want to configure what appears in the status bar via settings.json, so that I can customize the display to my needs.

**Acceptance Criteria:**
- [ ] `mux.statusbar.left` and `mux.statusbar.right` accept template strings
- [ ] `{cmd:name}`, `{hostname}`, `{cwd}` variables are resolved
- [ ] `mux.statusbar.commands` registers executables with interval_ms

## Technical Requirements

### Functional Requirements

- **FR1: Settings structure** — Add `MuxStatusbarSettings` struct nested under `MuxSettings` with fields: `enabled` (bool, default `false`), `left` (String), `right` (String), `commands` (HashMap<String, MuxStatusbarCommand>). `MuxStatusbarCommand` has `executable` (String) and `interval_ms` (u64, default 5000). When `enabled` is `false`, daemon skips all command execution, timer setup, and StatusUpdate sending.
- **FR2: Template resolution** — Resolve `{cmd:name}` with command stdout (trimmed), `{hostname}` with `gethostname()`, `{cwd}` with active pane's cached working directory (from OSC 7). If no OSC 7 has been received yet, resolve to empty string.
- **FR3: Command execution** — Execute only registered executables (no arguments, no shell). Expand `~` to home directory. Working directory is the active pane's cwd (from OSC 7); falls back to the user's home directory if unavailable or non-existent. `interval_ms` is clamped to a minimum of 1000ms. Each command runs with single-flight control — if the previous execution is still running when the next tick fires, the tick is skipped. Timeout after 5 seconds (kill process, retain previous value). Suppress stderr. For multi-line stdout, use only the first line (trimmed).
- **FR4: StatusUpdate message** — Replace `StatusUpdateMsg { session_name, window_names, active_window_index }` with `StatusUpdateMsg { left: String, right: String }`. Update bincode serialization on both Rust and TypeScript sides.
- **FR5: Periodic StatusUpdate push** — Each registered command runs on its own independent timer (`interval_ms`). A separate render timer (fixed 1-second interval) resolves the full template using cached command outputs, resolves `{cwd}`, and sends StatusUpdate only if the resolved content differs from the previous send (differential). The render timer only runs when at least one template variable is used.
- **FR6: OSC layer display** — When `MuxClient` receives StatusUpdate, invoke `OscLayerController.handleCommand("set", "left", msg.left)` and `("set", "right", msg.right)`.
- **FR7: Auto-clear on exit** — When `exitMuxMode` is called, clear the OSC layer via `OscLayerController.handleCommand("clear")`.
- **FR8: Settings file reading** — Daemon reads `settings.json` using the same path logic as `tmux_import.rs` (`XDG_CONFIG_HOME` or `~/.config/net.laser5.app.emterm/settings.json`). Parse only `mux.statusbar` section. Read once at daemon startup (no hot-reload required). On any failure (file missing, permission denied, invalid JSON, schema mismatch), log a warning, fall back to `MuxStatusbarSettings::default()` (`enabled: false`), and send a StatusUpdate with the error message in the `left` field so the user sees the warning in the OSC layer. The daemon must never crash or panic due to settings file issues.
- **FR9: Active pane cwd** — OSC 7 detection runs in each pane's `pty_reader_loop` (pane level, not connection level), scanning raw PTY output bytes for `ESC ] 7 ; file://host/path ST` pattern. Detected cwd is cached in `MuxPane.cwd: Option<String>`. This runs unconditionally regardless of `statusbar.enabled` (lightweight byte pattern match). On mux window switch, `{cwd}` resolves to the active pane's cached value. Empty string if OSC 7 not yet received. Active pane is determined by tracking the last pane that received `PtyInput` or `SwitchWindow` — stored as `active_pane_id` per connection. Initial value is `None` (resolves to empty string until first input or window switch).
- **FR10: Tab switch handling** — When GUI switches to a non-mux tab, clear the OSC layer. When switching to a mux tab, send a `RequestStatusUpdate` (0x17) message to the daemon via the MuxClient. The daemon responds with a fresh `StatusUpdate` (0x16) containing the current resolved template values. This ensures the displayed data is always up-to-date rather than stale cached values.
- **FR11: RequestStatusUpdate message** — New message type `RequestStatusUpdate` (0x17), direction GUI→Daemon, empty payload (message header only, no body bytes). On receipt, daemon immediately resolves templates with current cached values and sends a `StatusUpdate` (0x16) response. On the TypeScript side, `MuxClient` sends this by encoding only the message type byte (0x17) with zero-length body, following the existing framed message protocol.

### Non-Functional Requirements

- **NFR1 - Security:** Only executables listed in `mux.statusbar.commands` may be executed. No shell invocation, no argument injection. Matches the app-side `statusbar_custom_commands` security model.
- **NFR2 - Performance:** Command execution is async and non-blocking. Timeout prevents daemon hangs. StatusUpdate messages are small (< 1KB typically).
- **NFR3 - Platform:** `{cwd}` is based on OSC 7 detection (cross-platform, depends on shell support). `{hostname}` works on all platforms.

## Implementation Approach

### Architecture

```
┌──────────────────────────────────────────────────────┐
│ GUI (TypeScript)                                     │
│  MuxClient.onStatusUpdate → OscLayerController       │
│      ↑ StatusUpdate (0x16) via APC/OSC               │
├──────────────────────────────────────────────────────┤
│ Bridge (Rust) — pass-through                         │
├──────────────────────────────────────────────────────┤
│ Daemon (Rust)                                        │
│  settings.json → MuxStatusbarSettings                │
│  CommandRunner: periodic exec → template resolve     │
│  → StatusUpdateMsg { left, right }                   │
│  → send to GUI client via framed socket              │
└──────────────────────────────────────────────────────┘
```

### Data Flow

```
Daemon startup
  → read settings.json (mux.statusbar section)
  → resolve {hostname} once (cached)
  → for each command: start independent timer (interval_ms)
  → start render timer (1-second fixed interval)

Command timer tick (per command):
  → spawn process with 5s timeout
  → read stdout, trim, cache result
  → on timeout/error: retain previous cached value

PTY output (per pane):
  → scan for OSC 7 sequence → update pane.cwd cache

Render timer tick (1 second):
  → read active pane's cached cwd
  → resolve template: replace {cmd:name} with cached values, {hostname}, {cwd}
  → compare with previous resolved strings
  → if changed: encode StatusUpdateMsg { left, right } → send to GUI

Tab switch (GUI):
  → non-mux tab becomes active → clear OSC layer
  → mux tab becomes active → send RequestStatusUpdate (0x17) to daemon
      → daemon resolves templates immediately → sends StatusUpdate (0x16)
      → GUI updates OSC layer

GUI receives StatusUpdate:
  → MuxClient.handleIncomingApc decodes StatusUpdateMsg
  → calls onStatusUpdate callback
  → callback writes to OscLayerController

exitMuxMode:
  → OscLayerController.handleCommand("clear")
```

### Settings Structure (Rust)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MuxStatusbarSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub left: String,
    #[serde(default)]
    pub right: String,
    #[serde(default)]
    pub commands: HashMap<String, MuxStatusbarCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxStatusbarCommand {
    pub executable: String,
    #[serde(default = "default_mux_statusbar_interval")]
    pub interval_ms: u64,
}

fn default_mux_statusbar_interval() -> u64 { 5000 }
```

### Settings Structure (TypeScript)

```typescript
export interface MuxStatusbarSettings {
  enabled: boolean;
  left: string;
  right: string;
  commands: Record<string, MuxStatusbarCommand>;
}

export interface MuxStatusbarCommand {
  executable: string;
  interval_ms: number;
}

export interface MuxSettings {
  // ... existing fields ...
  statusbar: MuxStatusbarSettings;
}
```

### StatusUpdateMsg (Rust)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusUpdateMsg {
    pub left: String,
    pub right: String,
}
```

### StatusUpdateMsg Decoder (TypeScript)

```typescript
function decodeStatusUpdateMsg(data: Uint8Array): { left: string; right: string } | null {
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  let offset = 0;

  // left: String (u64 LE len + UTF-8)
  if (offset + 8 > data.length) return null;
  const leftLen = Number(view.getBigUint64(offset, true));
  offset += 8;
  if (offset + leftLen > data.length) return null;
  const left = new TextDecoder().decode(data.slice(offset, offset + leftLen));
  offset += leftLen;

  // right: String (u64 LE len + UTF-8)
  if (offset + 8 > data.length) return null;
  const rightLen = Number(view.getBigUint64(offset, true));
  offset += 8;
  if (offset + rightLen > data.length) return null;
  const right = new TextDecoder().decode(data.slice(offset, offset + rightLen));

  return { left, right };
}
```

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/mux/ipc/protocol.rs`: StatusUpdateMsg definition
- `src-tauri/src/mux/ipc/connection.rs`: message loop with select!
- `src-tauri/src/mux/session/pane.rs`: pane cwd cache (from OSC 7)
- `src-tauri/src/mux/tmux_import.rs`: settings file path logic (reuse)
- `src-tauri/src/commands/config/settings.rs`: MuxSettings struct
- `src/status-bar/osc-controller.ts`: OSC layer rendering
- `src/terminal/mux/mux-client.ts`: StatusUpdate decoding

**External Dependencies:**
- `libc::gethostname` (unix) / `winapi::GetComputerNameW` (windows)
- `tokio::process::Command` for async command execution with timeout

### File Structure

```
src-tauri/src/
├── commands/config/settings.rs  # MuxStatusbarSettings, MuxStatusbarCommand
├── mux/
│   ├── ipc/
│   │   ├── protocol.rs          # StatusUpdateMsg { left, right }
│   │   ├── connection.rs         # StatusUpdate periodic timer in select!
│   │   └── statusbar.rs          # NEW: command runner, template engine
│   ├── daemon.rs                 # Settings loading at startup
│   └── session/pane.rs           # Pane cwd cache (from OSC 7)

src/
├── settings/types.ts             # MuxStatusbarSettings type
├── terminal/mux/mux-client.ts    # Updated decoder, onStatusUpdate wiring
├── terminal-app/mux/mux-session.ts  # Register onStatusUpdate callback
└── main.ts                       # OSC layer connection, exit cleanup
```

## Test Scenarios

### Unit Tests

- [ ] `MuxStatusbarSettings` deserializes correctly with defaults
- [ ] `MuxStatusbarSettings` deserializes correctly with full config
- [ ] Template resolution replaces `{cmd:name}`, `{hostname}`, `{cwd}` correctly
- [ ] Template resolution handles unknown variables (leaves as-is for easy debugging)
- [ ] `StatusUpdateMsg` encodes/decodes correctly (Rust bincode round-trip)
- [ ] Command timeout kills the process after 5 seconds
- [ ] `~` expansion works correctly in executable path
- [ ] Settings file path resolution matches tmux_import logic

### Integration Tests

- [ ] Daemon reads settings.json and starts status bar timer
- [ ] StatusUpdate message flows: daemon → bridge → GUI
- [ ] `decodeStatusUpdateMsg` (TypeScript) correctly decodes Rust bincode

### E2E Tests

**Existing E2E tests**: `e2e-tests/` with `scripts/run-e2e-docker.sh`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression

### Edge Cases

- [ ] `mux.statusbar.enabled` is `false` (default) — no timers started, no StatusUpdate sent
- [ ] Empty `mux.statusbar` (enabled but no left/right/commands) — no StatusUpdate sent
- [ ] Command executable does not exist — log error, resolve to empty string
- [ ] Command produces no output — resolve to empty string
- [ ] Command produces multi-line output — use first line only (trimmed)
- [ ] All commands time out — retain previous values
- [ ] `{cwd}` before any OSC 7 received — resolves to empty string
- [ ] Mux window switch — `{cwd}` updates to new active pane's cached value
- [ ] GUI tab switch to mux tab — sends RequestStatusUpdate, receives fresh StatusUpdate
- [ ] GUI tab switch to non-mux tab — clears OSC layer
- [ ] Settings file missing — log warning, use defaults (enabled: false), send error to OSC layer
- [ ] Settings file contains invalid JSON — log warning, use defaults (enabled: false)
- [ ] Settings file valid JSON but mux.statusbar has wrong types — log warning, use defaults (enabled: false)
- [ ] Multiple GUI clients connected — each receives StatusUpdate independently
- [ ] Daemon disconnects unexpectedly — existing `onDetached` callback triggers `exitMuxMode`, which clears the OSC layer

## Security Considerations

- **Command execution whitelist:** Only executables explicitly listed in `mux.statusbar.commands` are allowed. No shell invocation (`sh -c`), no argument passing.
- **Path validation:** Executable paths undergo `~` expansion but no further shell expansion. Path traversal in executable names is blocked by the file system (daemon runs as the user).
- **Input sanitization:** Command stdout is used as plain text. The OSC layer controller strips HTML tags before rendering (existing XSS protection via `stripHtmlTags`).
- **Timeout protection:** 5-second timeout prevents resource exhaustion from hung scripts.

## Error Handling

| Scenario | Handling |
|----------|----------|
| Settings file missing | Log warning, use defaults (enabled: false), send error to OSC layer |
| Settings file unreadable (permissions) | Log warning, use defaults, send error to OSC layer |
| Invalid JSON (syntax error) | Log warning, use defaults, send error to OSC layer (e.g. "settings.json: parse error at line 42") |
| Valid JSON but mux.statusbar schema mismatch | Log warning, use defaults, send error to OSC layer |
| Command not found | Log error, resolve variable to empty string |
| Command timeout (>5s) | Kill process, retain previous output |
| Command execution error | Log warning, resolve variable to empty string |
| OSC 7 not yet received for pane | Resolve `{cwd}` to empty string |
| StatusUpdate send failure | Log error, continue (connection may be closing) |

## Cleanup

- **Remove `src/terminal/mux/status-bar.ts`** — The `MuxStatusBar` class is unused and superseded by the OSC layer approach.
- **Remove `MuxStatusBar` imports** — Clean up any references in other files.

## Success Criteria

- [ ] All functional requirements (FR1-FR11) are implemented and tested
- [ ] All unit test scenarios pass
- [ ] Existing E2E tests pass without regression
- [ ] Security model matches app-side statusbar_custom_commands
- [ ] `MuxStatusBar` class is removed

## Open Questions

> **Note**: No unresolved requirements.

## References

- App-side status bar: `src/status-bar/` (renderer, osc-controller, providers)
- App-side custom commands: `src-tauri/src/commands/statusbar.rs`
- Mux IPC protocol: `src-tauri/src/mux/ipc/protocol.rs`
- Settings path logic: `src-tauri/src/mux/tmux_import.rs`
