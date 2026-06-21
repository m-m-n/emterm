# Implementation Plan: Window Maximize-on-Launch and Dock Grouping

## Overview
Make the settings, Markdown, and JSON/YAML viewer windows launch maximized (image viewer excluded), and unify the X11 `WM_CLASS` / Wayland `app_id` of every window so Ubuntu/GNOME groups them under one dock icon.

## Objectives
- Launch settings / Markdown / JSON-YAML viewer windows maximized while preserving their current initial (restore) sizes.
- Apply one canonical application identifier to all windows so they group under a single dock icon on Linux.
- Leave the image viewer's image-fit sizing untouched while still including it in dock grouping.

## Prerequisites

### Development Environment
- Rust toolchain as pinned by the project; GUI feature (`--features gui`, default-on).
- Linux host with a GTK + WebKitGTK stack (for build) and an Ubuntu/GNOME session (X11 and Wayland) for manual grouping verification.

### Dependencies
- Existing crates only: `winit` 0.30 (x11 + wayland features already enabled), `gtk`/`wry` for the WebView host. No new dependencies.
- Internal components that must exist: `webview_host` (settings + Markdown), `viewer::data_window`, `viewer::image_window`, `window_host` (main terminal).

## Architecture Overview

### Technology Stack
- **Language**: Rust
- **Windowing**: winit 0.30 (main terminal, image viewer, JSON/YAML data viewer); GTK via `WebViewHost` (settings, Markdown viewer)
- **Key mechanisms**: winit window-attribute builder (maximized + platform window-name), GTK window maximize + program identity

### Design Approach
Two independent, additive changes — neither alters existing sizing logic except by adding a maximized startup state:

1. **Maximize-on-launch**: extend the shared `WebViewHost` config with a maximize flag honored by both OS runtimes (GTK and winit+WebView2), and add a maximized startup attribute to the winit-based data viewer. Initial sizes stay as restore sizes. The image viewer is deliberately not touched.
2. **Unified identifier**: define a single canonical window identifier constant (value `emterm`, matching `emterm.desktop` and its `StartupWMClass`). Every window-creation site sets it — winit windows via the platform window-name attribute (X11 `WM_CLASS` / Wayland `app_id`), GTK windows via the program identity used by GTK to derive `WM_CLASS`/`app_id`.

### Component Interaction
- A new shared constant is referenced by `window_host`, `viewer::data_window`, `viewer::image_window`, `settings_window`, and `viewer::window` (the latter two through `WebViewHost`).
- `WebViewHost` gains a maximize flag; its Linux and Windows runtimes each honor it at window-creation time.

## Implementation Phases

### Phase 1: Maximize-on-launch for child windows

**Goal**: Settings, Markdown, and JSON/YAML viewer windows open maximized; image viewer unchanged; restore sizes preserved.

**Files to Modify**:
- `src-tauri/src/webview_host/mod.rs` - add a maximize flag to the `WebViewHost` config (documented default).
- `src-tauri/src/webview_host/linux.rs` - when the flag is set, request the GTK window to start maximized (alongside the existing default-size call, which becomes the restore size).
- `src-tauri/src/webview_host/windows.rs` - when the flag is set, start the winit+WebView2 window maximized.
- `src-tauri/src/settings_window/mod.rs` - set the maximize flag when constructing the host.
- `src-tauri/src/viewer/window.rs` - set the maximize flag when constructing the Markdown host.
- `src-tauri/src/viewer/data_window.rs` - add a maximized startup attribute to the winit window attributes; keep the existing initial-size call as the restore size.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `WebViewHost` config | Carry an opt-in maximize flag | Caller constructs config | Both OS runtimes can read the flag |
| `webview_host::linux::run` | Honor the maximize flag for GTK windows | Flag known | GTK window starts maximized when flagged; default size is the restore size |
| `webview_host::windows::run` | Honor the maximize flag for WebView2 windows | Flag known | Window starts maximized when flagged |
| `viewer::data_window` | Start the data viewer maximized | winit attributes built | Data viewer opens maximized; initial size is the restore size |

**Processing Flow** (diagram-convertible):
1. Caller builds the window config / attributes.
   - settings / Markdown -> set maximize flag = true on `WebViewHost`
   - JSON/YAML data viewer -> set maximized startup attribute = true
   - image viewer -> no maximize (unchanged)
2. OS runtime creates the window.
   - flag set -> create maximized; retain configured size as restore size
   - flag unset -> create at configured size (current behavior)

**Implementation Steps**:
1. **Add maximize flag to `WebViewHost`** - extend the config struct with a documented opt-in maximize flag.
2. **Honor flag in Linux runtime** - request maximize on the GTK window when flagged.
3. **Honor flag in Windows runtime** - start the WebView2 window maximized when flagged.
4. **Flag settings + Markdown hosts** - set the flag at both `WebViewHost` construction sites.
5. **Maximize the data viewer** - add the maximized startup attribute to the data viewer's winit window attributes.
6. **Leave image viewer untouched** - confirm no maximize is applied to the image viewer.

**Dependencies**: Independent of Phase 2.

**Testing Approach**:
- Unit: assert that the settings/Markdown host config produced by their construction sites carries the maximize flag (expose a small config-building helper if needed for testability).
- Manual: open each window; confirm settings/Markdown/JSON/YAML start maximized and restore to ~1080×760 / ~960×720 / ~960×640; confirm image viewer is image-sized, not maximized.

