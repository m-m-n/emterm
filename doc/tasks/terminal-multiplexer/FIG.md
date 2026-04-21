# Terminal Multiplexer Architecture Diagrams

> **NOTE (post-cleanup)**: 本ドキュメント内の Pane Layout / Copy Mode / SplitPane (0x11) / split-vertical / split-horizontal / zoom-toggle / copy-mode 等の記述は、`doc/tasks/mux-feature-cleanup/` で削除された機能に関する履歴情報。現行の有効機能のアーキテクチャは `doc/tasks/terminal-multiplexer/SPEC.md` を参照。

## 1. Component Architecture

```mermaid
flowchart TB
    subgraph GUI["eMterm GUI (Tauri)"]
        TermApp["TerminalApp"]
        MuxClient["MuxClient"]
        PrefixKey["PrefixKeyHandler"]
        Layout["Layout Engine"]
        PaneCanvases["Per-Pane Canvas + WASM"]
        CopyMode["CopyModeManager"]
        TabGroup["MuxTabGroup"]
    end

    subgraph Bridge["Tauri Bridge (bridge.rs)"]
        mux_connect["mux_connect"]
        mux_handshake["mux_handshake"]
        mux_send_input["mux_send_input"]
        mux_send_control["mux_send_control"]
        mux_output_stream["mux_start_output_stream"]
    end

    subgraph Daemon["Mux Daemon (daemon.rs)"]
        Listener["UnixListener"]
        ConnHandler["handle_connection"]
        RouteMsg["route_message"]
        SessionMgr["SessionManager"]
    end

    subgraph Sessions["Session Hierarchy"]
        Session["MuxSession"]
        Window["MuxWindow"]
        Pane["MuxPane"]
        PTY["PTY Process"]
        ReaderThread["pty_reader_loop"]
    end

    TermApp --> MuxClient
    TermApp --> PrefixKey
    TermApp --> Layout
    TermApp --> PaneCanvases
    TermApp --> CopyMode
    TermApp --> TabGroup

    MuxClient --> mux_connect
    MuxClient --> mux_handshake
    MuxClient --> mux_send_input
    MuxClient --> mux_send_control
    MuxClient -.->|events| mux_output_stream

    mux_connect -->|Unix socket| Listener
    Listener --> ConnHandler
    ConnHandler --> RouteMsg
    RouteMsg --> SessionMgr

    SessionMgr --> Session
    Session --> Window
    Window --> Pane
    Pane --> PTY
    PTY --> ReaderThread
```

## 2. PTY Data Flow — Normal Mode vs Mux Mode

### 2a. Normal Mode (direct PTY)

Shell → Rust reader thread → Tauri Channel (binary) → WASM → Canvas.
VT100 パースは WASM で **1回だけ**。中間レイヤーなし。

```mermaid
sequenceDiagram
    participant Shell as Shell Process
    participant RustPTY as Rust PTY reader<br>(std#58;#58;thread)
    participant TauriCh as Tauri Channel<br>(binary ArrayBuffer)
    participant PtyClient as PtyClient (TS)
    participant WASM as WasmGrid<br>process_pty_data()
    participant Canvas as Canvas Renderer

    Shell->>RustPTY: PTY read (raw bytes)
    RustPTY->>TauriCh: channel.send(raw bytes)
    TauriCh->>PtyClient: onmessage(ArrayBuffer)
    PtyClient->>WASM: process_pty_data(data)
    Note over WASM: VT100 parse (1回だけ)
    WASM->>Canvas: dirty rows → render
```

### 2b. tmux 経由 (問題のあるパターン)

Shell → tmux 仮想端末 → **VT100 再パース + 再生成** → ホスト PTY → eMterm。
tmux 内部で VT100 を解釈し再構築するため、高スループット時にちらつきが発生する。

```mermaid
sequenceDiagram
    participant Shell as Shell Process
    participant tmuxPTY as tmux 内部 PTY
    participant tmuxVT as tmux VT100 Engine<br>(parse + re-generate)
    participant HostPTY as Host PTY
    participant RustPTY as Rust PTY reader
    participant WASM as WasmGrid<br>process_pty_data()
    participant Canvas as Canvas Renderer

    Shell->>tmuxPTY: PTY output (raw bytes)
    tmuxPTY->>tmuxVT: VT100 parse (1回目)
    Note over tmuxVT: 仮想端末グリッド更新<br>差分検出<br>VT100 シーケンス再生成
    tmuxVT->>HostPTY: 再生成された VT100 bytes
    Note over tmuxVT,HostPTY: ボトルネック#58;<br>再生成が追いつかず<br>ちらつき発生
    HostPTY->>RustPTY: PTY read
    RustPTY->>WASM: process_pty_data(data)
    Note over WASM: VT100 parse (2回目)
    WASM->>Canvas: render
```

### 2c. eMterm Mux Mode (解決策)

Shell → daemon reader → Unix socket → Tauri bridge → WASM → Canvas。
daemon は raw bytes をそのまま中継。**VT100 パースは WASM で1回だけ**。
tmux の二重パースボトルネックを完全に排除。

