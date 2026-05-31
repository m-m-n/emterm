# Feature: Status Bar Native Port (egui)

## Overview

Port the full WebView status-bar feature set from `src/status-bar/` to the
native-poc (egui) build. The native-poc already ships a minimal status bar
(Phase 4-D: mux-supplied left/right + local clock); this task brings up the
remaining capabilities (template engine, four variable providers, App Line 2,
OSC 777;statusbar reception, settings fields) to achieve feature parity.
Concurrently, introduce a reusable HTML parser as the foundation for the
later Markdown-viewer port.

## Objectives

- Achieve full feature parity with the WebView status-bar (PoC Go criterion).
- Implement a template engine and four providers (`{time}`, `{cwd}`,
  `{git_branch}`, `{cmd:name}`) in Rust without adding tokio.
- Add `OSC 777;statusbar` dispatch (set/clear/show/hide) without breaking the
  existing emterm-extension OSC route (markdown/image/viewer).
- Build a common HTML parser (inline subset) reusable by both status-bar
  templates and the future Markdown viewer.
- Extend `Settings::StatusBarSettings` with the WebView-compatible fields;
  the JSON loader stays out of scope (Phase 7).

## User Stories

### US1: Default display

As a terminal user, I want the native-poc status bar to display the local
clock on the left and the current working directory's basename on the right
by default, so that the experience matches the WebView build.

**Acceptance Criteria:**
- [ ] App Line 1 left renders the current time formatted by `time_format`
- [ ] App Line 1 right renders the basename of the latest OSC 7 cwd
- [ ] The clock advances every second without freezing the terminal
- [ ] `Settings::default()` produces `app_line1_left="{time}"` and
      `app_line1_right="{cwd}"`

### US2: Git branch with status color

As a developer, I want to put `{git_branch}` into a template and see the
branch name colored according to clean / dirty / untracked state, so that I
know the repo state without running `git status`.

**Acceptance Criteria:**
- [ ] `{git_branch}` resolves to the branch name in the active tab's cwd
- [ ] Clean / dirty / untracked map to the three color tokens specified in
      FR5 of the requirements
- [ ] `git` execution runs on a worker thread, never blocking egui's UI loop
- [ ] Five-second timeout kills the process and retains the previous value

### US3: Custom commands

As a power user, I want to define `{cmd:battery}` style variables backed by
my own executables, so that I can surface arbitrary system state in the
bar.

**Acceptance Criteria:**
- [ ] `Settings::StatusBarSettings::custom_commands` accepts `{ executable,
      interval_ms }` entries keyed by name
- [ ] Name validation: `[a-zA-Z0-9_-]+`
- [ ] `~/` is expanded to the home directory; no other shell expansion
- [ ] Only the first trimmed line of stdout is displayed
- [ ] `interval_ms` is clamped to ≥ 1000ms

### US4: External content via OSC 777;statusbar

As a shell-script developer, I want to push content into the OSC layer via
`OSC 777;statusbar;...` sequences, so that scripts can surface ad-hoc
status without restarting the terminal.

**Acceptance Criteria:**
- [ ] `set;left;<content>` / `set;right;<content>` populate the OSC layer
- [ ] `clear`, `clear;left`, `clear;right` work as documented
- [ ] `show` / `hide` toggle OSC layer visibility
- [ ] All HTML tags are stripped from `<content>` before render (XSS)
- [ ] Existing OSC 777 routes (markdown/image/viewer) keep functioning

### US5: Mux 3-layer coexistence

As a mux user, I want the daemon-supplied `StatusUpdateMsg` to appear on the
OSC layer (3rd row) while my local templates keep rendering on App Line 1
and Line 2, so that I see both pieces of information without conflict.

**Acceptance Criteria:**
- [ ] OSC layer receives `StatusUpdateMsg.left/right` verbatim (no client
      re-resolution)
- [ ] App Line 1/2 render client-side templates concurrently
- [ ] All three rows share the same background (no visual separation)
- [ ] On `exitMuxMode` equivalent (mux disconnect), the OSC layer clears

### US6: Reusable HTML parser

As a contributor porting the Markdown viewer in a later phase, I want to
reuse the HTML parser shipped with this task, so that I avoid writing two
HTML parsers.

**Acceptance Criteria:**
- [ ] HTML parser lives in `native-poc/src/html/` as a standalone module
- [ ] Public API exposes `parse(input) -> Vec<Node>` and
      `to_rich_text_runs(&[Node], &Theme) -> Vec<egui::RichText>`
- [ ] The Node enum is extensible (block elements can be added without
      breaking inline consumers)
- [ ] `<script>` / `<style>` tags and their contents are dropped from the AST

## Technical Requirements

### Functional Requirements

- **FR1: Layer Structure** — Status bar renders three rows top-to-bottom:
  OSC layer, App Line 1, App Line 2. App Line 1 is always visible when the
  bar is enabled; OSC and Line 2 hide when both their left and right
  sections are empty. The whole bar is hidden when
  `settings.statusbar.enabled` is false. No opening/closing animation. All
  rows share the same panel background — no per-layer surface tint.

- **FR2: Template Engine** — Parse `{var}` and `{cmd:name}` patterns from
  template strings. Variable name regex:
  `[a-zA-Z_][a-zA-Z0-9_]*(?::[a-zA-Z0-9_-]+)?`. Resolve via registered
  providers. Unknown variables resolve to empty string. Providers that
  expose a color value (`get_color() -> Option<CssColor>`) get wrapped in
  `<font color="...">value</font>` so the HTML parser can colorize
  the run. Provider/`{cmd}` output is sanitized (`sanitize_provider_value`)
  to a `<font color>`-only subset: only `<font>` with a re-validated
  `color` attribute survives, every other tag (incl. `<span style>`) is
  dropped to plain text, all text is HTML-escaped, and unclosed `<font>`
  tags are balanced so a color cannot bleed past the substitution.

