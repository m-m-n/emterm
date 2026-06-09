# Implementation Plan: Markdown Viewer Port to native-poc (Wry Window)

## Overview

Add a Markdown viewer to native-poc that renders in a Wry (WebView) separate window, reusing the existing WebView Markdown renderer through an embedded standalone bundle. A new viewer subsystem drains the existing OSC queue, accumulates the `emterm;markdown` session, and spawns one window per completed document. The seven `markdown_*` settings are captured and injected into each window.

## Objectives

- Drain the existing OSC queue and accumulate Markdown sessions into complete documents (FR1, FR2).
- Render each completed document in its own Wry window reusing the WebView renderer (FR3, FR4, FR5).
- Capture and apply the seven Markdown settings (FR6).
- Open links safely and resolve inline images (FR7, FR8); provide window controls (FR9).
- Leave `src/` unchanged (NFR3).

## Prerequisites

### Development Environment

- Rust toolchain (native-poc), built from project root with `CARGO_TARGET_DIR` per `.claude/rules/native-poc-build-location.md`.
- Bun (existing) for the new viewer bundle build target.
- Linux desktop with WebKitGTK runtime (Wry backend) for manual verification.

### Dependencies

- `wry` 0.53 — already declared in `native-poc/Cargo.toml`.
- `crates/term_images` — image decode for `basedir`-relative images.
- Existing modules: `native-poc/src/callbacks.rs` (OSC queue), `native-poc/src/links.rs` (`is_safe_uri`, OS open), `native-poc/src/settings.rs`, `native-poc/src/ui/md3.rs`.
- `src/markdown/*` — reused by the viewer bundle (import only; not modified).

## Architecture Overview

### Technology Stack

- **Language**: Rust (native-poc) + TypeScript (viewer bundle).
- **Framework**: winit event loop + Wry WebView (Linux: WebKitGTK).
- **Key Libraries**: wry (WebView window + custom scheme + navigation interception), Bun (bundle build), reused `marked`/`highlight.js`/`mermaid`/`dompurify` via `src/markdown`.

### Design Approach

- **Separate viewer process (Linux WebKitGTK constraint)**: on Linux, Wry renders via WebKitGTK and requires GTK init + a GTK main loop, which does not compose with native-poc's winit event loop (winit was chosen over tao for IME). Therefore each viewer runs as a **separate child process**: the terminal process re-spawns its own binary with a `--viewer` subcommand. The child owns the GTK/Wry window and its own event loop; the terminal's winit loop and `WindowHost` are left unchanged. This isolates the viewer (a viewer crash cannot affect the terminal) and matches "new window per OSC emission".
- **Native accumulates, child renders**: the begin/chunk/end session is reassembled in Rust (ported from `MarkdownSessionManager`) in the terminal process; only the completed document is handed to a freshly spawned child. The child's page reuses the WebView renderer to display it. Memory is bounded by the size cap.
- **Payload transport parent→child**: the parent serializes the render payload (Markdown, format, basedir, resolved appearance) to a temp file and passes its path to the child (`/tmp` per project temp-file conventions). The child reads it on startup.
- **Custom URI scheme inside the child**: the child serves its own window via a registered custom scheme returning (a) the embedded bundle assets (same binary → shared embed) and (b) `basedir`-relative image bytes on demand. This replaces the WebView build's PTY image-request round-trip with direct native resolution in the child.
- **Spawn behind a sink**: Phase 2 emits a "render request" value to an abstract sink so session logic is unit-testable without spawning processes; Phase 4 provides the real sink that spawns child viewer processes.

### Component Interaction

```
term_core OSC parser
  -> NativeCallbacks::on_osc(100, "markdown;…")  [existing]
  -> osc_queue (EmtermOscRequest{payload})        [existing]

ViewerSpawner.drain()                              [new, Phase 2]
  -> parse "<viewer>;<verb>;<k=v>…"
  -> route: markdown -> MarkdownViewerSessions
            image/json/yaml -> reserved no-op + debug log
MarkdownViewerSessions                             [new, Phase 2]
  -> begin/chunk/end accumulation, limits, timeout
  -> on end: emit RenderRequest -> ViewerSink

ViewerSink (real impl, parent)                     [new, Phase 4]
  -> serialize payload to temp file
  -> spawn child: self binary `--viewer <payload-path>`
  -> track child loosely; non-blocking reap

Child viewer process (`--viewer` mode)             [new, Phase 4]
  -> gtk init + Wry WebView window + GTK loop
  -> custom scheme serves embedded bundle + reads payload
  -> navigation interception (Phase 5) -> is_safe_uri -> OS open
  -> custom scheme image resolver (Phase 5) -> term_images
  -> close/Esc/q -> process exit
```

