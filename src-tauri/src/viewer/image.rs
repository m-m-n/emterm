//! Parent-side image-viewer router.
//!
//! Consumes the `ImageEvent` stream that `Tab::pump` decodes from Kitty
//! APC / SIXEL DCS payloads (`emterm image`, or any program speaking those
//! protocols) and drives the native image-viewer child windows. Mirrors
//! the WebView build's `ImageHandler` (`src/terminal-app/handlers/image.ts`):
//!
//! - `ImageReady` stores the decoded RGBA, keyed by image id, in an
//!   LRU-quota store (`pendingImages` + the backend 320 MB quota).
//! - `Place` looks the image up and shows it — here by handing the RGBA
//!   to a **spawn worker thread** that serializes it to a temp payload
//!   file and spawns `self --image-viewer <path>`. The disk write of a
//!   potentially tens-of-MB payload and the process spawn both happen
//!   OFF the terminal pump thread, so a large image (or a burst of
//!   `Place` events) never stalls PTY processing. (The WebView build
//!   re-shows a single overlay; child windows behave like the Markdown
//!   viewer instead, so several can coexist.)
//! - `Delete` drops stored bytes (`ById`) or clears the store (`All`);
//!   already-open windows are the user's to close, matching the
//!   Markdown-viewer lifecycle.
//! - `Animation` is logged and ignored: the viewer shows static images
//!   (the `emterm image` CLI never emits animation frames).

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{RecvTimeoutError, SyncSender, TrySendError};
use std::time::Duration;

use term_images::image_proc::{DecodedImage, ImageDelete, ImageEvent};

use super::image_payload::{ViewerChrome, write_image_payload};
use super::launch::{preset_token, theme_token};
use crate::settings::Settings;

/// Hard cap on concurrently tracked viewer children, so a hostile output
/// stream replaying `Place` cannot open unbounded windows.
const MAX_VIEWER_CHILDREN: usize = 8;

/// Hard cap on the number of stored decoded images. The byte quota alone
/// does not bound a flood of tiny images (per-entry map/queue overhead is
/// outside `rgba_data.len()`), so the count is capped independently;
/// excess evicts from the LRU front like the byte quota does.
const MAX_IMAGE_COUNT: usize = 1024;

/// Spawn-worker queue depth. A full queue drops the `Place` (with a
/// warning) instead of blocking the pump thread — by then there are
/// already several viewer spawns in flight plus up to
/// [`MAX_VIEWER_CHILDREN`] windows, so dropping is the bounded-loss
/// option.
const SPAWN_QUEUE_DEPTH: usize = 4;

/// How often the spawn worker wakes to reap exited children while idle.
/// Keeps closed viewer windows from lingering as zombies until the next
/// image arrives.
const IDLE_REAP_INTERVAL: Duration = Duration::from_secs(5);

/// LRU + quota store for decoded images awaiting a `Place`. Evicts from
/// the LRU front when EITHER the byte quota or the image-count cap is
/// exceeded.
struct ImageStore {
    images: HashMap<u32, DecodedImage>,
    /// Front = next eviction victim, back = most recently used.
    lru: VecDeque<u32>,
    memory_used: u64,
    quota_bytes: u64,
}

impl ImageStore {
    fn new(quota_bytes: u64) -> Self {
        Self {
            images: HashMap::new(),
            lru: VecDeque::new(),
            memory_used: 0,
            quota_bytes,
        }
    }

    fn record(&mut self, image: DecodedImage) {
        let id = image.id;
        let byte_size = image.rgba_data.len() as u64;
        if let Some(prev) = self.images.insert(id, image) {
            self.memory_used = self.memory_used.saturating_sub(prev.rgba_data.len() as u64);
            if let Some(pos) = self.lru.iter().position(|i| *i == id) {
                self.lru.remove(pos);
            }
        }
        self.memory_used = self.memory_used.saturating_add(byte_size);
        self.lru.push_back(id);
        self.evict_over_limits();
    }

    fn touch(&mut self, id: u32) {
        if let Some(pos) = self.lru.iter().position(|i| *i == id) {
            self.lru.remove(pos);
            self.lru.push_back(id);
        }
    }

    fn remove(&mut self, id: u32) {
        if let Some(prev) = self.images.remove(&id) {
            self.memory_used = self.memory_used.saturating_sub(prev.rgba_data.len() as u64);
            if let Some(pos) = self.lru.iter().position(|i| *i == id) {
                self.lru.remove(pos);
            }
        }
    }

    fn clear(&mut self) {
        self.images.clear();
        self.lru.clear();
        self.memory_used = 0;
    }

