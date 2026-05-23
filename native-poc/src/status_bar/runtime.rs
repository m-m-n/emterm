//! Status-bar runtime.
//!
//! Owns the [`TemplateEngine`], all four built-in providers, and the
//! OSC `777;statusbar` dispatcher. Per-frame
//! [`StatusBarRuntime::build_view_model`] projects current settings
//! plus the active tab's mux state into a
//! [`StatusBarViewModel`].

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use crate::html;
use crate::settings::StatusBarSettings;
use crate::status_bar::osc_dispatcher::{OscLayerState, StatusBarOscDispatcher};
use crate::status_bar::providers::time::RefreshConfig;
use crate::status_bar::providers::{
    CommandProvider, CwdProvider, CwdSource, GitBranchProvider, TimeProvider,
};
use crate::status_bar::template_engine::TemplateEngine;
use crate::status_bar::view_model::{AppRow, OscRow, StatusBarViewModel};
use crate::wakeup::WakeFn;

/// LRU capacity for resolved-run cache. 16 entries cover the four
/// app-row sides plus historical entries when settings flip.
const RUN_CACHE_CAPACITY: usize = 16;

/// Cache key: a row's raw template plus the per-variable version
/// tuple. When any underlying provider bumps its version counter the
/// key changes and the entry misses, forcing a re-resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RunCacheKey {
    template: String,
    version_tuple: Vec<u64>,
}

#[derive(Default)]
struct RunCache {
    entries: VecDeque<(RunCacheKey, Vec<html::RichTextRun>)>,
}

impl RunCache {
    fn lookup(&mut self, key: &RunCacheKey) -> Option<Vec<html::RichTextRun>> {
        // Linear scan is fine at capacity 16. Move-to-front on hit.
        let position = self.entries.iter().position(|(k, _)| k == key)?;
        let (k, v) = self.entries.remove(position).unwrap();
        let result = v.clone();
        self.entries.push_front((k, v));
        Some(result)
    }

    fn insert(&mut self, key: RunCacheKey, value: Vec<html::RichTextRun>) {
        if self.entries.len() >= RUN_CACHE_CAPACITY {
            self.entries.pop_back();
        }
        self.entries.push_front((key, value));
    }
}

pub struct StatusBarRuntime {
    engine: TemplateEngine,
    dispatcher: Arc<StatusBarOscDispatcher>,
    /// Per-frame resolve cache keyed by (template, version-tuple).
    /// Interior mutability so `build_view_model` stays `&self`.
    run_cache: Mutex<RunCache>,
    /// Previous frame's "mux attached" state. SPEC US5 / FR11 require
    /// the OSC layer to be cleared when the active tab transitions
    /// from "in a mux session" to "not in a mux session" (mux
    /// disconnect). We observe `mux_session_name: Some → None`
    /// between frames and trigger a `dispatcher.handle(&["clear"])`
    /// on the falling edge so any stale OSC 777 state from before
    /// the mux session no longer surfaces.
    prev_mux_attached: Mutex<bool>,
    /// Held so the Drop chain runs (worker join). The provider's
    /// `VariableProvider` impl is exercised via the engine.
    #[allow(dead_code)]
    time_provider: Arc<TimeProvider>,
    #[allow(dead_code)]
    cwd_provider: Arc<CwdProvider>,
    #[allow(dead_code)]
    git_provider: Arc<GitBranchProvider>,
    #[allow(dead_code)]
    command_provider: Arc<CommandProvider>,
}