- **FR3: TimeProvider** — Format current local time using these tokens:
  `YYYY MM DD HH hh mm ss A`. Replace longest tokens first. Default format
  `"HH:mm:ss"`. Unix path uses `libc::localtime_r`; Windows uses
  `GetLocalTime` (or chrono if already transitively available — verify
  during implementation, default plan is the raw API).
  `TimeProvider` owns a dedicated timer thread that calls
  `wakeup.wake()` at `refresh_rates["time"]` (default 1000 ms) so the
  egui frame is repainted even when no PTY output is flowing. The
  timer thread shuts down on `Drop` via an `Arc<AtomicBool>` stop flag
  + `Condvar::wait_timeout`. `get_value()` itself stays
  pull-based (computes `Instant::now()` on call); the timer thread's
  sole responsibility is the repaint trigger.

- **FR4: CwdProvider** — Read `NativeCallbackState.cwd` for the active tab
  and emit the basename. Support `file://host/path` URIs (drop host,
  percent-decode path). Preserve `/` root; handle Windows drive roots
  (`C:\`). Empty string when OSC 7 has not been received.
  `CwdProvider` does NOT poll; it is driven entirely by events.
  The OSC 7 reception path (`NativeCallbacks::on_osc(OSC_CWD, ...)`)
  invokes `wakeup.wake()` directly so the next egui frame picks up the
  new basename. No internal timer thread is created for this provider.

- **FR5: GitBranchProvider** — Run `git rev-parse --abbrev-ref HEAD` and
  `git status --porcelain` on a dedicated worker thread at the configured
  interval (`refresh_rates["git_branch"]`, default 5000 ms). Branch
  parsing rejects empty / `fatal:` prefixed output. Status classification:
  - empty porcelain → `clean` → color `#4caf50`
  - any non-`??` line → `dirty` → color `#f9a825`
  - only `??` lines → `untracked` → color `#9e9e9e`
  - command failure / non-repo → empty string, no color
  Timeout 5 s kills the child via `std::process::Child::kill()`; previous
  value is retained.
  After each successful refresh, the worker bumps its internal version
  counter and calls `wakeup.wake()` so the egui frame redraws even when
  no PTY output is flowing.

- **FR6: CommandProvider** — Spawn a single executable (no args, no shell)
  on a worker thread at `interval_ms` (clamped to ≥ 1000 ms). `~/` is
  expanded to `$HOME` (or `%USERPROFILE%` on Windows). Stdout is
  `trim()`-ed and the first line is used. Failure / timeout → empty
  string; previous value is retained on timeout (matches WebView). Name
  validation: `[a-zA-Z0-9_-]+`.
  After each successful refresh, the worker bumps its internal version
  counter and calls `wakeup.wake()` so the egui frame redraws even when
  no PTY output is flowing.

- **FR7: OSC 777;statusbar Dispatch** — In
  `NativeCallbacks::on_osc(action_type=OSC_EMTERM_EXTENSION /* 100 */,
  data)`, branch on the first semicolon-delimited token. When it equals
  `statusbar`, route the remaining tokens into a new
  `StatusBarOscDispatcher` (described below). When it is anything else
  (e.g. `markdown`, `image`), push to the existing `osc_queue` as today.

  Supported subcommands:

  | Tokens after `statusbar;` | Effect |
  | --- | --- |
  | `set;left;<content>`   | OSC layer left ← strip(content); auto-show |
  | `set;right;<content>`  | OSC layer right ← strip(content); auto-show |
  | `clear`                | OSC layer cleared (both sides) |
  | `clear;left`           | OSC layer left cleared |
  | `clear;right`          | OSC layer right cleared |
  | `show`                 | OSC layer forced visible |
  | `hide`                 | OSC layer forced hidden |

  Unknown commands log at debug level and are ignored.

- **FR8: HTML Parser (common foundation)** — `native-poc/src/html/` module
  exposes:
  - `parse(input: &str) -> Vec<Node>`
  - `Node` enum: `Text(String)`, `LineBreak`,
    `Span { color: Option<CssColor>, children: Vec<Node> }`,
    `Bold(Vec<Node>)`, `Italic(Vec<Node>)`, `Underline(Vec<Node>)`
  - `to_rich_text_runs(nodes: &[Node], theme: &Theme) -> Vec<RichTextRun>`
    where `RichTextRun` is the egui-friendly run (text + per-run style)
    that the status-bar widget consumes.
  - Inline subset only (status-bar use). Block elements (`<p>`, `<div>`,
    `<pre>`, `<code>`, `<ul>`, `<ol>`, `<li>`, `<table>`, `<a>`,
    `<img>`, `<h1>`..`<h6>`) are reserved for the Markdown-viewer phase
    and **MUST be parseable to a future variant without breaking the
    public API** (e.g. add `Node::Block { kind, children }` later).
  - `<script>` / `<style>` tags drop both the tags and their contents.
  - `<font color="...">` maps to `Span { color, children }` (status-bar
    color markup); only the `color` attribute is honored.
  - HTML entities (`&amp; &lt; &gt; &quot; &apos; &#NN;`) decoded.

- **FR9: HTML Sanitizer (OSC route)** — A standalone helper
  `strip_html_tags(input: &str) -> String` removes all tags. Used by
  `StatusBarOscDispatcher` before writing to the OSC layer. Behavior
  mirrors WebView `stripHtmlTags` (see `src/status-bar/osc-controller.ts`):
  - Drop `<script>...</script>` and `<style>...</style>` blocks (tag + body)
  - Drop other opening/closing tags but keep their inner text
  - Preserve non-HTML angle brackets (`1 < 2`)

- **FR10: Settings extension** — Add fields to `StatusBarSettings`:
  ```rust
  pub struct StatusBarSettings {
      pub enabled: bool,               // existing
      pub position: StatusBarPosition, // existing
      pub app_line1_left: String,      // default "{time}"
      pub app_line1_right: String,     // default "{cwd}"
      pub app_line2_left: String,      // default ""
      pub app_line2_right: String,     // default ""
      pub time_format: String,         // default "HH:mm:ss"
      pub font_size: Option<f32>,      // default None
      pub custom_commands: HashMap<String, CustomCommand>, // default {}
      pub refresh_rates: HashMap<String, u64>,             // default {}
  }
  pub struct CustomCommand {
      pub executable: String,
      pub interval_ms: u64, // default 1000
  }
  ```
  The JSON loader is **out of scope** (Phase 7). This task uses only
  `Settings::default()` values.

- **FR11: Mux integration** — Preserve `Tab::apply_mux_message` for
  `MessageType::StatusUpdate`. The view-model produced by
  `App::status_bar_state()` is replaced with a richer struct that carries:
  (a) the mux session badge + daemon `left/right` (-> OSC layer, no client
  re-resolution), and (b) per-tab CallbackState reference / cwd / git
  cache used by client-side providers (-> App Line 1/2 templates). On mux
  disconnect (detected via tab dropping its `mux_session_name`), the OSC
  layer is cleared.

- **FR12: Auto layer visibility** — In `draw`, before rendering each row,
  check `left.is_empty() && right.is_empty()`. If true and the row is OSC
  or App Line 2, skip rendering (the row vanishes without resizing the
  panel — total panel height is dynamic, sized by visible rows × row
  height). App Line 1 is always rendered when the bar is enabled.

### Non-Functional Requirements

- **NFR1 - Performance:** Template resolution and HTML parsing must each
  complete in < 1 ms per status-bar render. Variable providers run on
  worker threads so the egui render loop is never blocked by external
  processes. Identical resolved output between frames bypasses rebuild of
  the `RichText` run list (cached by template-string + provider-version
  pair).

  **Refresh-redraw architecture (provider ownership)**: Each provider
  that needs periodic refresh owns its own timer / worker thread and
  receives an `Arc<Wakeup>` via its constructor. When the provider's
  underlying value advances (TimeProvider tick, GitBranch refresh,
  CommandProvider refresh) the worker bumps a version counter and calls
  `wakeup.wake()`, which posts an event to the winit event loop and
  triggers a redraw. The render layer MUST NOT rely on
  `egui::Context::request_repaint_after` for periodic refresh — that API
  is internal to egui and does not bridge back to winit, so the frame
  stalls when no other source of input is active. See "Notes" below.

  **winit ApplicationHandler::user_event MUST request redraw**: The
  `Wakeup` implementation calls `EventLoopProxy::send_event(())`, which
  wakes the winit event loop and dispatches a `UserEvent(())` to the
  `ApplicationHandler`. winit 0.30's `ApplicationHandler::user_event`
  has a default implementation that does nothing, so the wake-up
  signal is dropped on the floor unless the application explicitly
  overrides it. native-poc therefore MUST implement
  `ApplicationHandler::user_event` on `PocApp` and, when a window
  host exists, call `host.window().request_redraw()` so the next frame
  is scheduled. Without this hook the provider-owned wake chain is
  half-wired: the event loop wakes but no redraw is requested, and
  periodic refresh (clock tick, git refresh, command refresh) silently
  fails again. See "Notes" below.

- **NFR2 - Security:** OSC 777 content is fully tag-stripped (no inline
  HTML allowed from external scripts). Custom command (`{cmd}`) output is
  sanitized to a `<font color>`-only subset (`sanitize_provider_value`):
  the `color` attribute is re-validated and re-serialized so a crafted
  attribute cannot break out of the tag, every other tag is dropped, text
  is HTML-escaped, and unclosed `<font>` tags are balanced. Custom
  commands accept a single executable path with `~/` expansion only — no
  shell invocation, no argument injection. Command name validation rejects
  characters outside `[a-zA-Z0-9_-]`. `<script>` and `<style>` blocks are
  dropped from the HTML AST entirely.

- **NFR3 - Platform:** Linux and Windows only (no macOS, matches
  project-wide policy). Unix paths use `libc::localtime_r`; Windows uses
  `GetLocalTime` or equivalent. `libc`-gated code wears
  `#[cfg(unix)]` / `#[cfg(windows)]` per CLAUDE.md guidance.

- **NFR4 - Visual consistency:** Follow the existing UI Design Guidelines
  (`doc/UI-DESIGN-GUIDELINES.yaml`). Use the same panel background as the
  rest of the egui frame — do not introduce per-layer surface tints. No
  open/close animations (matches WebView intentional behavior recorded in
  `project_status_bar_design.md`).

- **NFR5 - Extensibility:** The HTML parser's `Node` enum and the
  template engine's provider trait must accept new variants without
  breaking existing call sites. Status-bar use today must not paint the
  parser into a corner for the Markdown-viewer port.

## Implementation Approach

### Architecture

```
┌────────────────────────────────────────────────────────────────────┐
│ native-poc App (egui frame)                                        │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ Status Bar Panel (TopBottomPanel)                             │  │
│  │ ┌──────────────────────────────────────────────────────────┐ │  │
│  │ │ OSC Layer (mux StatusUpdateMsg + OSC 777;statusbar)     │ │  │
│  │ │   left:  daemon.left or osc_set;left content            │ │  │
│  │ │   right: daemon.right or osc_set;right content          │ │  │
│  │ ├──────────────────────────────────────────────────────────┤ │  │
│  │ │ App Line 1 (client templates)                            │ │  │
│  │ │   TemplateEngine.resolve(app_line1_left)                 │ │  │
│  │ │   TemplateEngine.resolve(app_line1_right)                │ │  │
│  │ ├──────────────────────────────────────────────────────────┤ │  │
│  │ │ App Line 2 (client templates)                            │ │  │
│  │ │   ditto for app_line2_*                                  │ │  │
│  │ └──────────────────────────────────────────────────────────┘ │  │
│  └──────────────────────────────────────────────────────────────┘  │
│                                                                    │
│  StatusBarRuntime (per App, single instance)                       │
│   ├─ Arc<Wakeup>            ← shared with every provider below     │
│   ├─ TemplateEngine                                                │
│   │   ├─ TimeProvider            (own timer thread → wake)         │
│   │   ├─ CwdProvider     ← reads active tab's cb_state.cwd          │
│   │   │                          (no thread; wake fires on OSC 7)  │
│   │   ├─ GitBranchProvider       (worker thread → wake on refresh) │
│   │   └─ CommandProvider × N     (worker thread → wake on refresh) │
│   └─ OscLayerState { left: String, right: String, visible: bool }  │
│                                                                    │
│  NativeCallbacks::on_osc(100, data)                                │
│   └─ if data starts with "statusbar;" → StatusBarOscDispatcher     │
│      else → existing osc_queue                                     │
│                                                                    │
│  Tab::apply_mux_message(StatusUpdate) → tab.mux_status_state       │
│   └─ App::status_bar_view_model() pulls left/right into OscLayer   │
└────────────────────────────────────────────────────────────────────┘
```

### Data Flow

**Per-frame render (App Line 1/2):**
```
egui draw_terminal
  → app.status_bar_view_model()
      → for line in [Line1, Line2]:
          for side in [Left, Right]:
              template_engine.resolve(template)
                → providers[var].get_value()
                → wrap in <font color> if get_color() Some
              html_parser.parse(resolved)
              cache_if_unchanged()
  → ui::status_bar::draw(ctx, view_model, settings)
```

**OSC 777;statusbar reception:**
```
PTY bytes
  → TerminalCore::process_pty_data
  → osc dispatch
  → NativeCallbacks::on_osc(100, "statusbar;set;left;Hello")
      → StatusBarOscDispatcher::handle(["set", "left", "Hello"])
          → osc_layer.left = strip_html_tags("Hello")
          → osc_layer.visible = true
  → next frame: draw uses osc_layer.{left,right,visible}
```

**Mux daemon StatusUpdate:**
```
APC frame
  → MuxMessage::decode
  → Tab::apply_mux_message(StatusUpdate)
  → tab.mux_status_state = StatusUpdateMsg { left, right }
  → next frame: App::status_bar_view_model() maps this onto OscLayer.{left,right}
```

**GitBranchProvider worker:**
```
worker thread (sleep interval_ms)
  → read shared cwd (active tab's cb_state)
  → spawn `git rev-parse --abbrev-ref HEAD` with 5s timeout
  → spawn `git status --porcelain` with 5s timeout
  → write { branch, state } into Arc<Mutex<GitCache>>
  → version_counter += 1
  → wakeup.wake()                            (Arc<Wakeup> injected via ctor)
  → EventLoopProxy::send_event(())           (inside wake())
  → winit event loop wakes
  → ApplicationHandler::user_event           (PocApp impl)
  → host.window().request_redraw()
  → next frame redraws status bar
```

**TimeProvider timer thread:**
```
timer thread (Condvar::wait_timeout(refresh_rates["time"]))
  → check stop flag → break if true
  → wakeup.wake()                            (Arc<Wakeup> injected via ctor)
  → EventLoopProxy::send_event(())
  → winit event loop wakes
  → ApplicationHandler::user_event           (PocApp impl)
  → host.window().request_redraw()
  → next frame computes Instant::now() inside get_value()
```

**CwdProvider event-driven path:**
```
PTY bytes containing OSC 7
  → TerminalCore::process_pty_data
  → NativeCallbacks::on_osc(OSC_CWD, "file://host/path")
      → cb_state.cwd = decoded path
      → CwdProvider::set_cwd(new) → wakeup.wake()
  → EventLoopProxy::send_event(())
  → winit event loop wakes
  → ApplicationHandler::user_event           (PocApp impl)
  → host.window().request_redraw()
  → next frame: CwdProvider::get_value() reads the new cwd
```

### Module Layout

```
native-poc/src/
├── settings.rs                     # +CustomCommand, +StatusBarSettings fields
├── status_bar/                     # NEW: client-side status bar runtime
│   ├── mod.rs                      # StatusBarRuntime, ViewModel
│   ├── template_engine.rs          # parse + resolve, regex-free
│   ├── osc_dispatcher.rs           # OSC 777;statusbar handler
│   └── providers/
│       ├── mod.rs                  # VariableProvider trait
│       ├── time.rs
│       ├── cwd.rs
│       ├── git_branch.rs           # worker thread
│       └── command.rs              # worker thread
├── html/                           # NEW: shared HTML parser
│   ├── mod.rs                      # parse(), Node, RichTextRun
│   ├── tokenizer.rs                # tag/entity tokenizer
│   ├── parser.rs                   # token → AST
│   ├── sanitizer.rs                # strip_html_tags()
│   └── rich_text.rs                # AST → egui RichText runs
├── ui/
│   └── status_bar.rs               # UPDATE: 3-row layout + template render
├── callbacks.rs                    # UPDATE: dispatch statusbar prefix in OSC 100
└── app.rs                          # UPDATE: status_bar_view_model() construction
```

### Status Bar View Model

Replaces the current `StatusBarState`:

```rust
pub struct StatusBarViewModel {
    /// Whole-bar enable flag.
    pub enabled: bool,
    pub position: StatusBarPosition,
    pub font_size: Option<f32>,

    /// Row 1 (OSC layer). Sourced from either mux StatusUpdateMsg
    /// (daemon-resolved, verbatim) or OSC 777;statusbar (HTML-stripped).
    /// Hidden when both `left` and `right` are empty.
    pub osc: OscRow,

    /// Row 2 (App Line 1). Always rendered when `enabled`.
    pub app_line1: AppRow,

    /// Row 3 (App Line 2). Hidden when both sides resolve to empty.
    pub app_line2: AppRow,

    /// Optional mux session badge prepended to App Line 1 (or its own
    /// position — TBD during implementation; spec keeps both options open).
    pub mux_session_name: Option<String>,
}

pub struct OscRow {
    pub left: String,
    pub right: String,
    pub forced_visible: Option<bool>, // OSC 777 show/hide override
}

pub struct AppRow {
    /// Resolved + HTML-parsed runs ready to feed into egui.
    pub left: Vec<RichTextRun>,
    pub right: Vec<RichTextRun>,
}
```

### Template Engine

```rust
pub struct TemplateEngine {
    providers: HashMap<String, Box<dyn VariableProvider>>,
}

pub trait VariableProvider: Send + Sync {
    fn get_value(&self) -> String;
    fn get_color(&self) -> Option<CssColor> { None }
}

impl TemplateEngine {
    pub fn extract_variables(template: &str) -> Vec<String>;
    pub fn register(&mut self, name: &str, provider: Box<dyn VariableProvider>);
    pub fn unregister(&mut self, name: &str);
    pub fn has_provider(&self, name: &str) -> bool;

    /// Resolve a template into an HTML-bearing string.
    /// Colors from providers wrap the value in `<font color="...">`.
    pub fn resolve(&self, template: &str) -> String;
}
```

Variable name pattern (compiled by a small handwritten scanner, no `regex`
crate dependency):

```
{ <name> }
name := [a-zA-Z_][a-zA-Z0-9_]*  ( ':' [a-zA-Z0-9_-]+ )?
```

### OSC Dispatcher

```rust
pub struct StatusBarOscDispatcher {
    osc_layer: Arc<Mutex<OscLayerState>>,
}

pub struct OscLayerState {
    pub left: String,
    pub right: String,
    pub forced_visible: Option<bool>,
}

impl StatusBarOscDispatcher {
    /// Called with the slice of tokens after the leading `statusbar;`
    /// (which the caller has already verified).
    pub fn handle(&self, tokens: &[&str]) { /* set/clear/show/hide */ }
}

/// Hot path: returns true if the payload was a statusbar command (and was
/// dispatched). Returns false to let the caller fall through to the legacy
/// emterm-extension `osc_queue`.
pub fn try_dispatch_statusbar(
    dispatcher: &StatusBarOscDispatcher,
    payload: &str,
) -> bool {
    let mut it = payload.split(';');
    if it.next() != Some("statusbar") {
        return false;
    }
    let rest: Vec<&str> = it.collect();
    dispatcher.handle(&rest);
    true
}
```

Wired into `NativeCallbacks::on_osc`:

```rust
OSC_EMTERM_EXTENSION => {
    if !try_dispatch_statusbar(&self.statusbar_dispatcher, data) {
        self.state.lock().osc_queue.push(EmtermOscRequest {
            payload: data.to_string(),
        });
    }
}
```

### HTML Parser

```rust
pub fn parse(input: &str) -> Vec<Node>;

pub enum Node {
    Text(String),
    LineBreak,
    Span { color: Option<CssColor>, children: Vec<Node> },
    Bold(Vec<Node>),
    Italic(Vec<Node>),
    Underline(Vec<Node>),
    // Future (Markdown viewer phase):
    //   Block { kind: BlockKind, children: Vec<Node> }
    //   Link { href: String, children: Vec<Node> }
    //   Image { src: String, alt: String }
}

pub enum CssColor {
    Hex(u8, u8, u8),       // #RRGGBB
    Rgb(u8, u8, u8),       // rgb(r,g,b)
    Named(&'static str),   // CSS named color
}

pub struct RichTextRun {
    pub text: String,
    pub color: Option<egui::Color32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

pub fn to_rich_text_runs(nodes: &[Node], theme: &Theme) -> Vec<RichTextRun>;

pub fn strip_html_tags(input: &str) -> String;
```

**Tokenizer rules:**
- Recognize `<tagname attrs...>`, `</tagname>`, `<tagname/>` as tags
- Recognize `&entityname;` / `&#NN;` as entities
- Treat anything else (including `<` followed by whitespace or digits) as text
- Self-closing only for `<br>` `<br/>`

**Parser rules:**
- Inline-only nesting: tags close in the order they opened; mismatched
  closes are tolerated (consume + ignore + warn at debug)
- `<script>` and `<style>` start a "swallow until matching close" mode
- Unknown tags: keep their children, drop the tag wrapper (matches lenient
  browser behavior; safer than failing closed)
- Attribute parsing: `color` on `<font>` and `style` (`color: ...`) on
  `<span>` are interpreted today; both map to `Span { color }`. Future
  variants can extend this without touching the surface API

### GitBranchProvider Threading

```rust
pub struct GitBranchProvider {
    cache: Arc<Mutex<GitCache>>,
    cwd_source: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    handle: Option<std::thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

#[derive(Default, Clone)]
struct GitCache {
    branch: String,
    color: Option<CssColor>,
    last_cwd: Option<String>,
}
```

Worker loop:
1. Sleep `refresh_rates["git_branch"]` (default 5000 ms) using
   `Condvar::wait_timeout` so `stop` can interrupt.
2. Read cwd from `cwd_source()`. If empty / unchanged + non-empty cache,
   skip this iteration.
3. Spawn `git rev-parse --abbrev-ref HEAD` with 5 s timeout (wait_timeout
   on `Child` + kill on expiry).
4. If branch is empty / `fatal:` → cache cleared.
5. Else spawn `git status --porcelain` with 5 s timeout.
6. Classify state (clean/dirty/untracked) and update cache + call
   `wakeup::wake()` so the UI redraws.

On `Drop`: set `stop = true`, join handle.

### CommandProvider Threading

Same pattern as `GitBranchProvider`, but executes a single user-defined
executable. `~` expansion is a one-time substitution in the constructor.
Spawn uses `std::process::Command::new(executable)` with no args.

### Tokio: not used

native-poc has no tokio dependency today (see `native-poc/Cargo.toml`).
This task keeps it that way. All async behavior is thread-based.

### Dependencies

**Internal dependencies:**
- `native-poc/src/callbacks.rs` — `NativeCallbacks` gains a
  `statusbar_dispatcher` field
- `native-poc/src/app.rs` — `App` gains a `status_bar_runtime` field;
  `status_bar_state()` replaced by `status_bar_view_model()`
- `native-poc/src/tabs.rs` — `Tab` already exposes `mux_status_state`;
  no schema change
- `native-poc/src/ui/status_bar.rs` — rewritten to consume the new
  `StatusBarViewModel` with three rows + Layout::left_to_right /
  right_to_left for sections
- `native-poc/src/settings.rs` — extend `StatusBarSettings`, add
  `CustomCommand`
- `native-poc/src/wakeup.rs` — invoked by worker threads when caches change

**External dependencies (no new crates):**
- `std::process::Command` — git / custom commands
- `std::thread`, `std::sync::{Arc, Mutex, Condvar, atomic::AtomicBool}` —
  worker threading
- `libc` (unix) / `winapi` (windows, gated) — local time
- Existing `egui` 0.29 — RichText composition
- `image` 0.25 (default-features = false) — Lanczos3 downscale for color
  emoji; accepted as a hard need (swash bilinear strike scaling is too soft
  at the status bar's ~10x reduction)

No new crates unless implementation discovers a hard need. Specifically:
- **No `regex` crate.** Template variable scanner is handwritten (less
  than 100 LOC, single-pass).
- **No `tokio`.** Worker threads + std primitives only.
- **No HTML crate.** Custom tokenizer/parser per FR8 (the parser is the
  entire deliverable for HTML reuse — pulling in `html5ever` would be
  overkill for the inline subset).

### File Structure

```
native-poc/src/
├── settings.rs                # MOD: extend StatusBarSettings, add CustomCommand
├── status_bar/                # NEW
│   ├── mod.rs                 # StatusBarRuntime + ViewModel construction
│   ├── template_engine.rs
│   ├── osc_dispatcher.rs
│   ├── runtime.rs             # ties providers + view-model
│   └── providers/
│       ├── mod.rs             # VariableProvider trait
│       ├── time.rs
│       ├── cwd.rs
│       ├── git_branch.rs
│       └── command.rs
├── html/                      # NEW
│   ├── mod.rs                 # public API re-exports
│   ├── tokenizer.rs
│   ├── parser.rs
│   ├── sanitizer.rs           # strip_html_tags()
│   └── rich_text.rs           # AST → RichTextRun
├── ui/
│   └── status_bar.rs          # MOD: 3-row rendering against ViewModel
├── callbacks.rs               # MOD: wire StatusBarOscDispatcher into OSC 100
└── app.rs                     # MOD: own StatusBarRuntime, build ViewModel
```

## Test Scenarios

### Unit Tests

- [ ] `TemplateEngine::extract_variables` finds `{time}`, `{cmd:foo}`,
      duplicates included
- [ ] `TemplateEngine::resolve` substitutes registered providers
- [ ] `TemplateEngine::resolve` returns "" for unknown variable
- [ ] `TemplateEngine::resolve` wraps value in `<font color="...">`
      when provider supplies a color
- [ ] `sanitize_provider_value` keeps `<font color>`, drops other tags,
      and closes unclosed `<font>` so color does not bleed
- [ ] `TimeProvider::format` handles all tokens (YYYY MM DD HH hh mm ss A)
- [ ] `TimeProvider::format` AM / PM boundary at noon / midnight
- [ ] `CwdProvider` extracts basename from `/home/me`,
      `file:///home/me/x`, `C:\foo\bar`, `/`, percent-encoded paths
- [ ] `GitBranchProvider::parse_branch` handles `main`, empty, `fatal:`
- [ ] `GitBranchProvider::parse_status` distinguishes clean/dirty/untracked
- [ ] `CommandProvider` expands `~/` to home directory
- [ ] `CommandProvider` clamps interval_ms to ≥ 1000
- [ ] `strip_html_tags` removes `<script>...</script>` body, keeps text,
      preserves `1 < 2`
- [ ] `html::parse` returns `Span` for `<span style="color:#fff">x</span>`
- [ ] `html::parse` returns `Span` for `<font color="#fff">x</font>`
- [ ] `html::parse` parses nested `<b><i>x</i></b>`
- [ ] `html::parse` decodes `&amp; &lt; &gt; &#65;`
- [ ] `html::parse` drops `<script>` content entirely
- [ ] `html::parse` accepts unknown tag (keeps children, drops wrapper)
- [ ] `to_rich_text_runs` flattens nested style into per-run flags
- [ ] `try_dispatch_statusbar` returns false for non-statusbar payloads
- [ ] `StatusBarOscDispatcher::handle` set/clear/show/hide round trip

### Integration Tests

- [ ] `OSC 777;statusbar;set;left;Hello` routed through
      `NativeCallbacks::on_osc` updates the OSC layer left
- [ ] `OSC 777;markdown;...` still pushed onto `osc_queue` (regression)
- [ ] StatusUpdateMsg arrival populates OSC layer's left/right
- [ ] Mux session drop clears OSC layer
- [ ] App Line 2 hides when both templates resolve to empty
- [ ] Disabling `statusbar.enabled` removes the panel (existing test
      `disabled_status_bar_does_not_insert_panel` extended for 3-row case)

### E2E Tests

**Existing E2E tests:** `e2e-tests/` (WebdriverIO + tauri-driver, runs
through the legacy Tauri build)
**Run command:** `./scripts/run-e2e-docker.sh`
**Native-poc-specific:** native-poc does not yet have an E2E harness;
verification is via `cargo test` and manual run from
`native-poc/target-host/release/emterm-native-poc` per
`.claude/rules/native-poc-build-location.md`.

- [ ] Existing Tauri-side E2E tests pass without regression (no changes
      to `src/status-bar/`)
- [ ] Manual smoke run of native-poc: status bar shows time + cwd by default
- [ ] Manual smoke run: `printf '\033]777;statusbar;set;left;hi\033\\'`
      updates the OSC layer

### Edge Cases

- [ ] OSC layer both sides empty → row hidden
- [ ] App Line 2 both sides empty → row hidden
- [ ] `git_branch` provider when not in a repo → empty, no color
- [ ] Custom command timeout (5 s) → previous value retained, no UI hang
- [ ] Custom command produces multi-line output → first line only
- [ ] OSC 777;statusbar with unknown subcommand → log + no-op
- [ ] OSC 777;statusbar;set;left;`<script>x</script>foo` → "foo"
- [ ] Mux disconnect mid-frame → OSC layer cleared next frame
- [ ] Template containing only unknown variables → resolves to empty
      string → row may hide if applicable
- [ ] HTML parser malformed close tag (`<b>x</i>`) → tolerated, debug log
- [ ] HTML entity `&#0;` → null char or skip (specify: skip; warn at debug)
- [ ] Tab switch updates cwd / git cache to new active tab
- [ ] `CustomCommand` name with `;` rejected by validation
- [ ] `~/` expansion when `$HOME` is unset → empty (don't crash)

### Performance Tests

- [ ] Template resolution benchmark: 100k iterations × 4 sections / second
- [ ] HTML parse benchmark: 10k iterations of typical OSC payload / second
- [ ] No render frame drop while git worker is running (visual inspection
      + frame timing log)

## Security Considerations

- **Authentication / Authorization:** N/A (local app, no auth surface).
- **Input Validation:**
  - Custom command names match `[a-zA-Z0-9_-]+`; rejects path traversal in
    the name space.
  - `executable` must be absolute or start with `~/`; relative paths
    rejected.
  - OSC layer content is fully tag-stripped (`strip_html_tags`).
- **Data Protection:** No sensitive data persisted. Status bar contents
  are ephemeral.
- **XSS Prevention:** External (OSC 777) content forbidden HTML — pure
  text only. Internal (user-configured template) content allows the
  inline subset; user-controlled.
- **Process Sandbox:** Custom commands run with no shell, no args, the
  user's privileges (same as the rest of native-poc). Five-second
  timeout prevents hung children from leaking.

## Error Handling

| Scenario | Handling |
| --- | --- |
| Unknown template variable | Resolve to empty string |
| Provider returns empty value | Treat as empty in render; row visibility re-evaluated |
| Custom command fails to spawn | Value = ""; log at `warn` |
| Custom command times out (≥ 5 s) | Kill child, retain previous value; log at `warn` |
| `git` not on PATH | `git_branch` empty; log once at `debug` |
| Not a git repo (`fatal: ...`) | `git_branch` empty, no color |
| OSC 777 statusbar with unknown subcommand | Log at `debug`, ignore |
| OSC 777 statusbar with non-existent section (e.g. `set;mid`) | Log at `debug`, ignore |
| OSC 7 never received | `{cwd}` resolves to empty string |
| HTML parser receives malformed tags | Tolerate (drop wrapper), log at `debug` |
| HTML entity unrecognized | Emit literal `&entity;` text |
| Mux daemon disconnect mid-update | OSC layer cleared on next frame after `mux_session_name` drop |
| Settings load fails (Phase 7) | N/A this task; defaults used unconditionally |

## Performance Optimization

### Performance Goals

- Template resolution: < 1 ms per row per frame.
- HTML parse: < 1 ms for typical OSC payloads (≤ 256 bytes).
- Frame render: no measurable fps impact over the Phase 4-D baseline
  (verified by visual inspection + render-loop log).

### Optimization Strategies

- **Run-list caching:** Cache `(template_str, provider_version_tuple) →
  Vec<RichTextRun>`. Recompute only when the template changes or any
  involved provider bumps its version counter.
- **Per-provider versioning:** Each provider increments a `u64` when its
  underlying value changes (cwd update, git fetch result, time tick).
  This skips the regex scan + HTML parse on identical frames.
- **Worker threads for IO:** All process spawns (git, custom commands)
  happen off the UI thread, with `wakeup::wake()` notifying egui when a
  redraw is warranted.
- **Diff-render:** `StatusBarViewModel` is built each frame, but the
  widget compares against the previous frame's hash and skips egui
  emission when unchanged.

### Caching Strategy

| What | Mechanism | TTL |
| --- | --- | --- |
| Time string | Recomputed every frame (cheap); TimeProvider's own timer thread calls `wakeup.wake()` at this rate to trigger redraw | `refresh_rates["time"]` (default 1000 ms) |
| Cwd basename | Updated on OSC 7 event + on tab switch; OSC 7 handler calls `wakeup.wake()` directly (no provider-owned thread) | event-driven |
| Git branch / state | Worker thread cache | `refresh_rates["git_branch"]` (default 5000 ms) |
| Custom command output | Worker thread cache | `CustomCommand.interval_ms` (≥ 1000 ms) |
| Template runs | LRU keyed by `(template, provider_version_tuple)` | reset on template change |

## Success Criteria

- [ ] All functional requirements (FR1–FR12) implemented and tested
- [ ] All listed test scenarios pass (`cargo test` from `native-poc/`)
- [ ] No new external crates added (verified via `Cargo.toml` diff review)
      — exception: `image` (default-features = false) is accepted as a hard
      need for high-quality Lanczos3 color-emoji downscale; swash's bilinear
      strike scaling is too soft at the status bar's ~10x reduction
- [ ] Native-poc launches with status bar showing time + cwd by default
- [ ] `OSC 777;statusbar;set;left;X` updates the OSC layer (manual smoke)
- [ ] `OSC 777;markdown;...` still triggers the emterm-extension queue
      (regression preserved)
- [ ] Mux-connected tab shows daemon left/right in OSC layer while App Line
      1/2 render local templates (3-layer coexistence)
- [ ] Linux build (`CARGO_TARGET_DIR=./target-host cargo build --release`)
      succeeds and produces a working binary
- [ ] Windows cross-build still compiles (gated on libc paths)
- [ ] HTML parser exposes `Node`, `parse`, `to_rich_text_runs`,
      `strip_html_tags` for the future Markdown-viewer port to consume

## Open Questions

> **Note**: 未解決の要件は sdd.yaml で `status: tbd` として管理されています。
> `/em-sdd:sdd.2-create-plan` の実行前に解決してください。

(No `tbd` items at SPEC-time — implementation-detail choices like "regex
crate vs handwritten" are deferred to the planning step, not the spec.)

## Notes

### `egui::Context::request_repaint_after` does not bridge to winit

The first verification cycle shipped a release binary where the
status-bar clock did not advance while the shell was idle. Root cause:
`request_repaint_after(Duration::from_secs(1))` is observed by egui's
internal repaint scheduler but does not post any event back to the
winit event loop. When no other input (PTY bytes, cursor blink, mouse,
keyboard) is active, winit never wakes, so the frame stalls and the
clock freezes. The `native-poc/src/window_host.rs` integration layer
already documents this constraint as "egui's request_repaint_after is
silent (no callback bridges it back to winit)".

The fix (mandated by FR3 / FR5 / FR6 / NFR1 above) is that any provider
that needs periodic refresh owns its own thread and calls
`wakeup.wake()` (the existing `EventLoopProxy::send_event` wrapper used
by the PTY reader thread). Future contributors MUST NOT reintroduce
`request_repaint_after` as a substitute for `wake()`.

### winit 0.30: `EventLoopProxy::send_event` requires `ApplicationHandler::user_event`

The second verification cycle landed the provider-ownership refresh
chain (each provider owns a thread + calls `wakeup.wake()`) but the
release-build clock **still** froze when the shell was idle. Root
cause: winit 0.30's `ApplicationHandler::user_event` has a no-op
default impl. `Wakeup` calls `EventLoopProxy::send_event(())`, which
**does** wake the winit event loop and dispatch a `UserEvent(())` —
but with no override, the handler does nothing, no `request_redraw()`
is issued, and `about_to_wait` finds no reason to redraw because the
existing condition (`pty_changed || ime_changed || blink_due`) doesn't
include a "status-bar tick" signal. The frame stalls again, even though
every layer below the `ApplicationHandler` was correct.

The fix (mandated by NFR1 above and `application-handler-user-event`
task) is to override `ApplicationHandler::user_event` on `PocApp` so
that any `UserEvent(())` triggers `host.window().request_redraw()`
when a window host exists. This closes the loop:

```
provider thread
  → wake()
  → EventLoopProxy::send_event(())
  → winit event loop wakes
  → ApplicationHandler::user_event(_, ())         (MUST be implemented)
  → host.window().request_redraw()
  → next frame
```

Future contributors MUST NOT remove the `user_event` override or
narrow its `request_redraw()` to a subset of UserEvents. If new
UserEvent variants are introduced, they MUST still result in a
redraw request (or have an explicit, tested reason not to).
Verification owns this via TS-32 (handler-level unit test) and TS-30
(release-build idle-clock smoke).

## Implementation Phases

### Phase A: HTML Parser Foundation

**Goals:** Stand up the reusable parser before any UI work depends on it.
**Deliverables:**
- `native-poc/src/html/` module with `parse`, `Node`, `strip_html_tags`,
  `to_rich_text_runs`, `CssColor`, `RichTextRun`
- Unit tests covering inline subset, entities, malformed input, sanitizer
- No call sites yet — pure library

### Phase B: Settings Extension

**Goals:** Land the new `StatusBarSettings` fields and `CustomCommand`.
**Deliverables:**
- Updated `Settings::default()` shape
- Default-value tests (already pattern in `settings.rs`)
- No behavior change in existing code (new fields unused until Phase D)

### Phase C: Template Engine + Providers

**Goals:** Resolve templates without UI integration.
**Deliverables:**
- `status_bar/template_engine.rs`
- `status_bar/providers/{time,cwd,git_branch,command}.rs`
- Worker-thread infrastructure for git / command providers
- Unit tests for engine + each provider
- Smoke test: register all four providers + resolve a representative
  template

### Phase D: OSC 777;statusbar Dispatcher

**Goals:** Receive external content.
**Deliverables:**
- `status_bar/osc_dispatcher.rs`
- `NativeCallbacks` wired to call `try_dispatch_statusbar` before
  pushing to `osc_queue`
- Unit + integration tests covering the routing fork

### Phase E: View-Model + UI Integration

**Goals:** Replace the Phase 4-D `StatusBarState` with the new 3-row
view-model and wire it into `App`.
**Deliverables:**
- `StatusBarRuntime` in `App` (owns engine + dispatcher + providers)
- `App::status_bar_view_model()` builds the per-frame snapshot
- `ui::status_bar::draw` rewritten for 3 rows + Layout::left_to_right /
  right_to_left sections
- Mux integration: OSC layer fed from `tab.mux_status_state` when present;
  falls back to OSC 777;statusbar state otherwise
- Tests adapted from existing TS-status-1/2/3 to the new view-model

### Phase F: Polish + Verification

**Goals:** Round trip and verify against the WebView build.
**Deliverables:**
- Diff-render caching (run-list keyed on provider versions)
- Manual smoke checklist from Success Criteria
- Native-poc binary built via `CARGO_TARGET_DIR=./target-host` per
  `.claude/rules/native-poc-build-location.md`
- Doc update: `native-poc/README.md` references new status bar
  capabilities (if README is the right surface — implementation step
  will confirm)

## References

- WebView status bar SPEC: `doc/tasks/status-bar/SPEC.md`
- Mux status bar SPEC: `doc/tasks/mux-statusbar/SPEC.md`
- WebView implementation: `src/status-bar/`
- Current native-poc widget: `native-poc/src/ui/status_bar.rs`
- Settings shape: `native-poc/src/settings.rs::StatusBarSettings`
- OSC dispatch: `native-poc/src/callbacks.rs::NativeCallbacks::on_osc`
- Mux StatusUpdateMsg handling: `native-poc/src/tabs.rs::Tab::apply_mux_message`
- Project status bar design memory: `project_status_bar_design`
- Project native-poc Go criteria memory: `project_native_poc_goals`
- HTML parser reuse memory: `project_native_html_parser_reuse`
- UI Design Guidelines: `doc/UI-DESIGN-GUIDELINES.yaml`
- Native-poc build location rule: `.claude/rules/native-poc-build-location.md`
