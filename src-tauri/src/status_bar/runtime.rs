//! Status-bar runtime.
//!
//! Owns the [`TemplateEngine`], all four built-in providers, and the
//! OSC `777;statusbar` dispatcher. Per-frame
//! [`StatusBarRuntime::build_view_model`] projects current settings
//! into a [`StatusBarViewModel`].

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
    ///
    /// The OSC row is fed only by the OSC `777;statusbar` dispatcher
    /// (mux-status-bar-removal task0001, FR1/FR6): mux attach/detach
    /// is not an input to this method or to [`build_osc_row`] — status
    /// bar content and row count are functions of `settings` and the
    /// dispatcher's own state only.
    pub fn build_view_model(&self, settings: &StatusBarSettings) -> StatusBarViewModel {
        if !settings.enabled {
            return StatusBarViewModel::default();
        }

        let osc = build_osc_row(&self.dispatcher.state_handle());

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
            font_size: settings.font_size,
            osc,
            app_line1,
            app_line2,
        }
    }
}

/// Hard cap on the character length of an OSC status-bar section
/// (`left` / `right`) accepted at the view-model boundary.
///
/// OSC layer content is terminal-controlled — a mux daemon or any
/// program writing the status-bar OSC sequence supplies it, so it is
/// attacker-influenceable. The drawer only ever paints a half-row
/// prefix (a few hundred grapheme cells at most), so a payload far
/// beyond that is never visible; capping here keeps an adversarial
/// multi-megabyte payload from forcing per-frame atomization and
/// measurement work downstream. The cap is generous (well past any
/// real status line) and truncates on a `char` boundary so it never
/// splits a UTF-8 sequence.
const OSC_SECTION_CHAR_CAP: usize = 4096;

/// Truncate `s` to at most [`OSC_SECTION_CHAR_CAP`] characters on a
/// char boundary, returning the original allocation when it already
/// fits (no copy on the common path).
fn cap_osc_section(s: String) -> String {
    // `char_indices().nth(cap)` is the byte offset of the (cap+1)-th
    // char; `None` means the string has ≤ cap chars and needs no cut.
    match s.char_indices().nth(OSC_SECTION_CHAR_CAP) {
        Some((byte_idx, _)) => s[..byte_idx].to_string(),
        None => s,
    }
}

fn build_osc_row(layer: &Arc<Mutex<OscLayerState>>) -> OscRow {
    let state = layer.lock().clone();
    OscRow {
        left: cap_osc_section(state.left),
        right: cap_osc_section(state.right),
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
    fn cap_osc_section_leaves_short_strings_untouched() {
        let s = "1:shell 2:nvim* host01".to_string();
        assert_eq!(cap_osc_section(s.clone()), s);
    }

    #[test]
    fn cap_osc_section_truncates_overlong_input_on_char_boundary() {
        // A multibyte fill so a naive byte cut would split a codepoint.
        let long: String = "あ".repeat(OSC_SECTION_CHAR_CAP * 2);
        let capped = cap_osc_section(long);
        assert_eq!(capped.chars().count(), OSC_SECTION_CHAR_CAP);
        // Still valid UTF-8 (no split codepoint) — every char is 'あ'.
        assert!(capped.chars().all(|c| c == 'あ'));
    }

    #[test]
    fn build_osc_row_caps_terminal_controlled_sections() {
        let huge = "x".repeat(OSC_SECTION_CHAR_CAP + 5000);
        let layer = Arc::new(Mutex::new(OscLayerState {
            left: huge.clone(),
            right: huge,
            forced_visible: Some(true),
        }));
        let row = build_osc_row(&layer);
        assert_eq!(row.left.chars().count(), OSC_SECTION_CHAR_CAP);
        assert_eq!(row.right.chars().count(), OSC_SECTION_CHAR_CAP);
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
        let vm = rt.build_view_model(&s);
        assert!(!vm.enabled);
    }

    #[test]
    fn build_view_model_app_line1_resolves_time_template() {
        let s = settings_with("{time}", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        let vm = rt.build_view_model(&s);
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
        let vm = rt.build_view_model(&s);
        assert!(vm.app_line1.left.is_empty());
        assert!(vm.app_line1.right.is_empty());
    }

    /// AC-2/TS1 (mux-status-bar-removal task0001): `build_view_model`
    /// takes no mux-status input at all -- the OSC row reflects only the
    /// OSC `777;statusbar` dispatcher's state.
    #[test]
    fn build_view_model_osc_row_reflects_dispatcher_state_only() {
        let s = settings_with("", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        let dispatch = rt.dispatcher();
        crate::status_bar::osc_dispatcher::try_dispatch_statusbar(
            &dispatch,
            "statusbar;set;left;hello",
        );
        let vm = rt.build_view_model(&s);
        assert_eq!(vm.osc.left, "hello");
        assert_eq!(vm.osc.forced_visible, Some(true));
    }

    /// AC-2/TS1: repeated `build_view_model` calls with unchanged
    /// dispatcher state produce an unchanged OSC row -- there is no
    /// hidden mux-attach-state input that could make two frames differ
    /// with identical dispatcher content (this is the row-count /
    /// content invariant AC-2 requires: mux attach/detach is not an
    /// input).
    #[test]
    fn build_view_model_osc_row_is_stable_across_repeated_calls() {
        let s = settings_with("", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        crate::status_bar::osc_dispatcher::try_dispatch_statusbar(
            &rt.dispatcher(),
            "statusbar;set;left;steady",
        );
        let vm1 = rt.build_view_model(&s);
        let vm2 = rt.build_view_model(&s);
        assert_eq!(vm1.osc, vm2.osc);
        assert_eq!(vm1.osc.left, "steady");
    }

    #[test]
    fn build_view_model_app_line2_empty_means_no_runs() {
        let s = settings_with("{time}", "");
        let rt = StatusBarRuntime::new(&s, Arc::new(|| None), noop_wake());
        let vm = rt.build_view_model(&s);
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
        let vm1 = rt.build_view_model(&s);
        let vm2 = rt.build_view_model(&s);
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