## Implementation Phases

### Phase 1: Markdown Settings Loader

**Goal**: native-poc loads all seven `markdown_*` settings with correct defaults and exposes a resolved-appearance value (theme/preset/fonts/size) honoring `follow_ui`.

**Files to Modify**:
- `native-poc/src/settings.rs` - add seven fields to the resolved `Settings` and the raw deserialize layer; defaults; flat-key merge; a resolver that picks the effective theme/preset source based on `follow_ui`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Markdown settings fields | Hold the 7 values on `Settings` | settings loaded | values available to the viewer subsystem |
| Raw merge | Map flat config keys onto resolved settings | raw config parsed | unknown/null fall back to documented defaults |
| Appearance resolver | Choose theme/preset source per `follow_ui` | settings resolved | returns effective {theme, preset, body/code/emoji font, size} |

**Processing Flow**:
1. Parse config → raw layer.
2. Merge into resolved `Settings` with defaults (fonts empty, size 14, follow_ui true, theme System, preset Purple).
3. Appearance resolver:
   - `follow_ui = true` → use `ui_theme` / `ui_theme_preset`.
   - `follow_ui = false` → use `markdown_theme` / `markdown_theme_preset`.

**Implementation Steps**:
1. **Add resolved fields** - extend `Settings` with the 7 Markdown values (reuse existing `UiTheme` / `UiThemePreset`).
2. **Add raw fields + defaults** - mirror the established settings pattern (flat keys, null-safe defaults).
3. **Merge** - copy raw → resolved with documented fallbacks.
4. **Appearance resolver** - expose the effective appearance honoring `follow_ui`.
5. **Unit tests** - defaults, overrides, and `follow_ui` source selection.

**Dependencies**: None. Blocks Phase 5 (settings injection).

**Testing Approach**:
- Unit: each key parses; defaults correct; `follow_ui` true/false selects the right theme source.

**Acceptance Criteria**:
- [ ] All 7 settings load with correct defaults.
- [ ] Appearance resolver returns the right source per `follow_ui`.

**Estimated Effort**: small

---

### Phase 2: OSC Viewer Dispatch + Markdown Session Accumulation

**Goal**: A viewer subsystem drains the OSC queue, parses payloads, and reassembles Markdown sessions into a complete document, emitting a render request on completion — all unit-testable without windows.

**Files to Create**:
- `native-poc/src/viewer/mod.rs` - replace the stub with `ViewerSpawner`: drain queue, parse `<viewer>;<verb>;<k=v>…`, route by viewer type; reserved branches for image/json/yaml.
- `native-poc/src/viewer/markdown.rs` - replace the stub with `MarkdownViewerSessions`: begin/chunk/end lifecycle, limits, timeout, ordered base64 join; emits a `RenderRequest` to a `ViewerSink` abstraction.

**Files to Modify**:
- `native-poc/src/callbacks.rs` - expose a way to take/drain queued `EmtermOscRequest`s for the spawner (read side); no protocol change.
- `native-poc/src/main.rs` (or module root) - register the new `viewer` module.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| ViewerSpawner | Drain queue, parse payload, route by viewer kind | queue may hold payloads | markdown payloads handed to session manager; others logged |
| Payload parser | Split `<viewer>;<verb>;<k=v>…` into (kind, verb, params) | payload string | structured command or warning on malformed |
| MarkdownViewerSessions | Accumulate begin/chunk/end with limits | parsed markdown commands | on `end`, a complete decoded document |
| ViewerSink (trait) | Receive a RenderRequest | document ready | implementor decides how to display |
| RenderRequest | Carry {markdown, format, basedir} | session completed | consumed by a sink |

**Processing Flow**:
1. Drain queued payloads in arrival order.
2. Parse each: first token = viewer kind, second = verb, rest = key/value.
   - kind = markdown → forward to session manager.
   - kind ∈ {image, json, yaml} → reserved: debug-log and ignore (future feature).
   - unknown kind/verb or missing required key → warn, skip.