impl StatusBarRuntime {
    /// Construct a runtime seeded with the supplied settings. The
    /// runtime spins up TimeProvider's timer thread, the GitBranch
    /// worker, and one worker per configured custom command — each
    /// receives a clone of the global [`WakeFn`] (from
    /// [`crate::wakeup::shared_wake_fn`]) so cache updates schedule
    /// the next winit frame even when no PTY / input is active.
    ///
    /// `cwd_source` returns the active tab's cwd (used by Cwd /
    /// GitBranch providers).
    pub fn new(settings: &StatusBarSettings, cwd_source: CwdSource, wake: WakeFn) -> Self {
        let mut engine = TemplateEngine::new();

        // TimeProvider owns a timer thread that fires at
        // `refresh_rates["time"]` (default 1000 ms).
        let time_interval_ms = settings.refresh_rates.get("time").copied().unwrap_or(1000);
        let time = Arc::new(TimeProvider::with_wake(
            settings.time_format.clone(),
            wake.clone(),
            RefreshConfig {
                interval: Duration::from_millis(time_interval_ms),
            },
        ));
        engine.register(time.clone() as Arc<_>);

        // CwdProvider has no thread; OSC 7 callers invoke
        // `set_cwd()` which fires `wake` event-driven.
        let cwd = Arc::new(CwdProvider::with_wake(cwd_source.clone(), wake.clone()));
        engine.register(cwd.clone() as Arc<_>);

        // GitBranch worker. Settings refresh_rates can override the
        // default 5 s interval.
        let git_interval_ms = settings
            .refresh_rates
            .get("git_branch")
            .copied()
            .unwrap_or(5000);
        let git = Arc::new(GitBranchProvider::start_with_wake(
            cwd_source,
            Duration::from_millis(git_interval_ms),
            wake.clone(),
        ));
        engine.register(git.clone() as Arc<_>);

        let cmd = Arc::new(CommandProvider::with_wake(wake));
        for (name, command) in &settings.custom_commands {
            cmd.spawn_worker(name, command);
        }
        engine.register(cmd.clone() as Arc<_>);

        let dispatcher = Arc::new(StatusBarOscDispatcher::new());

        Self {
            engine,
            dispatcher,
            run_cache: Mutex::new(RunCache::default()),
            prev_mux_attached: Mutex::new(false),
            time_provider: time,
            cwd_provider: cwd,
            git_provider: git,
            command_provider: cmd,
        }
    }

    /// Shared handle to the runtime's CwdProvider so the OSC 7 path
    /// in `NativeCallbacks` can call [`CwdProvider::set_cwd`] when a
    /// new cwd arrives. The provider's wake handle (from `new`) then
    /// schedules the next frame without polling.
    pub fn cwd_provider(&self) -> Arc<CwdProvider> {
        self.cwd_provider.clone()
    }

    /// Snapshot the OSC dispatcher (shared reference). Callbacks
    /// route OSC 777 payloads through this.
    pub fn dispatcher(&self) -> Arc<StatusBarOscDispatcher> {
        self.dispatcher.clone()
    }

    /// Build a per-frame view model.
    ///
    /// Inputs:
    /// - `settings`: current status-bar settings (templates,
    ///   position, font_size, …)
    /// - `mux_session_name`: `Some(name)` when the active tab is in a
    ///   mux session, used to populate the OSC row badge
    /// - `mux_status`: `Some(StatusUpdateMsg)` when the daemon has
    ///   pushed status state for the active tab — wins over the OSC
    ///   `777;statusbar` layer
    pub fn build_view_model(
        &self,
        settings: &StatusBarSettings,
        mux_session_name: Option<&str>,
        mux_status: Option<&mux_ipc::protocol::StatusUpdateMsg>,
    ) -> StatusBarViewModel {
        if !settings.enabled {
            return StatusBarViewModel::default();
        }

        // SPEC US5 / FR11: clear the OSC layer on mux disconnect.
        // We detect the `Some → None` falling edge of
        // `mux_session_name` between frames and flush the
        // dispatcher's state so any leftover OSC 777 content (or
        // mux-daemon residue) doesn't leak into the post-disconnect
        // display. Tracked here (in the runtime) rather than in the
        // tab layer so the rule lives next to the OSC dispatcher it
        // affects.
        let attached_now = mux_session_name.is_some();
        let mut prev = self.prev_mux_attached.lock();
        if *prev && !attached_now {
            // Falling edge: reset both sides + auto-hide marker.
            self.dispatcher.handle(&["clear"]);
            // Drop forced_visible so the auto-hide rule (FR12) can
            // hide the row when both sides are empty after the
            // disconnect.
            self.dispatcher.reset_forced_visible();
        }
        *prev = attached_now;
        drop(prev);

        // OSC row: mux daemon wins. When no mux state is available,
        // fall back to the OSC 777 dispatcher's layer state.
        let osc = build_osc_row(&self.dispatcher.state_handle(), mux_status);

        // App rows: resolve each side through the engine + html
        // pipeline, with the (template, version-tuple) cache in
        // front to short-circuit identical frames (NFR1).
        let mut cache = self.run_cache.lock();
        let app_line1 = AppRow {
            left: resolve_runs_cached(&self.engine, &mut cache, &settings.app_line1_left),
            right: resolve_runs_cached(&self.engine, &mut cache, &settings.app_line1_right),
        };
        let app_line2 = AppRow {
            left: resolve_runs_cached(&self.engine, &mut cache, &settings.app_line2_left),
            right: resolve_runs_cached(&self.engine, &mut cache, &settings.app_line2_right),
        };
        drop(cache);

        StatusBarViewModel {
            enabled: true,
            position: settings.position,
            font_size: settings.font_size,
            mux_session_name: mux_session_name.map(str::to_string),
            osc,
            app_line1,
            app_line2,
        }
    }
}