    /// Evict LRU-front images until both the byte quota and the count
    /// cap are satisfied.
    fn evict_over_limits(&mut self) {
        while self.memory_used > self.quota_bytes || self.images.len() > MAX_IMAGE_COUNT {
            let Some(victim) = self.lru.pop_front() else {
                break;
            };
            if let Some(prev) = self.images.remove(&victim) {
                self.memory_used = self.memory_used.saturating_sub(prev.rgba_data.len() as u64);
                log::warn!(
                    "image store: evicted image id={} ({} bytes, now {} images / {}/{} bytes)",
                    victim,
                    prev.rgba_data.len(),
                    self.images.len(),
                    self.memory_used,
                    self.quota_bytes
                );
            }
        }
    }
}

/// One queued viewer-spawn request, handed from the pump thread to the
/// worker. The RGBA is a clone of the stored image (the store keeps its
/// copy for future re-`Place`s); the memcpy is microseconds-scale where
/// the disk write it unblocks is milliseconds-to-seconds-scale.
struct SpawnJob {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Handle to the lazily-started spawn worker thread.
struct SpawnWorker {
    tx: SyncSender<SpawnJob>,
}

impl SpawnWorker {
    /// Start the worker. `current_exe` is resolved once here, not per
    /// `Place`. The thread owns the child table: it reaps exited viewers
    /// on every job and on an idle tick ([`IDLE_REAP_INTERVAL`]), so
    /// closed windows are collected even when no further images arrive.
    fn start(chrome: ViewerChrome) -> std::io::Result<Self> {
        // Resolve once at worker-thread start (not per `Place`), preserving
        // the existing timing. Routed through `self_exec` for parity with the
        // other spawn sites; the resolver is `current_exe()` fresh.
        let exe = crate::self_exec::self_exe_path()?;
        let (tx, rx) = std::sync::mpsc::sync_channel::<SpawnJob>(SPAWN_QUEUE_DEPTH);
        std::thread::Builder::new()
            .name("image-viewer-spawn".to_string())
            .spawn(move || {
                // Each child is tracked TOGETHER with its payload path so
                // the parent owns the temp file's lifetime: the child
                // unlinks on a successful read, but if it exits without
                // ever reading (panic before read, GPU-less host, user
                // kill), `reap` removes the file. Without this, a child
                // that dies pre-read would leak a tens-of-MB payload
                // until reboot.
                let mut children: Vec<(std::process::Child, std::path::PathBuf)> = Vec::new();
                loop {
                    match rx.recv_timeout(IDLE_REAP_INTERVAL) {
                        Ok(job) => {
                            reap(&mut children);
                            if children.len() >= MAX_VIEWER_CHILDREN {
                                log::warn!(
                                    "image viewer: {MAX_VIEWER_CHILDREN} children already open; skipping new window"
                                );
                                continue;
                            }
                            match spawn_viewer_child(&exe, &job, &chrome) {
                                Ok(entry) => children.push(entry),
                                Err(e) => {
                                    crate::self_exec::note_spawn_failure();
                                    log::warn!(
                                        "image viewer: failed to spawn child ({e}); terminal unaffected"
                                    );
                                }
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => reap(&mut children),
                        // Router dropped → terminal is shutting down. Open
                        // viewers stay alive as independent processes (and
                        // unlink their own payloads after reading).
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?;
        Ok(Self { tx })
    }
}

/// Non-blocking reap of exited viewer children (same discipline as
/// `ProcessViewerSink::reap`), plus best-effort removal of each exited
/// child's payload file. A child that read successfully already unlinked
/// it (the remove is then a no-op); a child that died before reading
/// would otherwise leak the file.
fn reap(children: &mut Vec<(std::process::Child, std::path::PathBuf)>) {
    children.retain_mut(|(child, payload)| match child.try_wait() {
        Ok(Some(_status)) => {
            let _ = std::fs::remove_file(payload);
            false
        }
        Ok(None) => true,
        Err(e) => {
            log::warn!("image viewer: try_wait failed for child: {e}");
            let _ = std::fs::remove_file(payload);
            false
        }
    });
}

/// Serialize one image to a temp payload file and spawn the child
/// window. Runs ON THE WORKER THREAD only. On spawn failure the
/// just-written payload is removed before the error propagates — nothing
/// will ever read it.
fn spawn_viewer_child(
    exe: &std::path::Path,
    job: &SpawnJob,
    chrome: &ViewerChrome,
) -> std::io::Result<(std::process::Child, std::path::PathBuf)> {
    let path = write_image_payload(job.width, job.height, &job.rgba, chrome)?;
    let child = match std::process::Command::new(exe)
        .arg("--image-viewer")
        .arg(&path)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    };
    log::warn!(
        "image viewer: spawned child pid={} ({}x{}, payload={})",
        child.id(),
        job.width,
        job.height,
        path.display()
    );
    Ok((child, path))
}

/// Routes decoded image events to native viewer child windows.
pub struct ImageViewerRouter {
    store: ImageStore,
    /// Parent-resolved chrome appearance, embedded in every payload so
    /// the child never re-reads `settings.json`.
    chrome: ViewerChrome,
    /// Lazily started on the first `Place` so constructing an `App`
    /// (e.g. in unit tests) never spins up a thread.
    worker: Option<SpawnWorker>,
}

impl std::fmt::Debug for ImageViewerRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageViewerRouter")
            .field("images", &self.store.images.len())
            .field("worker_started", &self.worker.is_some())
            .finish()
    }
}

impl ImageViewerRouter {
    /// Derives the byte quota (`image_memory_quota_mb`) and the chrome
    /// appearance tokens from the parent's resolved settings.
    pub fn new(settings: &Settings) -> Self {
        let quota_bytes = (settings.image_memory_quota_mb as u64) * 1024 * 1024;
        let chrome = ViewerChrome {
            theme: theme_token(settings.ui_theme).to_string(),
            preset: preset_token(settings.ui_theme_preset).to_string(),
            ui_font_family: settings.ui_font_family.clone(),
            terminal_font_family: settings
                .font_family_fallback
                .first()
                .cloned()
                .unwrap_or_default(),
        };
        Self {
            store: ImageStore::new(quota_bytes),
            chrome,
            worker: None,
        }
    }

    /// Production entry: handle a batch of decoded events. Each `Place`
    /// enqueues a spawn job to the worker thread; the pump thread never
    /// blocks on disk I/O or process spawn. A full queue drops the
    /// `Place` with a warning (bounded loss under a hostile burst).
    pub fn handle_events(&mut self, events: Vec<ImageEvent>) {
        let chrome = self.chrome.clone();
        let worker = &mut self.worker;
        let mut show = |image: &DecodedImage| {
            let w = match worker {
                Some(w) => w,
                None => match SpawnWorker::start(chrome.clone()) {
                    Ok(w) => worker.insert(w),
                    Err(e) => {
                        log::warn!("image viewer: failed to start spawn worker ({e})");
                        return;
                    }
                },
            };
            let job = SpawnJob {
                width: image.width,
                height: image.height,
                rgba: image.rgba_data.clone(),
            };
            match w.tx.try_send(job) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    log::warn!("image viewer: spawn queue full; dropping Place");
                }
                Err(TrySendError::Disconnected(_)) => {
                    log::warn!("image viewer: spawn worker exited; dropping Place");
                    *worker = None;
                }
            }
        };
        Self::route(&mut self.store, events, &mut show);
    }

    /// Spawn-agnostic core, unit-testable without launching processes.
    fn route<F>(store: &mut ImageStore, events: Vec<ImageEvent>, show: &mut F)
    where
        F: FnMut(&DecodedImage),
    {
        for evt in events {
            match evt {
                ImageEvent::ImageReady { image } => store.record(image),
                ImageEvent::Place { placement } => {
                    let id = placement.image_id;
                    match store.images.get(&id) {
                        Some(image) => {
                            show(image);
                            store.touch(id);
                        }
                        // WebView parity: warn when a Place references an
                        // unknown (or already-evicted) image.
                        None => log::warn!("image viewer: Place for unknown image id={id}"),
                    }
                }
                ImageEvent::Delete { target } => match target {
                    // WebView parity: `Delete All` clears the pending
                    // store; open viewer windows stay (they are separate
                    // processes the user closes, like Markdown viewers).
                    ImageDelete::All | ImageDelete::AllIncludingHidden => store.clear(),
                    ImageDelete::ById(id) => store.remove(id),
                    // Placement-scoped deletes have nothing to address
                    // here — there is no inline placement surface.
                    ImageDelete::ByPlacement { .. }
                    | ImageDelete::AtCursor { .. }
                    | ImageDelete::ByZIndex(_) => {}
                },
                ImageEvent::Animation(_) => {
                    log::debug!("image viewer: animation event ignored (static viewer)");
                }
                ImageEvent::QueryResponse { supported } => {
                    log::debug!("image viewer: graphics query (supported={supported})");
                }
                ImageEvent::Response { .. } => {
                    // Tab::drain_and_decode_images splits these off and
                    // writes them back to the PTY before we ever run.
                    debug_assert!(false, "Response events must be split off before routing");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use term_images::image_proc::ImagePlacement;

    fn decoded(id: u32, byte_size: usize) -> DecodedImage {
        DecodedImage {
            id,
            width: 4,
            height: (byte_size / 16) as u32,
            rgba_data: vec![0u8; byte_size],
            rgba_base64: String::new(),
        }
    }

    fn ready(id: u32, byte_size: usize) -> ImageEvent {
        ImageEvent::ImageReady {
            image: decoded(id, byte_size),
        }
    }

    fn place(image_id: u32) -> ImageEvent {
        ImageEvent::Place {
            placement: ImagePlacement {
                image_id,
                ..ImagePlacement::default()
            },
        }
    }

    fn route_capture(store: &mut ImageStore, events: Vec<ImageEvent>) -> Vec<u32> {
        let mut shown = Vec::new();
        ImageViewerRouter::route(store, events, &mut |img: &DecodedImage| shown.push(img.id));
        shown
    }

    #[test]
    fn place_shows_stored_image() {
        let mut store = ImageStore::new(1024);
        let shown = route_capture(&mut store, vec![ready(1, 100), place(1)]);
        assert_eq!(shown, vec![1]);
    }

    #[test]
    fn place_for_unknown_image_shows_nothing() {
        let mut store = ImageStore::new(1024);
        let shown = route_capture(&mut store, vec![place(42)]);
        assert!(shown.is_empty());
    }

    #[test]
    fn image_persists_for_repeated_place() {
        let mut store = ImageStore::new(1024);
        let shown = route_capture(&mut store, vec![ready(1, 100), place(1), place(1)]);
        assert_eq!(shown, vec![1, 1]);
    }

    #[test]
    fn delete_by_id_drops_stored_bytes() {
        let mut store = ImageStore::new(1024);
        let shown = route_capture(
            &mut store,
            vec![
                ready(1, 100),
                ImageEvent::Delete {
                    target: ImageDelete::ById(1),
                },
                place(1),
            ],
        );
        assert!(shown.is_empty());
        assert_eq!(store.memory_used, 0);
    }

    #[test]
    fn delete_all_clears_store() {
        let mut store = ImageStore::new(1024);
        let shown = route_capture(
            &mut store,
            vec![
                ready(1, 100),
                ready(2, 100),
                ImageEvent::Delete {
                    target: ImageDelete::All,
                },
                place(1),
                place(2),
            ],
        );
        assert!(shown.is_empty());
        assert_eq!(store.memory_used, 0);
    }

    #[test]
    fn store_evicts_lru_over_byte_quota() {
        let mut store = ImageStore::new(300);
        let shown = route_capture(
            &mut store,
            vec![
                ready(1, 100),
                ready(2, 100),
                ready(3, 100),
                ready(4, 100), // pushes total to 400 → evicts 1
                place(1),
                place(4),
            ],
        );
        assert_eq!(shown, vec![4]);
        assert_eq!(store.memory_used, 300);
    }

    #[test]
    fn store_evicts_lru_over_image_count_cap() {
        // Tiny images stay far below the byte quota, but the count cap
        // still bounds the entry overhead.
        let mut store = ImageStore::new(u64::MAX);
        for id in 0..(MAX_IMAGE_COUNT as u32 + 10) {
            store.record(decoded(id, 16));
        }
        assert_eq!(store.images.len(), MAX_IMAGE_COUNT);
        assert_eq!(store.lru.len(), MAX_IMAGE_COUNT);
        // The first 10 ids were evicted from the LRU front.
        assert!(!store.images.contains_key(&0));
        assert!(!store.images.contains_key(&9));
        assert!(store.images.contains_key(&10));
    }

    #[test]
    fn place_touches_lru_order() {
        let mut store = ImageStore::new(300);
        let shown = route_capture(
            &mut store,
            vec![
                ready(1, 100),
                ready(2, 100),
                ready(3, 100),
                place(1),      // 1 becomes MRU
                ready(4, 100), // evicts 2 (LRU front after the touch)
                place(1),
                place(2),
            ],
        );
        assert_eq!(shown, vec![1, 1]);
    }

    #[test]
    fn record_same_id_replaces_bytes_without_leak() {
        let mut store = ImageStore::new(1024);
        store.record(decoded(1, 100));
        store.record(decoded(1, 256));
        assert_eq!(store.memory_used, 256);
        assert_eq!(store.images.len(), 1);
        assert_eq!(store.lru.len(), 1);
    }

    #[test]
    fn animation_and_query_events_are_ignored() {
        let mut store = ImageStore::new(1024);
        let shown = route_capture(
            &mut store,
            vec![ImageEvent::QueryResponse { supported: true }],
        );
        assert!(shown.is_empty());
    }

    #[test]
    fn router_new_derives_chrome_tokens_from_settings() {
        let mut settings = Settings::default();
        settings.ui_theme = crate::settings::UiTheme::Light;
        settings.ui_theme_preset = crate::settings::UiThemePreset::Pink;
        settings.ui_font_family = "Noto Sans JP".to_string();
        let router = ImageViewerRouter::new(&settings);
        assert_eq!(router.chrome.theme, "light");
        assert_eq!(router.chrome.preset, "pink");
        assert_eq!(router.chrome.ui_font_family, "Noto Sans JP");
        assert!(router.worker.is_none(), "worker must start lazily");
    }
}
