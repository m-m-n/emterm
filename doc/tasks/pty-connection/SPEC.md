# PTY Connection Technical Specification

## 1. Overview

This specification defines the implementation details for the PTY (Pseudo Terminal) connection functionality in eMterm. This feature enables bidirectional communication between the terminal emulator frontend and shell processes through a PTY interface.

### 1.1 Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        eMterm Application                        │
├─────────────────────────────┬───────────────────────────────────┤
│      Frontend (WebView)     │         Backend (Rust/Tauri)      │
│                             │                                   │
│  ┌─────────────────────┐    │    ┌─────────────────────────┐   │
│  │   Terminal UI       │    │    │    PTY Manager          │   │
│  │   - Key capture     │    │    │    - Session registry   │   │
│  │   - Display output  │    │    │    - Lifecycle mgmt     │   │
│  └─────────┬───────────┘    │    └───────────┬─────────────┘   │
│            │                │                │                  │
│            │  IPC (Tauri)   │                │                  │
│            │◄──────────────►│                │                  │
│            │                │                │                  │
│  Events:   │                │    ┌───────────▼─────────────┐   │
│  - pty_output               │    │    PTY Session          │   │
│  - pty_exit                 │    │    - Reader thread      │   │
│  - pty_error                │    │    - Writer handle      │   │
│                             │    └───────────┬─────────────┘   │
│  Commands:                  │                │                  │
│  - pty_spawn                │                │                  │
│  - pty_write                │    ┌───────────▼─────────────┐   │
│  - pty_resize               │    │    portable-pty         │   │
│  - pty_kill                 │    │    (Platform PTY)       │   │
│                             │    └───────────┬─────────────┘   │
└─────────────────────────────┴────────────────┼──────────────────┘
                                               │
                                    ┌──────────▼──────────┐
                                    │    Shell Process    │
                                    │    (bash/zsh/pwsh)  │
                                    └─────────────────────┘
```

---

## 2. Backend Implementation (Rust)

### 2.1 Dependencies

Add to `src-tauri/Cargo.toml`:

```toml
[dependencies]
portable-pty = "0.8"
tokio = { version = "1", features = ["sync", "rt-multi-thread", "macros"] }
uuid = { version = "1", features = ["v4"] }
```

### 2.2 Module Structure

```
src-tauri/src/
├── lib.rs              # Tauri app entry, command registration
├── main.rs             # Binary entry point
└── pty/
    ├── mod.rs          # Module exports
    ├── manager.rs      # PTY session manager
    ├── session.rs      # Individual PTY session
    └── shell.rs        # Shell detection utilities
```

### 2.3 Core Types

#### 2.3.1 Session Identifier

```rust
// src-tauri/src/pty/mod.rs
use uuid::Uuid;

pub type SessionId = String;

pub fn generate_session_id() -> SessionId {
    Uuid::new_v4().to_string()
}
```

#### 2.3.2 PTY Session

```rust
// src-tauri/src/pty/session.rs
use portable_pty::{CommandBuilder, PtySize, PtyPair, native_pty_system};
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct PtySession {
    pub id: SessionId,
    pair: PtyPair,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl PtySession {
    pub fn new(
        id: SessionId,
        shell: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();

        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new(shell);
        // Set login shell for Unix
        #[cfg(unix)]
        cmd.arg("-l");

        let child = pair.slave.spawn_command(cmd)?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));

        Ok(Self { id, pair, child, writer })
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        self.pair.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub async fn write(&self, data: &[u8]) -> Result<(), PtyError> {
        let mut writer = self.writer.lock().await;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    pub fn take_reader(&self) -> Result<Box<dyn Read + Send>, PtyError> {
        self.pair.master.try_clone_reader()
    }

    pub fn try_wait(&mut self) -> Result<Option<portable_pty::ExitStatus>, PtyError> {
        self.child.try_wait()
    }

    pub fn kill(&mut self) -> Result<(), PtyError> {
        self.child.kill()
    }
}
```

#### 2.3.3 PTY Manager

```rust
// src-tauri/src/pty/manager.rs
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct PtyManager {
    sessions: Arc<RwLock<HashMap<SessionId, Arc<Mutex<PtySession>>>>>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_session(
        &self,
        shell: Option<String>,
        cols: u16,
        rows: u16,
    ) -> Result<SessionId, PtyError> {
        let shell = shell.unwrap_or_else(|| detect_default_shell());
        let id = generate_session_id();
        let session = PtySession::new(id.clone(), &shell, cols, rows)?;

        let mut sessions = self.sessions.write().await;
        sessions.insert(id.clone(), Arc::new(Mutex::new(session)));

        Ok(id)
    }

    pub async fn get_session(&self, id: &str) -> Option<Arc<Mutex<PtySession>>> {
        let sessions = self.sessions.read().await;
        sessions.get(id).cloned()
    }

    pub async fn remove_session(&self, id: &str) -> Option<Arc<Mutex<PtySession>>> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id)
    }
}
```

### 2.4 Shell Detection

```rust
// src-tauri/src/pty/shell.rs