```mermaid
sequenceDiagram
    participant Shell as Shell Process
    participant Reader as pty_reader_loop<br>(std#58;#58;thread)
    participant Channel as mpsc channel<br>(capacity 256)
    participant ConnLoop as select! loop<br>(tokio)
    participant Socket as Unix Socket
    participant BridgeTask as output_stream<br>(tokio#58;#58;spawn)
    participant TauriEvent as Tauri Event
    participant MuxClient as MuxClient (TS)
    participant WASM as WasmGrid<br>process_pty_data()
    participant Canvas as Canvas Renderer

    Shell->>Reader: PTY read (raw bytes)
    Note over Reader: raw bytes をそのまま転送<br>VT100 パースしない
    Reader->>Channel: try_send(PtyOutputChunk)
    Channel->>ConnLoop: recv()
    ConnLoop->>Socket: raw bytes + pane_id header
    Socket->>BridgeTask: read frame
    BridgeTask->>TauriEvent: emit("mux-pty-output")
    TauriEvent->>MuxClient: onPtyOutput(pane_id, data)
    MuxClient->>WASM: process_pty_data(data)
    Note over WASM: VT100 parse (1回だけ)
    WASM->>Canvas: dirty rows → render
```

### 2d. レイテンシ比較

```mermaid
flowchart LR
    subgraph Normal["Normal Mode"]
        N1["Shell"] --> N2["Rust reader"] --> N3["Tauri Channel"] --> N4["WASM parse"] --> N5["Canvas"]
    end

    subgraph tmux["tmux Mode (問題)"]
        T1["Shell"] --> T2["tmux parse"] --> T3["tmux re-gen"] --> T4["Host PTY"] --> T5["Rust reader"] --> T6["WASM parse"] --> T7["Canvas"]
    end

    subgraph Mux["eMterm Mux Mode"]
        M1["Shell"] --> M2["daemon reader"] --> M3["Unix socket"] --> M4["Bridge"] --> M5["WASM parse"] --> M6["Canvas"]
    end
```

## 3. IPC Protocol Messages

```mermaid
flowchart LR
    subgraph ClientToServer["Client to Server"]
        Hello["0x03 Hello"]
        PtyInput["0x02 PtyInput"]
        CreateWindow["0x12 CreateWindow"]
        SplitPane["0x11 SplitPane"]
        DestroyPane["0x07 DestroyPane"]
        Resize["0x08 Resize"]
        Attach["0x09 Attach"]
        Detach["0x0A Detach"]
        SwitchWindow["0x13 SwitchWindow"]
        RenameWindow["0x14 RenameWindow"]
        DestroyWindow["0x15 DestroyWindow"]
    end

    subgraph ServerToClient["Server to Client"]
        Welcome["0x04 Welcome"]
        PtyOutput["0x01 PtyOutput"]
        PaneCreated["0x06 PaneCreated"]
        PtyExited["0x10 PtyExited"]
        Detached["0x0B Detached"]
        Error["0x0F Error"]
    end
```

## 4. Session State Hierarchy

```mermaid
classDiagram
    class SessionManager {
        -sessions: HashMap~SessionId, MuxSession~
        -next_session_id: u32
        -next_pane_id: u32
        +create_session(name) SessionId
        +find_pane(pane_id) Option~SessionId, WindowId~
        +find_window_session(window_id) Option~SessionId~
        +alloc_pane_id() u32
    }

    class MuxSession {
        +id: SessionId
        +name: String
        +windows: HashMap~WindowId, MuxWindow~
        +active_window_id: Option~WindowId~
        +add_window(window) WindowId
        +remove_window(window_id)
    }

    class MuxWindow {
        +id: WindowId
        +name: String
        +panes: HashMap~PaneId, MuxPane~
        +active_pane_id: Option~PaneId~
        +add_pane(pane) PaneId
        +remove_pane(pane_id)
    }

    class MuxPane {
        +id: PaneId
        +cols: u16
        +rows: u16
        +output_target: SharedOutputTarget
        -writer: Option~Arc~Mutex~Writer~~~
        -master: Option~Box~MasterPty~~
        +exited: bool
        +write_input(data)
        +resize(cols, rows)
        +mark_exited()
    }

    class PaneOutputTarget {
        <<enumeration>>
        Connected(mpsc Sender)
        Detached(DetachRingBuffer)
    }

    SessionManager "1" --> "*" MuxSession
    MuxSession "1" --> "*" MuxWindow
    MuxWindow "1" --> "*" MuxPane
    MuxPane --> PaneOutputTarget
```

## 5. Frontend Pane Layout Tree

