//! WebView-backed tab bar (hybrid PoC).
//!
//! The hybrid restructure splits the window into two layers:
//!
//! ```text
//! ┌───────────────────────────────┐
//! │   TabBar  (wry WebView)       │   ← this module
//! ├───────────────────────────────┤
//! │   Terminal (wgpu + egui)      │   ← existing native pipeline
//! └───────────────────────────────┘
//! ```
//!
//! The tab strip stops being an egui widget and becomes a real HTML
//! document hosted in a `wry::WebView` mounted as a child of the main
//! winit window via `WebViewBuilder::build_as_child`. The webview occupies
//! the top `TAB_BAR_HEIGHT` logical points of the window; the existing
//! native grid pass (and the egui overlay for cursor / preedit / status
//! bar) continues to render below it.
//!
//! ## IPC contract
//!
//! - **JS → Rust**: the page calls
//!   `window.ipc.postMessage(JSON.stringify({ kind, ... }))` for every
//!   user interaction. Supported kinds:
//!     - `{ "kind": "new" }`           → [`TabEvent::New`]
//!     - `{ "kind": "switch", "i": N }` → [`TabEvent::Switch(N)`]
//!     - `{ "kind": "close",  "i": N }` → [`TabEvent::Close(N)`]
//! - **Rust → JS**: [`TabBarWebView::sync`] calls a global
//!   `window.emtermTabs.update(payload)` function that re-renders the
//!   strip. The payload mirrors [`TabBarItem`] plus the active index.
//!
//! Events are forwarded through a `crossbeam_channel::Sender<TabEvent>`
//! captured by the IPC handler; the main loop drains the receiver in
//! `about_to_wait` (same place we drain PTY / IME).
//!
//! ## Platform notes
//!
//! - On Linux, `wry::WebView::build_as_child` currently requires X11;
//!   the GTK example in upstream `wry` panics on Wayland. The hybrid
//!   PoC inherits the same limitation. Wayland users should run with
//!   `WINIT_UNIX_BACKEND=x11`.
//! - wry hosts the WebKitGTK widget inside the GTK main loop. We
//!   init GTK at startup and pump `gtk::main_iteration_do(false)`
//!   from `about_to_wait` (see `window_host.rs`).

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender};
use serde::Deserialize;
use winit::window::Window;
use wry::{
    dpi::{LogicalPosition, LogicalSize},
    Rect, WebView, WebViewBuilder,
};

use super::tab_bar::TAB_BAR_HEIGHT;
use super::TabEvent;

/// Minimal projection of a [`crate::tabs::Tab`] shipped to the WebView.
///
/// Kept in sync with the JS-side renderer; new fields here MUST be
/// reflected in [`TAB_BAR_HTML`]'s `renderTabs` function.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TabBarPayloadItem {
    pub title: String,
    /// `[mux:name] ` prefix when present (already composed by Rust so
    /// the JS side stays dumb).
    pub mux_session_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TabBarPayload {
    pub items: Vec<TabBarPayloadItem>,
    pub active: usize,
}

/// One in-process WebView hosting the tab bar HTML document.
///
/// Constructed once in `WindowHost::resumed`; owns the receiver end of
/// the IPC channel so the window host can drain `TabEvent`s out of band
/// without locking the WebView.
pub struct TabBarWebView {
    webview: WebView,
    rx: Receiver<TabEvent>,
    /// Last payload we sent — used to skip redundant `evaluate_script`
    /// calls when the tab roster has not changed.
    last_serialized: Option<String>,
}

/// Deserialization shape for IPC messages coming from the WebView.
///
/// `#[serde(tag = "kind", rename_all = "lowercase")]` mirrors the JS
/// `JSON.stringify({ kind: "...", ... })` pattern.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum IpcMessage {
    New,
    Switch { i: usize },
    Close { i: usize },
}

impl TabBarWebView {
    /// Build the WebView and mount it as a child of `window`, occupying
    /// the top `TAB_BAR_HEIGHT` logical points across `window.inner_size().width`.
    ///
    /// Fails if wry cannot create the webview (e.g. missing
    /// WebKitGTK runtime on Linux). The caller falls back to the
    /// egui tab bar in that case.
    pub fn new(window: &Arc<Window>) -> Result<Self, wry::Error> {
        let (tx, rx) = crossbeam_channel::unbounded::<TabEvent>();

        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
        let bounds = Rect {
            position: LogicalPosition::new(0u32, 0u32).into(),
            size: LogicalSize::new(size.width.max(1), TAB_BAR_HEIGHT as u32).into(),
        };

        let ipc_tx = tx;
        let webview = WebViewBuilder::new()
            .with_bounds(bounds)
            .with_transparent(false)
            .with_html(TAB_BAR_HTML)
            .with_ipc_handler(move |req| {
                let body = req.body().as_str();
                match serde_json::from_str::<IpcMessage>(body) {
                    Ok(IpcMessage::New) => {
                        let _ = ipc_tx.send(TabEvent::New);
                    }
                    Ok(IpcMessage::Switch { i }) => {
                        let _ = ipc_tx.send(TabEvent::Switch(i));
                    }
                    Ok(IpcMessage::Close { i }) => {
                        let _ = ipc_tx.send(TabEvent::Close(i));
                    }
                    Err(e) => {
                        log::warn!("tab_bar_webview: malformed IPC message {body:?}: {e}");
                    }
                }
            })
            .build_as_child(window.as_ref())?;

        Ok(Self {
            webview,
            rx,
            last_serialized: None,
        })
    }