3. Session manager by verb:
   - `begin` → create session (enforce max-sessions); record format/version/basedir.
   - `chunk` → store data by `seq` (enforce cumulative size cap).
   - `end` → join chunks in `seq` order, base64-decode to UTF-8, emit RenderRequest.
4. Idle sessions exceeding the timeout are dropped on the next drain pass.

**Implementation Steps**:
1. **Queue read side** - let the spawner consume queued payloads from callbacks state.
2. **Payload parser** - tokenize and validate into (kind, verb, params).
3. **Routing** - markdown vs reserved kinds.
4. **Session manager** - begin/chunk/end with max-sessions, size cap, timeout, ordered join + decode (port limit constants from `MarkdownSessionManager`).
5. **Sink abstraction + RenderRequest** - emit completed documents to a sink (test sink captures them).
6. **Unit/integration tests** - lifecycle, ordering, limits, malformed input, end-to-end dispatch with a capturing sink.

**Dependencies**: Requires callbacks queue (exists). Blocks Phase 4.

**Testing Approach**:
- Unit: parse; ordered join; out-of-order seq; max-sessions reject; size-cap error; timeout drop; missing id; unknown verb; malformed base64.
- Integration: feeding OSC payloads yields exactly one RenderRequest per completed session via a capturing sink.

**Acceptance Criteria**:
- [ ] begin→chunk×N→end yields one complete decoded document.
- [ ] Limits/timeout/malformed inputs behave per SPEC error table; no panics.

**Estimated Effort**: medium

---

### Phase 3: Standalone Viewer Bundle

**Goal**: A dedicated viewer web entry reuses `src/markdown` to render an injected document, built by a new Bun target and embedded into the native-poc binary.

**Files to Create**:
- `native-poc/viewer/web/index.html` - viewer page entry.
- `native-poc/viewer/web/entry.ts` - reads the injected payload (markdown, format, basedir, appearance) and renders via the reused `src/markdown` renderer (themes, syntax highlighting, mermaid, outline, tables, images).
- `native-poc/src/viewer/assets.rs` - access the embedded bundle bytes by path.

**Files to Modify**:
- `package.json` - add a `build:viewer` script that bundles the viewer entry to a fixed output directory (no `src/` change; imports `src/markdown` modules).
- `native-poc/Cargo.toml` / build glue - embed the built bundle directory into the binary (consistent with existing `include_bytes!` asset embedding).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Viewer entry (TS) | Render injected document using reused renderer | payload available to the page | DOM shows the rendered Markdown |
| Build target | Produce a self-contained bundle | Bun present | bundle artifacts at a known path |
| Asset accessor (Rust) | Return embedded bundle bytes for a path | bundle embedded | bytes + content type for the custom scheme |

**Processing Flow**:
1. Build step bundles the viewer entry (with reused `src/markdown`) into static assets.
2. Assets are embedded in the binary.
3. At runtime, the asset accessor returns bytes for a requested in-bundle path.
4. The page, once loaded, obtains the injected payload and renders it.

**Implementation Steps**:
1. **Viewer entry** - HTML + TS that reuses the `src/markdown` renderer and applies appearance (theme/preset/fonts/size) and basedir to the renderer.
2. **Build target** - `build:viewer` producing a fixed-output bundle; verify `src/` untouched.
3. **Embedding** - embed the bundle directory and expose a path→bytes accessor with content types.
4. **Render-on-payload** - the page renders once the payload is present (payload delivery wired in Phase 4).

**Dependencies**: None at runtime. Blocks Phase 4.

**Testing Approach**:
- Unit (TS): reuse existing `src/markdown` tests (no regression); a small entry test that rendering an injected sample produces expected structure.
- Manual: bundle builds; `src/` diff is empty.

**Acceptance Criteria**:
- [ ] `build:viewer` produces an embeddable bundle without modifying `src/`.
- [ ] The page renders an injected sample document with parity features.

**Estimated Effort**: medium

---

### Phase 4: Viewer Process Launch + Child Viewer Window

**Goal**: Completed render requests spawn a separate child viewer process (same binary, `--viewer` mode) that owns a GTK/Wry window serving the embedded bundle and payload; the terminal's winit loop and `WindowHost` are untouched; child processes are independent and close cleanly.