pub fn detect_default_shell() -> String {
    #[cfg(unix)]
    {
        std::env::var("SHELL").unwrap_or_else(|_| {
            #[cfg(target_os = "macos")]
            { "/bin/zsh".to_string() }
            #[cfg(not(target_os = "macos"))]
            { "/bin/sh".to_string() }
        })
    }

    #[cfg(windows)]
    {
        "powershell.exe".to_string()
    }
}
```

### 2.5 Tauri Commands

```rust
// src-tauri/src/lib.rs
use tauri::{AppHandle, Manager, State};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct SpawnResult {
    session_id: String,
}

#[derive(Serialize, Clone)]
pub struct PtyOutputPayload {
    session_id: String,
    data: Vec<u8>,
}

#[derive(Serialize, Clone)]
pub struct PtyExitPayload {
    session_id: String,
    code: i32,
}

#[derive(Serialize, Clone)]
pub struct PtyErrorPayload {
    session_id: String,
    message: String,
}

#[tauri::command]
async fn pty_spawn(
    app: AppHandle,
    state: State<'_, PtyManager>,
    shell: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<SpawnResult, String> {
    let cols = cols.unwrap_or(80);
    let rows = rows.unwrap_or(24);

    let session_id = state
        .create_session(shell, cols, rows)
        .await
        .map_err(|e| e.to_string())?;

    // Start output reader thread
    spawn_reader_thread(app, state.inner().clone(), session_id.clone());

    Ok(SpawnResult { session_id })
}

#[tauri::command]
async fn pty_write(
    state: State<'_, PtyManager>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), String> {
    let session = state
        .get_session(&session_id)
        .await
        .ok_or("Session not found")?;

    let session = session.lock().await;
    session.write(&data).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn pty_resize(
    state: State<'_, PtyManager>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = state
        .get_session(&session_id)
        .await
        .ok_or("Session not found")?;

    let session = session.lock().await;
    session.resize(cols, rows).map_err(|e| e.to_string())
}

#[tauri::command]
async fn pty_kill(
    state: State<'_, PtyManager>,
    session_id: String,
) -> Result<(), String> {
    if let Some(session) = state.remove_session(&session_id).await {
        let mut session = session.lock().await;
        session.kill().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn spawn_reader_thread(app: AppHandle, manager: PtyManager, session_id: String) {
    std::thread::spawn(move || {
        let session = futures::executor::block_on(manager.get_session(&session_id));
        let Some(session) = session else { return };

        let session_guard = futures::executor::block_on(session.lock());
        let Ok(mut reader) = session_guard.take_reader() else { return };
        drop(session_guard);

        let mut buf = [0u8; 4096];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let payload = PtyOutputPayload {
                        session_id: session_id.clone(),
                        data: buf[..n].to_vec(),
                    };
                    let _ = app.emit("pty_output", payload);
                }
                Err(e) => {
                    let payload = PtyErrorPayload {
                        session_id: session_id.clone(),
                        message: e.to_string(),
                    };
                    let _ = app.emit("pty_error", payload);
                    break;
                }
            }
        }

        // Check exit status
        if let Some(session) = futures::executor::block_on(manager.get_session(&session_id)) {
            let mut session = futures::executor::block_on(session.lock());
            if let Ok(Some(status)) = session.try_wait() {
                let code = status.exit_code() as i32;
                let payload = PtyExitPayload {
                    session_id: session_id.clone(),
                    code,
                };
                let _ = app.emit("pty_exit", payload);
            }
        }

        // Cleanup
        futures::executor::block_on(manager.remove_session(&session_id));
    });
}

pub fn run() {
    tauri::Builder::default()
        .manage(PtyManager::new())
        .invoke_handler(tauri::generate_handler![
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
        ])
        .setup(|app| {
            // ... existing setup
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### 2.6 Error Types

```rust
// src-tauri/src/pty/mod.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PtyError {
    #[error("PTY creation failed: {0}")]
    Creation(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("PTY error: {0}")]
    Pty(#[from] portable_pty::Error),
}
```

---

## 3. Frontend Implementation (TypeScript)

### 3.1 Type Definitions

```typescript
// src/types/pty.ts

export interface SpawnResult {
  session_id: string;
}

export interface PtyOutputPayload {
  session_id: string;
  data: number[];
}

export interface PtyExitPayload {
  session_id: string;
  code: number;
}

export interface PtyErrorPayload {
  session_id: string;
  message: string;
}

export interface PtySpawnOptions {
  shell?: string;
  cols?: number;
  rows?: number;
}
```

### 3.2 PTY Client Class

```typescript
// src/pty/client.ts
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import type {
  SpawnResult,
  PtyOutputPayload,
  PtyExitPayload,
  PtyErrorPayload,
  PtySpawnOptions,
} from '../types/pty';

export class PtyClient {
  private sessionId: string | null = null;
  private unlisteners: UnlistenFn[] = [];

  async spawn(options: PtySpawnOptions = {}): Promise<string> {
    const result = await invoke<SpawnResult>('pty_spawn', {
      shell: options.shell,
      cols: options.cols ?? 80,
      rows: options.rows ?? 24,
    });

    this.sessionId = result.session_id;
    return this.sessionId;
  }

  async write(data: Uint8Array | string): Promise<void> {
    if (!this.sessionId) {
      throw new Error('PTY session not started');
    }

    const bytes = typeof data === 'string'
      ? new TextEncoder().encode(data)
      : data;

    await invoke('pty_write', {
      sessionId: this.sessionId,
      data: Array.from(bytes),
    });
  }

  async resize(cols: number, rows: number): Promise<void> {
    if (!this.sessionId) {
      throw new Error('PTY session not started');
    }

    await invoke('pty_resize', {
      sessionId: this.sessionId,
      cols,
      rows,
    });
  }

  async kill(): Promise<void> {
    if (!this.sessionId) {
      return;
    }

    await invoke('pty_kill', {
      sessionId: this.sessionId,
    });

    this.sessionId = null;
  }

  async onOutput(callback: (data: Uint8Array) => void): Promise<void> {
    const unlisten = await listen<PtyOutputPayload>('pty_output', (event) => {
      if (event.payload.session_id === this.sessionId) {
        callback(new Uint8Array(event.payload.data));
      }
    });
    this.unlisteners.push(unlisten);
  }

  async onExit(callback: (code: number) => void): Promise<void> {
    const unlisten = await listen<PtyExitPayload>('pty_exit', (event) => {
      if (event.payload.session_id === this.sessionId) {
        callback(event.payload.code);
      }
    });
    this.unlisteners.push(unlisten);
  }

  async onError(callback: (message: string) => void): Promise<void> {
    const unlisten = await listen<PtyErrorPayload>('pty_error', (event) => {
      if (event.payload.session_id === this.sessionId) {
        callback(event.payload.message);
      }
    });
    this.unlisteners.push(unlisten);
  }

  dispose(): void {
    for (const unlisten of this.unlisteners) {
      unlisten();
    }
    this.unlisteners = [];
  }
}
```

### 3.3 Key Input Handler

```typescript
// src/pty/keyboard.ts

export interface KeyMapping {
  key: string;
  ctrl?: boolean;
  alt?: boolean;
  shift?: boolean;
  sequence: number[];
}

const SPECIAL_KEYS: KeyMapping[] = [
  // Control characters
  { key: 'c', ctrl: true, sequence: [0x03] },  // ETX (Ctrl+C)
  { key: 'd', ctrl: true, sequence: [0x04] },  // EOT (Ctrl+D)
  { key: 'z', ctrl: true, sequence: [0x1a] },  // SUB (Ctrl+Z)
  { key: 'l', ctrl: true, sequence: [0x0c] },  // FF (Ctrl+L)

  // Arrow keys
  { key: 'ArrowUp', sequence: [0x1b, 0x5b, 0x41] },    // ESC [ A
  { key: 'ArrowDown', sequence: [0x1b, 0x5b, 0x42] },  // ESC [ B
  { key: 'ArrowRight', sequence: [0x1b, 0x5b, 0x43] }, // ESC [ C
  { key: 'ArrowLeft', sequence: [0x1b, 0x5b, 0x44] },  // ESC [ D

  // Navigation keys
  { key: 'Home', sequence: [0x1b, 0x5b, 0x48] },       // ESC [ H
  { key: 'End', sequence: [0x1b, 0x5b, 0x46] },        // ESC [ F
  { key: 'PageUp', sequence: [0x1b, 0x5b, 0x35, 0x7e] },   // ESC [ 5 ~
  { key: 'PageDown', sequence: [0x1b, 0x5b, 0x36, 0x7e] }, // ESC [ 6 ~
  { key: 'Insert', sequence: [0x1b, 0x5b, 0x32, 0x7e] },   // ESC [ 2 ~
  { key: 'Delete', sequence: [0x1b, 0x5b, 0x33, 0x7e] },   // ESC [ 3 ~

  // Function keys
  { key: 'F1', sequence: [0x1b, 0x4f, 0x50] },   // ESC O P
  { key: 'F2', sequence: [0x1b, 0x4f, 0x51] },   // ESC O Q
  { key: 'F3', sequence: [0x1b, 0x4f, 0x52] },   // ESC O R
  { key: 'F4', sequence: [0x1b, 0x4f, 0x53] },   // ESC O S
  { key: 'F5', sequence: [0x1b, 0x5b, 0x31, 0x35, 0x7e] },  // ESC [ 15 ~
  { key: 'F6', sequence: [0x1b, 0x5b, 0x31, 0x37, 0x7e] },  // ESC [ 17 ~
  { key: 'F7', sequence: [0x1b, 0x5b, 0x31, 0x38, 0x7e] },  // ESC [ 18 ~
  { key: 'F8', sequence: [0x1b, 0x5b, 0x31, 0x39, 0x7e] },  // ESC [ 19 ~
  { key: 'F9', sequence: [0x1b, 0x5b, 0x32, 0x30, 0x7e] },  // ESC [ 20 ~
  { key: 'F10', sequence: [0x1b, 0x5b, 0x32, 0x31, 0x7e] }, // ESC [ 21 ~
  { key: 'F11', sequence: [0x1b, 0x5b, 0x32, 0x33, 0x7e] }, // ESC [ 23 ~
  { key: 'F12', sequence: [0x1b, 0x5b, 0x32, 0x34, 0x7e] }, // ESC [ 24 ~

  // Special
  { key: 'Enter', sequence: [0x0d] },      // CR
  { key: 'Tab', sequence: [0x09] },        // HT
  { key: 'Backspace', sequence: [0x7f] },  // DEL
  { key: 'Escape', sequence: [0x1b] },     // ESC
];

export function keyEventToBytes(event: KeyboardEvent): Uint8Array | null {
  // Check for special key mappings
  for (const mapping of SPECIAL_KEYS) {
    if (
      event.key === mapping.key &&
      !!event.ctrlKey === !!mapping.ctrl &&
      !!event.altKey === !!mapping.alt &&
      !!event.shiftKey === !!mapping.shift
    ) {
      return new Uint8Array(mapping.sequence);
    }
  }

  // Ctrl + letter (a-z)
  if (event.ctrlKey && event.key.length === 1) {
    const char = event.key.toLowerCase();
    if (char >= 'a' && char <= 'z') {
      return new Uint8Array([char.charCodeAt(0) - 96]);
    }
  }

  // Alt + key (send ESC prefix)
  if (event.altKey && event.key.length === 1) {
    const bytes = new TextEncoder().encode(event.key);
    const result = new Uint8Array(bytes.length + 1);
    result[0] = 0x1b; // ESC
    result.set(bytes, 1);
    return result;
  }

  // Regular printable character
  if (event.key.length === 1 && !event.ctrlKey && !event.altKey) {
    return new TextEncoder().encode(event.key);
  }

  return null;
}
```

### 3.4 Terminal Size Calculator

```typescript
// src/pty/size.ts

export interface TerminalSize {
  cols: number;
  rows: number;
}

export function calculateTerminalSize(
  container: HTMLElement,
  charWidth: number,
  charHeight: number,
): TerminalSize {
  const { clientWidth, clientHeight } = container;

  // Account for padding/margins
  const style = getComputedStyle(container);
  const paddingX = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
  const paddingY = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom);

  const availableWidth = clientWidth - paddingX;
  const availableHeight = clientHeight - paddingY;

  const cols = Math.max(1, Math.floor(availableWidth / charWidth));
  const rows = Math.max(1, Math.floor(availableHeight / charHeight));

  return { cols, rows };
}

export function measureCharacterSize(
  fontFamily: string,
  fontSize: number,
): { width: number; height: number } {
  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d')!;

  ctx.font = `${fontSize}px ${fontFamily}`;
  const metrics = ctx.measureText('M');

  return {
    width: metrics.width,
    height: fontSize * 1.2, // Approximate line height
  };
}
```

---

## 4. Tauri Configuration

### 4.1 Capabilities Update

Update `src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "enables the default permissions",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:event:default",
    "shell:allow-spawn",
    "shell:allow-execute"
  ]
}
```

### 4.2 Shell Plugin (if needed)

If using `tauri-plugin-shell` for additional shell operations:

```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-shell = "2"
```

---

## 5. Data Flow

### 5.1 Input Flow (User -> Shell)

```
1. User presses key
   ↓
2. DOM KeyboardEvent captured
   ↓
3. keyEventToBytes() converts to byte sequence
   ↓
4. PtyClient.write() sends via IPC
   ↓
5. pty_write command receives data
   ↓
6. PtySession.write() sends to PTY master
   ↓
7. Shell process receives input
```

### 5.2 Output Flow (Shell -> User)

```
1. Shell writes to stdout/stderr
   ↓
2. PTY master receives data
   ↓
3. Reader thread reads into buffer
   ↓
4. app.emit("pty_output") sends event
   ↓
5. Frontend listener receives PtyOutputPayload
   ↓
6. Callback processes byte array
   ↓
7. Terminal display updated (future: ANSI parser)
```

### 5.3 Resize Flow

```
1. Window resize event or manual trigger
   ↓
2. calculateTerminalSize() computes cols/rows
   ↓
3. PtyClient.resize() sends via IPC
   ↓
4. pty_resize command receives dimensions
   ↓
5. PtySession.resize() calls PTY resize
   ↓
6. SIGWINCH sent to shell (Unix) / ConPTY updated (Windows)
```

---

## 6. Error Handling

### 6.1 Error Categories

| Category | Source | Handling |
|----------|--------|----------|
| Spawn Error | Shell not found, permission denied | Display error, offer retry |
| IO Error | Read/write failures | Emit pty_error, cleanup session |
| Session Error | Invalid session ID | Return error to caller |
| Platform Error | PTY API failures | Log, emit error, cleanup |

### 6.2 Cleanup Strategy

1. On normal exit: Reader thread detects EOF, emits `pty_exit`, removes session
2. On error: Emit `pty_error`, kill child process, remove session
3. On app shutdown: Kill all sessions in `PtyManager.sessions`

---

## 7. Platform-Specific Notes

### 7.1 Linux/macOS

- Uses POSIX PTY via `openpty()`/`forkpty()`
- Shell runs as login shell (`-l` flag)
- SIGWINCH automatically sent on resize
- Environment variables inherited from parent

### 7.2 Windows

- Uses ConPTY (Windows 10 1809+)
- PowerShell only (no cmd.exe support)
- Different escape sequence handling may be needed
- UTF-8 code page should be set

---

## 8. Testing Strategy

### 8.1 Unit Tests (Rust)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_detection() {
        let shell = detect_default_shell();
        assert!(!shell.is_empty());
    }

    #[test]
    fn test_session_id_generation() {
        let id1 = generate_session_id();
        let id2 = generate_session_id();
        assert_ne!(id1, id2);
    }
}
```

### 8.2 Integration Tests

1. Spawn shell and verify output
2. Send input and verify echo
3. Test resize and verify `stty size` output
4. Test exit command and verify cleanup

### 8.3 Manual Testing Checklist

- [ ] Shell prompt appears
- [ ] Typing shows characters
- [ ] Ctrl+C interrupts running command
- [ ] Arrow keys navigate history
- [ ] Tab completion works
- [ ] Window resize updates terminal size
- [ ] `exit` closes session cleanly