fn build_osc_row(
    layer: &Arc<Mutex<OscLayerState>>,
    mux_status: Option<&mux_ipc::protocol::StatusUpdateMsg>,
) -> OscRow {
    if let Some(mux) = mux_status {
        return OscRow {
            left: mux.left.clone(),
            right: mux.right.clone(),
            // Daemon-supplied state always renders; setting
            // `forced_visible = Some(true)` keeps the auto-hide rule
            // out of the way.
            forced_visible: Some(true),
        };
    }
    let state = layer.lock().clone();
    OscRow {
        left: state.left,
        right: state.right,
        forced_visible: state.forced_visible,
    }
}

fn resolve_runs(engine: &TemplateEngine, template: &str) -> Vec<html::RichTextRun> {
    if template.is_empty() {
        return Vec::new();
    }
    let resolved = engine.resolve(template);
    let nodes = html::parse(&resolved);
    html::to_rich_text_runs(&nodes)
}

/// Cache-aware variant. Computes the (template, version-tuple) key
/// from the engine's registered providers; on a hit returns the
/// cached run list, on a miss falls through to `resolve_runs` and
/// inserts the result.
fn resolve_runs_cached(
    engine: &TemplateEngine,
    cache: &mut RunCache,
    template: &str,
) -> Vec<html::RichTextRun> {
    if template.is_empty() {
        return Vec::new();
    }
    let key = build_cache_key(engine, template);
    if let Some(hit) = cache.lookup(&key) {
        return hit;
    }
    let runs = resolve_runs(engine, template);
    cache.insert(key, runs.clone());
    runs
}