**Files to Create**:
- `native-poc/src/viewer/launch.rs` - parent-side launcher: serialize a RenderRequest payload to a temp file, spawn the self binary with `--viewer <path>`, track the child handle, reap non-blocking.
- `native-poc/src/viewer/window.rs` - child-side viewer: enter GTK/Wry mode, build a Wry WebView window, register the custom scheme (bundle + image bytes), read the payload, render, run the child event loop, exit on close.

**Files to Modify**:
- `native-poc/src/main.rs` - dispatch `--viewer <path>` to the child viewer entry before normal terminal startup; the normal path is unchanged.
- `native-poc/src/viewer/mod.rs` - provide the real `ViewerSink` that calls the launcher; the parent drains the viewer queue on the existing event-loop wakeup.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Real ViewerSink (parent) | Bridge RenderRequest → child process spawn | render request available | one child viewer process per request |
| Payload transport | Serialize {markdown, format, basedir, appearance} to a temp file | render request | child reads the payload by path |
| Child viewer entry | `--viewer` mode: GTK/Wry window + render | payload path passed | window displays the rendered document |
| Custom scheme handler (child) | Map in-bundle paths and image requests to bytes | child window created | page loads assets; images resolved |

**Processing Flow**:
1. Event-loop wakeup (existing proxy mechanism) triggers a parent drain pass.
2. ViewerSpawner produces render requests; the real sink serializes each payload to a temp file and spawns `self --viewer <path>`.
3. The child enters GTK/Wry mode, builds a window, and points the WebView at the custom scheme root.
4. The custom scheme serves bundle assets; the child reads the payload temp file and the page renders it.
5. The child runs its own event loop; close (button) exits the child. The parent's terminal loop is unaffected; the parent reaps exited children non-blocking.

**Implementation Steps**:
1. **`--viewer` dispatch** - route the subcommand to the child entry in `main.rs`.
2. **Child viewer window** - GTK init + Wry WebView window + child event loop.
3. **Custom scheme** - serve embedded bundle assets in the child.
4. **Payload transport** - parent serializes to a temp file; child reads it.
5. **Real sink + launcher** - spawn one child per RenderRequest; track + non-blocking reap.
6. **Manual bring-up** - confirm a Markdown OSC spawns a child that renders on Linux/WebKitGTK.

**Dependencies**: Requires Phase 2 (render requests) and Phase 3 (bundle). Blocks Phase 5.

**Testing Approach**:
- Unit: payload serialize/deserialize round-trip; sink translates a render request into one spawn intent (spawn boundary abstracted for testability).
- Manual: emit a Markdown OSC → a child window renders the document; closing it leaves the terminal and other children intact.

**Acceptance Criteria**:
- [ ] A completed session spawns a child viewer that shows the rendered document.
- [ ] Multiple child viewers coexist; closing one does not affect others or the terminal.

**Estimated Effort**: large

---

### Phase 5: Links, Images, Settings Injection, Window Controls

**Goal**: Links open safely via the OS, inline images resolve (data URI in-page; basedir-relative via native), resolved settings are injected, and `Esc`/`q`/close work.

**Files to Modify**:
- `native-poc/src/viewer/window.rs` (child) - navigation interception; key handling for `Esc`/`q`; custom-scheme image resolver for basedir-relative files; apply payload appearance to the page.
- `native-poc/src/links.rs` - reuse `is_safe_uri` and OS open paths (extend only if needed).

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Navigation interceptor (child) | Decide allow/deny for in-window navigation | link activated | external safe URIs opened via OS; in-window navigation suppressed |
| Image resolver (child) | Serve basedir-relative image bytes safely | page requests an image path | decoded bytes for allowed MIME within basedir; otherwise denied |
| Appearance application | Apply payload appearance to the page | appearance present in payload (Phase 1 resolver + Phase 4 transport) | page styles per settings |
| Window key handling (child) | Close on `Esc`/`q` | window focused | child exits |

**Processing Flow**:
1. Navigation request for an external URI:
   - `is_safe_uri` allows (http/https/mailto/ssh) → open via OS handler; deny in-window navigation.
   - disallowed → deny + warn.
2. Image reference:
   - data URI → handled in-page (DOMPurify/MIME allowlist; SVG excluded — reused renderer behavior).
   - basedir-relative path → custom scheme resolves within `basedir` only (reject traversal), decodes via term_images, returns bytes for allowed MIME; otherwise denied.
3. Appearance from Phase 1 resolver injected into the payload.
4. `Esc` / `q` / close button → destroy window.

