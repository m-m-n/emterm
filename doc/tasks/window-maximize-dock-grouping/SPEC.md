# Feature: Window Maximize-on-Launch and Dock Grouping

## Overview

Child windows (settings, Markdown viewer, JSON/YAML data viewer) launch maximized like the main terminal, and every window (main terminal, settings, Markdown, JSON/YAML, image) is grouped under a single application icon in the Ubuntu (GNOME) dock. The image viewer keeps its existing image-fit sizing and is excluded from maximize-on-launch.

## Objectives

- Launch the settings, Markdown, and JSON/YAML viewer windows maximized by default.
- Group all windows under one dock icon on Linux by unifying the application identifier (X11 `WM_CLASS` / Wayland `app_id`).
- Preserve the image viewer's image-fit auto-sizing (exclude it from maximize only; still include it in dock grouping).

## User Stories

### US1: Maximized child windows
As an eMterm user, I want the settings and viewer windows to open maximized, so that I get a large working area immediately without manual resizing.

**Acceptance Criteria:**
- [ ] The settings window opens maximized.
- [ ] The Markdown viewer window opens maximized.
- [ ] The JSON/YAML data viewer window opens maximized.
- [ ] Un-maximizing restores each window to its current initial size.

### US2: Single dock icon
As an eMterm user on Ubuntu, I want all windows to share one dock icon, so that the application is recognized as a single app.

**Acceptance Criteria:**
- [ ] Main terminal, settings, Markdown, JSON/YAML, and image windows all group under one `emterm` dock icon.
- [ ] The native (winit) viewers no longer appear as a separate dock group.

### US3: Image viewer unchanged sizing
As an eMterm user, I want the image viewer to keep fitting its window to the image, so that a small image does not float inside a huge empty maximized window.

**Acceptance Criteria:**
- [ ] The image viewer is not maximized on launch.
- [ ] The image viewer's existing image-based sizing (capped at 90% of the monitor) is unchanged.
- [ ] The image viewer still groups under the single dock icon.

## Technical Requirements

### Functional Requirements

- **FR1 — Settings window maximize:** The settings window launches in a maximized state. Its current initial size (1080×760) is retained as the un-maximize restore size.
- **FR2 — Markdown viewer maximize:** The Markdown viewer window launches maximized. Its current initial size (960×720) is retained as the restore size.
- **FR3 — Data viewer maximize:** The JSON/YAML data viewer window launches maximized. Its current initial size (960×640) is retained as the restore size.
- **FR4 — Image viewer excluded from maximize:** The image viewer is NOT maximized on launch; its existing image-fit sizing (`image_window.rs`) is unchanged. It is still included in dock grouping (FR5).
- **FR5 — Unified dock grouping:** Every window (main terminal, settings, Markdown, JSON/YAML, image) sets a single canonical application identifier — X11 `WM_CLASS` and Wayland `app_id` equal to `emterm` — matching the existing `emterm.desktop` file name and its `StartupWMClass=emterm`. This makes GNOME/Ubuntu group all windows under one dock icon.

### Non-Functional Requirements

- **NFR1 — Consistency:** Maximize is a fixed behavior with no settings toggle, consistent with the main terminal's `with_maximized(true)`.
- **NFR2 — Platform scope:** Maximize-on-launch applies on both Linux and Windows (settings/Markdown/data viewers exist on both). Dock grouping (FR5) targets Linux X11/Wayland only; Windows taskbar grouping is out of scope.
- **NFR3 — Restore size preserved:** Un-maximizing restores each window to its current initial size; no initial sizes are removed.
- **NFR4 — Single source of identifier:** The canonical identifier (`emterm`) is defined once and referenced by all window-creation sites.

## Implementation Approach

### Current State (as investigated)

| Window | Stack | Creation site | Initial size | `maximized` today | WM_CLASS/app_id today |
|--------|-------|---------------|--------------|-------------------|------------------------|
| Main terminal | winit + wgpu + egui | `window_host.rs:323-329` | 960×600 | `with_maximized(true)` ✓ | none (toolkit default) |
| Settings | GTK/WebKitGTK via `WebViewHost` | `settings_window/mod.rs:84-114` | 1080×760 | none | none |
| Markdown viewer | GTK/WebKitGTK via `WebViewHost` | `viewer/window.rs:71-84` | 960×720 | none | none |
| JSON/YAML data viewer | winit + wgpu + egui | `viewer/data_window.rs:352-357` | 960×640 | none | none |
| Image viewer | winit + wgpu + egui | `viewer/image_window.rs:418-422` | image-fit (≤90% monitor) | none | none |

- `WebViewHost` (`webview_host/mod.rs`) abstracts the Linux (GTK) and Windows (winit + WebView2) child-window runtimes. The Linux path (`webview_host/linux.rs:21-26`) calls `gtk::Window::new` + `set_default_size`; no maximize, no WM_CLASS.
- No window currently sets `WM_CLASS` / `app_id` explicitly. The native winit viewers (image, JSON/YAML) appear as a separate dock group from the GTK-based settings/Markdown windows.
- The canonical app id used for data dirs is `net.laser5.app.emterm` (`settings_core.rs:61`), but the dock-grouping anchor is the desktop file `emterm.desktop` with `StartupWMClass=emterm` (`scripts/build-dpkg.sh:173,183`).