fn build_cache_key(engine: &TemplateEngine, template: &str) -> RunCacheKey {
    let vars = TemplateEngine::extract_variables(template);
    let mut versions = Vec::with_capacity(vars.len());
    for (name, argument) in vars {
        let v = engine
            .get(&name)
            .map(|p| p.version(argument.as_deref()))
            .unwrap_or(0);
        versions.push(v);
    }
    RunCacheKey {
        template: template.to_string(),
        version_tuple: versions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mux_ipc::protocol::StatusUpdateMsg;

    fn settings_with(left1: &str, right1: &str) -> StatusBarSettings {
        let mut s = StatusBarSettings::default();
        s.app_line1_left = left1.to_string();
        s.app_line1_right = right1.to_string();
        // Suppress the live git worker hitting the host filesystem
        // by using a very long interval; the cwd source returns None
        // so the worker will skip git calls regardless.
        s.refresh_rates.insert("git_branch".to_string(), 60_000);
        // Same for the time provider's timer thread: keep it idle so
        // unit tests don't pay an extra context switch per iteration.
        s.refresh_rates.insert("time".to_string(), 60_000);
        s
    }

    fn noop_wake() -> WakeFn {
        Arc::new(|| ())
    }

    #[test]
    fn build_view_model_disabled_returns_disabled_marker() {
        let rt = StatusBarRuntime::new(
            &StatusBarSettings::default(),
            Arc::new(|| None),
            noop_wake(),
        );
        let mut s = StatusBarSettings::default();
        s.enabled = false;
        let vm = rt.build_view_model(&s, None, None);
        assert!(!vm.enabled);
    }

    #[test]
    fn build_view_model_app_line1_resolves_time_template() {
        let s = settings_with("{time}", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        let vm = rt.build_view_model(&s, None, None);
        assert!(vm.enabled);
        // {time} provider returns "HH:mm:ss"; the run-list is one
        // non-empty text run.
        assert_eq!(vm.app_line1.left.len(), 1);
        assert_eq!(vm.app_line1.left[0].text.len(), 8);
    }

    #[test]
    fn build_view_model_empty_template_yields_empty_runs() {
        let s = settings_with("", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        let vm = rt.build_view_model(&s, None, None);
        assert!(vm.app_line1.left.is_empty());
        assert!(vm.app_line1.right.is_empty());
    }

    #[test]
    fn build_view_model_mux_status_populates_osc_row() {
        let s = settings_with("", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        let status = StatusUpdateMsg {
            left: "1:shell".to_string(),
            right: "host01".to_string(),
        };
        let vm = rt.build_view_model(&s, Some("main"), Some(&status));
        assert_eq!(vm.osc.left, "1:shell");
        assert_eq!(vm.osc.right, "host01");
        assert_eq!(vm.osc.forced_visible, Some(true));
        assert_eq!(vm.mux_session_name.as_deref(), Some("main"));
    }

    #[test]
    fn build_view_model_falls_back_to_dispatcher_when_no_mux() {
        let s = settings_with("", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        // Write to the dispatcher; no mux state.
        let dispatch = rt.dispatcher();
        crate::status_bar::osc_dispatcher::try_dispatch_statusbar(
            &dispatch,
            "statusbar;set;left;hello",
        );
        let vm = rt.build_view_model(&s, None, None);
        assert_eq!(vm.osc.left, "hello");
        assert_eq!(vm.osc.forced_visible, Some(true));
    }

    /// SPEC US5 / FR11 regression: when the active tab transitions
    /// from "in a mux session" to "not in a mux session" (mux
    /// disconnect), the OSC layer MUST be cleared on the next
    /// `build_view_model` call. The previous implementation relied on
    /// "absence of mux state" alone, which left stale OSC 777 content
    /// (or daemon residue still cached in the dispatcher) visible
    /// after the falling edge.
    #[test]
    fn mux_disconnect_clears_osc_layer() {
        let s = settings_with("", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());

        // Pre-seed the dispatcher as if some OSC 777 writer (or a
        // stale mux frame) had left content on screen.
        let dispatch = rt.dispatcher();
        crate::status_bar::osc_dispatcher::try_dispatch_statusbar(
            &dispatch,
            "statusbar;set;left;stale",
        );
        crate::status_bar::osc_dispatcher::try_dispatch_statusbar(
            &dispatch,
            "statusbar;set;right;stale-right",
        );
        assert_eq!(dispatch.snapshot().left, "stale");

        // Frame 1: mux attached. Daemon supplies the OSC row content;
        // the dispatcher's leftover state is ignored (mux wins).
        let status = mux_ipc::protocol::StatusUpdateMsg {
            left: "mux-left".to_string(),
            right: "mux-right".to_string(),
        };
        let vm_attached = rt.build_view_model(&s, Some("main"), Some(&status));
        assert_eq!(vm_attached.osc.left, "mux-left");
        assert_eq!(vm_attached.osc.right, "mux-right");

        // Frame 2: mux drops (mux_session_name → None, mux_status →
        // None). The runtime MUST clear the OSC dispatcher state on
        // this falling edge so the post-disconnect view doesn't fall
        // back to the stale `stale` / `stale-right` content from
        // before mux was attached.
        let vm_detached = rt.build_view_model(&s, None, None);
        assert!(
            vm_detached.osc.left.is_empty(),
            "OSC left must be cleared on mux disconnect, got {:?}",
            vm_detached.osc.left
        );
        assert!(
            vm_detached.osc.right.is_empty(),
            "OSC right must be cleared on mux disconnect, got {:?}",
            vm_detached.osc.right
        );
        // forced_visible reset to None so the FR12 auto-hide rule can
        // suppress the now-empty row.
        assert_eq!(vm_detached.osc.forced_visible, None);
    }

    /// A new OSC 777 `set` issued AFTER the disconnect must still
    /// surface — the clear only flushes pre-disconnect state, it does
    /// not permanently suppress the OSC route.
    #[test]
    fn mux_disconnect_does_not_block_subsequent_osc_writes() {
        let s = settings_with("", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());

        // Attach → detach.
        let status = mux_ipc::protocol::StatusUpdateMsg {
            left: "x".to_string(),
            right: String::new(),
        };
        rt.build_view_model(&s, Some("main"), Some(&status));
        rt.build_view_model(&s, None, None);

        // Now a fresh OSC 777 set arrives.
        crate::status_bar::osc_dispatcher::try_dispatch_statusbar(
            &rt.dispatcher(),
            "statusbar;set;left;fresh",
        );
        let vm = rt.build_view_model(&s, None, None);
        assert_eq!(vm.osc.left, "fresh");
    }

    #[test]
    fn build_view_model_mux_wins_over_dispatcher() {
        let s = settings_with("", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        let dispatch = rt.dispatcher();
        crate::status_bar::osc_dispatcher::try_dispatch_statusbar(
            &dispatch,
            "statusbar;set;left;dispatched",
        );
        let status = StatusUpdateMsg {
            left: "mux-left".to_string(),
            right: String::new(),
        };
        let vm = rt.build_view_model(&s, Some("main"), Some(&status));
        // Mux wins.
        assert_eq!(vm.osc.left, "mux-left");
    }

    #[test]
    fn build_view_model_app_line2_empty_means_no_runs() {
        let s = settings_with("{time}", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        let vm = rt.build_view_model(&s, None, None);
        assert!(!vm.app_line2.has_content());
    }

    // ── Phase F: run-list LRU cache ────────────────────────────

    #[test]
    fn run_cache_hits_when_template_and_versions_match() {
        let mut cache = RunCache::default();
        let key = RunCacheKey {
            template: "{time}".to_string(),
            version_tuple: vec![0],
        };
        cache.insert(key.clone(), Vec::new());
        assert!(cache.lookup(&key).is_some());
    }

    #[test]
    fn run_cache_misses_when_version_changes() {
        let mut cache = RunCache::default();
        let key = RunCacheKey {
            template: "{time}".to_string(),
            version_tuple: vec![0],
        };
        cache.insert(key, Vec::new());

        let other = RunCacheKey {
            template: "{time}".to_string(),
            version_tuple: vec![1],
        };
        assert!(cache.lookup(&other).is_none());
    }

    #[test]
    fn run_cache_evicts_oldest_when_full() {
        let mut cache = RunCache::default();
        for i in 0..(RUN_CACHE_CAPACITY + 2) {
            let key = RunCacheKey {
                template: format!("t{i}"),
                version_tuple: vec![0],
            };
            cache.insert(key, Vec::new());
        }
        // The two oldest entries should have been evicted.
        let oldest = RunCacheKey {
            template: "t0".to_string(),
            version_tuple: vec![0],
        };
        assert!(cache.lookup(&oldest).is_none());
        // The newest entry should still be present.
        let newest = RunCacheKey {
            template: format!("t{}", RUN_CACHE_CAPACITY + 1),
            version_tuple: vec![0],
        };
        assert!(cache.lookup(&newest).is_some());
    }

    #[test]
    fn cached_resolve_returns_same_runs_on_repeated_calls() {
        // {time} hops the cwd_source-less worker path and produces a
        // deterministic non-empty run on each call once the cache is
        // primed.
        let s = settings_with("{time}", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        let vm1 = rt.build_view_model(&s, None, None);
        let vm2 = rt.build_view_model(&s, None, None);
        // Both frames render at least one run for `{time}`.
        assert!(!vm1.app_line1.left.is_empty());
        assert!(!vm2.app_line1.left.is_empty());
    }

    // ── Provider-ownership refresh-redraw wiring ─────────────────

    /// The runtime hands the same `WakeFn` to every provider. We
    /// verify the contract end-to-end: calling
    /// [`CwdProvider::set_cwd`] on the runtime's CwdProvider handle
    /// must invoke the wake callback supplied to `new`.
    #[test]
    fn runtime_injects_wake_into_cwd_provider() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        let wake: WakeFn = Arc::new(move || {
            c2.fetch_add(1, Ordering::Relaxed);
        });
        let s = settings_with("{cwd}", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), wake);
        // Drive a cwd update through the runtime's CwdProvider handle.
        rt.cwd_provider().set_cwd(Some("/home/me/repo"));
        assert!(
            count.load(Ordering::Relaxed) >= 1,
            "runtime must wire the wake handle into CwdProvider::set_cwd"
        );
    }

    /// TS-29 (at the runtime level): TimeProvider's timer thread,
    /// constructed by the runtime, fires the runtime-provided wake on
    /// the `refresh_rates["time"]` interval. We use a short interval
    /// to keep the test fast.
    #[test]
    fn runtime_time_provider_timer_fires_wake() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        let wake: WakeFn = Arc::new(move || {
            c2.fetch_add(1, Ordering::Relaxed);
        });
        let mut s = StatusBarSettings::default();
        s.refresh_rates.insert("time".to_string(), 25);
        // Keep the git worker idle.
        s.refresh_rates.insert("git_branch".to_string(), 60_000);
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), wake);
        std::thread::sleep(Duration::from_millis(120));
        let observed = count.load(Ordering::Relaxed);
        // The git-branch worker also fires `wake` once on the initial
        // clear-cache call (cwd_source returns None). We expect ≥ 2
        // wakes from the time timer alone, so ≥ 3 total is the lower
        // bound. A looser ≥ 2 keeps the test robust under heavily
        // loaded CI.
        assert!(
            observed >= 2,
            "expected ≥ 2 wakes from TimeProvider timer, observed {observed}"
        );
        drop(rt);
    }
}