**Implementation Steps**:
1. **Navigation interception** - route external links to the OS via `is_safe_uri`; suppress in-window navigation.
2. **Image resolver** - basedir-confined resolution with MIME allowlist and traversal guard.
3. **Appearance application** - apply payload appearance (theme/preset/fonts/size) to the page.
4. **Window controls** - `Esc`/`q`/close exits the child.
5. **Tests** - `is_safe_uri` gating (reuse), basedir traversal rejection, MIME/SVG exclusion.

**Dependencies**: Requires Phase 4 (child window) and Phase 1 (appearance).

**Testing Approach**:
- Unit: `is_safe_uri` allow/deny; basedir traversal rejected; disallowed MIME / SVG excluded.
- Manual: clicking a link opens the browser without navigating the window; an image referenced relative to basedir displays; settings changes reflect in the next window.

**Acceptance Criteria**:
- [ ] External links open via OS; window does not navigate.
- [ ] basedir-relative images display; traversal and disallowed MIME rejected.
- [ ] The 7 settings visibly affect the window; `Esc`/`q`/close work.

**Estimated Effort**: medium-large

---

## Complete File Structure

```
native-poc/
├── src/
│   ├── viewer/
│   │   ├── mod.rs          # ViewerSpawner: drain, parse, route, real sink (P2/P4)
│   │   ├── markdown.rs     # MarkdownViewerSessions: lifecycle, limits (P2)
│   │   ├── launch.rs       # parent: serialize payload, spawn `--viewer` child, reap (P4)
│   │   ├── window.rs       # child: GTK/Wry window, scheme, nav, images, controls (P4/P5)
│   │   └── assets.rs       # embedded viewer bundle accessor (P3)
│   ├── settings.rs         # + 7 markdown_* fields + appearance resolver (P1)
│   ├── callbacks.rs        # expose queue read/drain side (P2)
│   └── main.rs             # `--viewer <path>` dispatch to child entry (P4); register viewer module (P2)
│   └── viewer/web/
│       ├── index.html      # viewer page entry (P3)
│       └── entry.ts        # reuse src/markdown; render injected payload (P3)
package.json                # + build:viewer script (P3)
```

Note: `window_host.rs` is **not** modified — the terminal stays single-window; viewers are separate processes.

## Testing Strategy

- Unit: parser, session lifecycle/limits, settings loader/resolver, registry routing, link/image guards.
- Integration: OSC payloads → RenderRequest via capturing sink (Phase 2).
- E2E: native-poc has no automated GUI E2E framework; viewer behavior is verified manually (run the release binary, emit a Markdown OSC).
- Manual: rendering parity, links, images, settings effect, window lifecycle (Linux/WebKitGTK).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| wry | 0.53 | WebView window, custom URI scheme, navigation interception (already present) |
| term_images | workspace | basedir-relative image decode |
| Bun | existing | build the viewer bundle |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Child GTK/Wry bring-up on Linux (gtk init, WebKitGTK runtime availability) | Medium | High | Minimal `--viewer` child window first in Phase 4; fail with a clear log if WebKitGTK is absent; terminal unaffected |
| `wry` resolved version differs from `Cargo.toml` (`0.53` vs locked `0.45`) | Medium | Low | Align the declared version to what resolves at implementation; pin once verified |
| Payload transport for large Markdown (temp file path passing) | Low | Medium | Size cap from Phase 2 bounds payload; temp file (not args) avoids arg-length limits |
| Orphaned child processes / temp files on abnormal exit | Low | Low | Non-blocking reap in parent; temp files in `/tmp` (cleared on reboot, per project temp-file conventions) |
| Viewer bundle import of `src/markdown` breaks when WebView terminal is later removed | Medium | Medium | Out of scope here; recorded as a follow-up relocation task (NFR3) |
| basedir image path traversal | Low | High | Confine resolution to basedir; reject `..`/absolute escapes; MIME allowlist; SVG excluded |

## Open Questions

- [ ] Custom-scheme concrete layout (scheme name, payload route vs asset routes) — decided during Phase 3/4 implementation; not a requirement blocker.
- [ ] Whether `Esc`/`q` are handled by the page (in-bundle) or the native window layer — decided in Phase 5; both satisfy FR9.

## Success Metrics

- [ ] FR1–FR9 implemented and covered by tests/manual checks.
- [ ] native-poc existing tests do not regress; `src/` unchanged.
- [ ] A Markdown OSC renders a faithful window on Linux.