**Acceptance Criteria**:
- [ ] Settings, Markdown, JSON/YAML viewers launch maximized.
- [ ] Un-maximizing restores each window to its current initial size.
- [ ] Image viewer is not maximized; its sizing is unchanged.

**Estimated Effort**: small

---

### Phase 2: Unified dock grouping identifier

**Goal**: All windows (main terminal, settings, Markdown, JSON/YAML, image) share one application identifier so GNOME/Ubuntu groups them under a single dock icon on X11 and Wayland.

**Files to Create / Modify**:
- `src-tauri/src/lib.rs` (or a small dedicated module) - define one canonical identifier constant with value `emterm` (single source of truth, FR5/NFR4).
- `src-tauri/src/window_host.rs` - set the platform window-name (X11 `WM_CLASS` / Wayland `app_id`) on the main terminal window from the constant.
- `src-tauri/src/viewer/data_window.rs` - set the platform window-name from the constant.
- `src-tauri/src/viewer/image_window.rs` - set the platform window-name from the constant (grouping only; no maximize).
- `src-tauri/src/webview_host/linux.rs` - ensure the GTK windows (settings, Markdown) report the same identifier via the program identity GTK uses to derive `WM_CLASS`/`app_id`.

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Canonical identifier constant | Single source for the window identifier | — | Value is `emterm`, matching `emterm.desktop` / `StartupWMClass` |
| winit window-name application | Set X11 `WM_CLASS` / Wayland `app_id` on winit windows | Constant defined | main / data / image windows report `emterm` |
| GTK program identity | Make GTK windows report `emterm` | Constant defined | settings / Markdown windows report `emterm` |

**Processing Flow** (diagram-convertible):
1. Each window-creation site reads the canonical identifier constant.
2. winit windows (main, data, image) apply the platform window-name attribute under Linux.
   - X11 backend -> sets `WM_CLASS` instance/class
   - Wayland backend -> sets `app_id`
3. GTK windows (settings, Markdown) set the program identity so GTK derives the same `WM_CLASS`/`app_id`.
4. GNOME/Ubuntu matches every window to `emterm.desktop` and groups them under one dock icon.

**Implementation Steps**:
1. **Define canonical identifier constant** - one GUI-shared constant valued `emterm`.
2. **Apply to winit windows** - set the platform window-name attribute on main terminal, data viewer, and image viewer windows (Linux-gated).
3. **Apply to GTK windows** - set the program identity in the Linux `WebViewHost` runtime so settings + Markdown report the same identifier.
4. **Confirm `.desktop` consistency** - verify `emterm.desktop` / `StartupWMClass=emterm` already match the constant (no packaging change expected).

**Dependencies**: Independent of Phase 1; touches some of the same files (data viewer).

**Testing Approach**:
- Unit: assert the canonical identifier constant equals `emterm`.
- Manual: on Ubuntu X11 and Wayland, open all window types; confirm exactly one `emterm` dock icon groups them; confirm the correct icon shows on Wayland (app_id match).

**Acceptance Criteria**:
- [ ] One canonical identifier constant exists and is referenced by all window-creation sites.
- [ ] All windows group under a single `emterm` dock icon on X11 and Wayland.
- [ ] The native (winit) viewers no longer appear as a separate dock group.

**Estimated Effort**: small

---

## Complete File Structure

```
src-tauri/src/
├── lib.rs                       # (or dedicated module) canonical identifier constant
├── window_host.rs               # main terminal: + platform window-name
├── settings_window/mod.rs       # set maximize flag on WebViewHost
├── webview_host/
│   ├── mod.rs                   # + maximize flag in WebViewHost config
│   ├── linux.rs                 # GTK maximize + program identity (settings, Markdown)
│   └── windows.rs               # WebView2 window maximized when flagged
└── viewer/
    ├── window.rs                # Markdown: set maximize flag on WebViewHost
    ├── data_window.rs           # JSON/YAML: maximized attribute + platform window-name
    └── image_window.rs          # Image: platform window-name only (NOT maximized)
```

## Testing Strategy
- Unit: cover the deterministic, host-level facts (maximize flag carried by config, canonical identifier value). Window-manager-level behavior (actual maximize state, dock grouping) is not reliably unit-testable and is covered by manual verification.
- Manual: visual confirmation of maximize state, restore sizes, image-fit sizing, and single-icon dock grouping on both X11 and Wayland.

## Dependencies
| Package | Version | Purpose |
|---------|---------|---------|
| winit | 0.30 (existing) | winit window attributes (maximized + platform window-name) |
| gtk / wry | existing | GTK window maximize + program identity |

## Risk Assessment
| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Default `WM_CLASS`/`app_id` differs by winit backend (X11 vs Wayland) | Medium | Medium | Set the identifier explicitly on every window so grouping is deterministic regardless of backend defaults |
| GTK windows derive identity from program name, not a per-window call | Medium | Low | Set the program identity early in the Linux runtime before window creation |
| Maximizing changes perceived layout for very small content | Low | Low | Image viewer is excluded by design; other viewers benefit from larger area |
| Window-level behavior not unit-testable | High | Low | Rely on documented manual verification steps; unit-test only deterministic facts |

## Open Questions
- [ ] None — all clarifications resolved during spec creation.

## Success Metrics
- [ ] FR1–FR5 implemented; image viewer sizing unchanged.
- [ ] Build and unit tests pass.
- [ ] Manual checklist (maximize + single dock icon on X11/Wayland) passes.