    /// Take the next pending [`TabEvent`] queued by the WebView's IPC
    /// handler. Returns `None` when the queue is empty.
    pub fn try_recv(&self) -> Option<TabEvent> {
        self.rx.try_recv().ok()
    }

    /// Push the current tab roster to the WebView. No-op when the
    /// serialized payload matches what was last sent — `evaluate_script`
    /// is cheap but every call wakes the WebKit JS thread.
    pub fn sync(&mut self, payload: &TabBarPayload) {
        let json = match serde_json::to_string(payload) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("tab_bar_webview: failed to serialize payload: {e}");
                return;
            }
        };
        if self.last_serialized.as_deref() == Some(json.as_str()) {
            return;
        }
        // `evaluate_script` returns a `Result` we intentionally ignore:
        // a transient failure (window closing) is non-fatal, and the
        // next sync will pick up the same payload.
        let script = format!("window.emtermTabs && window.emtermTabs.update({json});");
        if let Err(e) = self.webview.evaluate_script(&script) {
            log::warn!("tab_bar_webview: evaluate_script failed: {e}");
            return;
        }
        self.last_serialized = Some(json);
    }

    /// Reposition the webview after a window resize. The tab strip's
    /// height stays fixed; only the width tracks the new client area.
    pub fn set_width(&self, window: &Window) {
        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
        let bounds = Rect {
            position: LogicalPosition::new(0u32, 0u32).into(),
            size: LogicalSize::new(size.width.max(1), TAB_BAR_HEIGHT as u32).into(),
        };
        if let Err(e) = self.webview.set_bounds(bounds) {
            log::warn!("tab_bar_webview: set_bounds failed: {e}");
        }
    }
}

/// The HTML document loaded into the WebView.
///
/// Kept in this Rust file as a single string literal so the PoC stays
/// self-contained (no asset bundling). The styling deliberately mirrors
/// the egui tab bar's visual rhythm: 28 px tall strip, 1 px bottom hairline,
/// active tab is brighter with an accent underline.
const TAB_BAR_HTML: &str = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8" />
<style>
  html, body {
    margin: 0;
    padding: 0;
    height: 100%;
    background: #1a1a1a;
    color: #ddd;
    font-family: system-ui, -apple-system, "Segoe UI", sans-serif;
    font-size: 12px;
    user-select: none;
    overflow: hidden;
  }
  #strip {
    display: flex;
    flex-direction: row;
    align-items: stretch;
    height: 100%;
    border-bottom: 1px solid #2a2a2a;
    box-sizing: border-box;
  }
  .tab {
    display: flex;
    align-items: center;
    flex: 1 1 80px;
    min-width: 80px;
    max-width: 240px;
    padding: 0 6px;
    background: #1a1a1a;
    border-right: 1px solid #2a2a2a;
    cursor: pointer;
    position: relative;
  }
  .tab:hover { background: #232323; }
  .tab.active {
    background: #2d2d2d;
    color: #fff;
    font-weight: 600;
  }
  .tab.active::after {
    content: "";
    position: absolute;
    left: 2px;
    right: 2px;
    bottom: 0;
    height: 2px;
    background: #5aa9ff;
  }
  .label {
    flex: 1 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .close {
    flex: 0 0 auto;
    width: 16px;
    height: 16px;
    line-height: 14px;
    text-align: center;
    margin-left: 6px;
    border-radius: 2px;
    color: #888;
    font-size: 13px;
  }
  .close:hover { background: #444; color: #fff; }
  #plus {
    flex: 0 0 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    color: #ccc;
    font-size: 15px;
  }
  #plus:hover { background: #232323; color: #fff; }
</style>
</head>
<body>
<div id="strip">
  <div id="plus" title="New tab">+</div>
</div>
<script>
  (function () {
    const strip = document.getElementById("strip");
    const plus  = document.getElementById("plus");

    function postIpc(payload) {
      try {
        window.ipc.postMessage(JSON.stringify(payload));
      } catch (e) {
        // wry not injected yet — fall back silently. Should not happen
        // in practice because the WebView IPC bridge is wired before
        // the first user click.
      }
    }

    plus.addEventListener("click", () => postIpc({ kind: "new" }));

    function renderTabs(payload) {
      // Remove any pre-existing .tab nodes (plus button stays at the
      // tail). We rebuild from scratch each call — the tab strip is
      // small enough that diffing is not worth the complexity.
      for (const el of Array.from(strip.querySelectorAll(".tab"))) {
        el.remove();
      }
      const items  = (payload && payload.items)  || [];
      const active = (payload && payload.active) || 0;
      for (let i = 0; i < items.length; i++) {
        const it = items[i];
        const tab = document.createElement("div");
        tab.className = "tab" + (i === active ? " active" : "");
        tab.title = it.title;

        const label = document.createElement("span");
        label.className = "label";
        const prefix = it.mux_session_name
          ? "[mux:" + it.mux_session_name + "] "
          : "";
        label.textContent = prefix + it.title;
        tab.appendChild(label);

        const close = document.createElement("span");
        close.className = "close";
        close.textContent = "×";
        close.addEventListener("click", (ev) => {
          ev.stopPropagation();
          postIpc({ kind: "close", i });
        });
        tab.appendChild(close);

        tab.addEventListener("click", () => postIpc({ kind: "switch", i }));
        strip.insertBefore(tab, plus);
      }
    }

    window.emtermTabs = { update: renderTabs };

    // The first sync() call from Rust runs immediately after the
    // webview is built (see `WindowHost::resumed`), so the page
    // starts empty for a single frame and then gets the real roster.
  })();
</script>
</body>
</html>
"#;