### Maximize-on-launch

1. **`WebViewHost` (settings + Markdown):** add a `maximized: bool` field to the `WebViewHost` config.
   - Linux path (`webview_host/linux.rs`): after creating the GTK window and before/after `show_all`, call `window.maximize()` when `maximized` is true (GTK `GtkWindowExt::maximize`).
   - Windows path (winit + WebView2): apply `.with_maximized(true)` to the window attributes when `maximized` is true.
   - Set `maximized: true` at the settings (`settings_window/mod.rs`) and Markdown (`viewer/window.rs`) creation sites.
2. **JSON/YAML data viewer (`viewer/data_window.rs`):** add `.with_maximized(true)` to the winit `WindowAttributes`. Keep the existing `with_inner_size(960×640)` as the restore size.
3. **Image viewer (`viewer/image_window.rs`):** unchanged — no maximize.

### Dock grouping (unified `WM_CLASS` / `app_id`)

Define a single constant, e.g. `pub const APP_WM_ID: &str = "emterm";`, in a shared module, referenced by every window-creation site.

1. **winit windows (main terminal, image viewer, JSON/YAML data viewer):** set X11 `WM_CLASS` and Wayland `app_id` via winit platform extension traits, gated to Linux:
   - X11: `WindowAttributesExtX11::with_name(APP_WM_ID, APP_WM_ID)` (general/general-class + instance).
   - Wayland: `WindowAttributesExtWayland::with_name(APP_WM_ID, APP_WM_ID)` (sets xdg-shell `app_id`).
   - winit applies the trait matching the active backend; both can be set under `#[cfg(target_os = "linux")]`.
2. **GTK windows (settings + Markdown via `WebViewHost` Linux path):** ensure the same identifier:
   - X11: set `WM_CLASS` (e.g. via `GtkWindowExt::set_wmclass` / equivalent) to `emterm`.
   - Wayland: GTK derives `app_id` from the program name; set it once early (e.g. `glib::set_prgname("emterm")` / `gdk::set_program_class("emterm")`) before window creation so the GTK windows report `app_id = emterm`.
3. The identifier `emterm` matches `emterm.desktop` (file basename) and its `StartupWMClass=emterm`, so GNOME associates every window with the single desktop entry / dock icon on both X11 and Wayland.

### File Structure (touch points)

```
src-tauri/src/
├── webview_host/
│   ├── mod.rs            # add `maximized` field to WebViewHost config; APP_WM_ID reference
│   └── linux.rs          # GTK maximize() + WM_CLASS/app_id (settings, Markdown)
├── settings_window/mod.rs   # set maximized: true
├── viewer/
│   ├── window.rs         # set maximized: true (Markdown)
│   ├── data_window.rs    # with_maximized(true) + winit WM_CLASS/app_id (JSON/YAML)
│   └── image_window.rs   # winit WM_CLASS/app_id only (NOT maximized)
├── window_host.rs        # winit WM_CLASS/app_id (main terminal; already maximized)
└── <shared>              # APP_WM_ID constant (single source)
```

## Test Scenarios

### Unit Tests
- [ ] `WebViewHost` config carries `maximized: true` for settings and Markdown creation sites.
- [ ] The data viewer's winit `WindowAttributes` include `maximized == true`.
- [ ] The image viewer's window attributes do NOT set maximize (image-fit size path unchanged).
- [ ] The canonical identifier constant equals `emterm` and is referenced by each window-creation site (compile-time / value assertion).

### Integration / Manual Verification
- [ ] Launch settings → window is maximized; un-maximize restores ~1080×760.
- [ ] Launch Markdown viewer → maximized; un-maximize restores ~960×720.
- [ ] Launch JSON/YAML viewer → maximized; un-maximize restores ~960×640.
- [ ] Launch image viewer → NOT maximized; sized to the image as before.
- [ ] On Ubuntu (X11 and Wayland): open all window types → exactly one `emterm` dock icon groups them all.

### Edge Cases
- [ ] Small image in image viewer → stays image-sized (not maximized).
- [ ] Wayland session: `app_id` is `emterm` so the window matches `emterm.desktop` and uses the correct icon + grouping.
- [ ] X11 session: `WM_CLASS` instance/class is `emterm` matching `StartupWMClass`.

## Security Considerations

- No new external input, IPC surface, or data handling is introduced. Window attribute changes only.

## Success Criteria

- [ ] All functional requirements (FR1–FR5) are implemented.
- [ ] Settings, Markdown, and JSON/YAML viewers launch maximized; image viewer keeps image-fit sizing.
- [ ] All windows group under one `emterm` dock icon on Ubuntu (X11 and Wayland).
- [ ] No settings toggle is added; behavior is fixed.
- [ ] Build and unit tests pass; no regression to existing window sizing.

## Open Questions

> All clarifications were resolved during spec creation. No `tbd` requirements remain.

## References

- 要件定義書: `doc/tasks/window-maximize-dock-grouping/要件定義書.md`
- Main terminal maximize precedent: `src-tauri/src/window_host.rs:329`
- Desktop entry / StartupWMClass: `scripts/build-dpkg.sh:173-185`
- WebViewHost abstraction: `src-tauri/src/webview_host/mod.rs`