```mermaid
flowchart TB
    subgraph LayoutTree["LayoutNode Binary Tree"]
        Root["split<br>direction: vertical<br>ratio: 0.5"]
        Left["leaf<br>paneId: 1"]
        Right["split<br>direction: horizontal<br>ratio: 0.5"]
        TopRight["leaf<br>paneId: 2"]
        BottomRight["leaf<br>paneId: 3"]

        Root --> Left
        Root --> Right
        Right --> TopRight
        Right --> BottomRight
    end

    subgraph Rendered["Rendered Layout"]
        P1["Pane 1<br>Canvas + WasmGrid<br>+ TerminalState<br>+ Renderer"]
        P2["Pane 2<br>Canvas + WasmGrid<br>+ TerminalState<br>+ Renderer"]
        P3["Pane 3<br>Canvas + WasmGrid<br>+ TerminalState<br>+ Renderer"]
    end

    subgraph Functions["Layout Functions"]
        calc["calculateLayout()"]
        split["splitPane()"]
        remove["removePane()"]
        resize["resizeSplitBetween()"]
        bounds["getSplitBounds()"]
        apply["applyLayoutToContainer()"]
    end

    LayoutTree -.-> calc
    calc -.-> Rendered
```

## 6. Prefix Key State Machine

```mermaid
stateDiagram-v2
    state "idle" as idle
    state "waiting" as waiting

    [*] --> idle

    idle --> waiting: Prefix key pressed<br>(default Ctrl+B)
    waiting --> idle: Action key pressed<br>(dispatch MuxAction)
    waiting --> idle: Timeout (2s)
    waiting --> idle: Unknown key<br>(ignored)

    state "MuxAction Dispatch" as actions {
        state "split-vertical" as sv
        state "split-horizontal" as sh
        state "next-pane" as np
        state "prev-pane" as pp
        state "close-pane" as cp
        state "zoom-toggle" as zt
        state "detach" as dt
        state "new-window" as nw
        state "next-window" as nxw
        state "prev-window" as pvw
        state "rename-window" as rw
        state "copy-mode" as cm
        state "paste" as ps
    }

    waiting --> actions: Keybind match
```

## 7. Copy Mode State Machine

```mermaid
stateDiagram-v2
    state "inactive" as inactive
    state "navigating" as navigating
    state "selecting" as selecting

    [*] --> inactive

    inactive --> navigating: prefix + [

    navigating --> navigating: h/j/k/l (move cursor)
    navigating --> navigating: 0/$ (line start/end)
    navigating --> selecting: v (start selection)
    navigating --> inactive: q / Escape

    selecting --> selecting: h/j/k/l (extend selection)
    selecting --> inactive: y (yank to clipboard)
    selecting --> inactive: q / Escape
```

## 8. Detach/Reattach Flow

```mermaid
sequenceDiagram
    participant GUI as GUI Client
    participant Daemon as Daemon
    participant SM as SessionManager
    participant Pane as MuxPane
    participant Ring as DetachRingBuffer
    participant PTY as PTY Process

    Note over GUI,PTY: Detach Flow
    GUI->>Daemon: Detach message
    Daemon->>SM: detach_session_panes()
    SM->>Pane: output_target = Detached(ring)
    Daemon->>GUI: Detached response
    GUI->>GUI: exitMuxMode()

    Note over PTY,Ring: While Detached
    PTY->>Pane: pty_reader_loop continues
    Pane->>Ring: ring.write(data)
    Note over Ring: Accumulates up to 64MB<br>oldest data overwritten

    Note over GUI,PTY: Reattach Flow
    GUI->>Daemon: Hello + handshake
    Daemon->>SM: collect_reattach_data()
    SM->>Ring: ring.read_all()
    SM->>Pane: output_target = Connected(tx)
    Daemon->>GUI: PaneCreated (per pane)
    Daemon->>GUI: PtyOutput (buffered data)
    GUI->>GUI: Replay into WasmGrid
```

## 9. Multi-Session Attach Flow

```mermaid
sequenceDiagram
    participant GUI as GUI Client
    participant Daemon as Daemon
    participant OldSession as Session A
    participant NewSession as Session B

    GUI->>Daemon: Attach(session_id=B)
    Daemon->>Daemon: Validate session B exists

    Daemon->>OldSession: detach_session_panes()
    Note over OldSession: All panes switch to<br>Detached(ring_buffer)

    Daemon->>Daemon: active_session_id = B

    Daemon->>NewSession: collect_reattach_data()
    Note over NewSession: Read ring buffers<br>Switch to Connected(tx)

    Daemon->>GUI: PaneCreated (per pane in B)
    Daemon->>GUI: PtyOutput (buffered data)
```

## 10. Graceful Shutdown

```mermaid
sequenceDiagram
    participant Signal as SIGTERM / SIGINT
    participant Daemon as run_daemon()
    participant Shutdown as graceful_shutdown()
    participant SM as SessionManager
    participant Pane as MuxPane

    Signal->>Daemon: Signal received
    Daemon->>Daemon: Break accept loop

    Daemon->>Shutdown: graceful_shutdown()
    Shutdown->>SM: Lock session manager

    loop For each session/window/pane
        SM->>Pane: mark_exited()
        Note over Pane: Drops writer + master<br>PTY process terminates
    end

    Shutdown->>Daemon: All PTYs closed
    Daemon->>Daemon: remove_file(socket)
    Note over Daemon: Daemon shutdown complete
```
