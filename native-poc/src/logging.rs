//! `env_logger` initialization with origin tagging.
//!
//! Tagging matches the project convention (`[LEVEL][ORIGIN]`). All native-poc
//! logs use the `NATIVE-POC` origin; this keeps them distinguishable from the
//! existing Tauri build logs in mixed sessions.

use std::io::Write;
use std::sync::Once;

static INIT: Once = Once::new();

/// Initialize the global logger. Safe to call multiple times.
///
/// Reads `RUST_LOG`. When unset, defaults to `info` for native-poc itself
/// while clamping noisy framework loggers (`wgpu*`, `naga`) to `warn` so
/// the per-frame `Device::maintain` info chatter does not flood the
/// stderr in normal runs. Users can still opt into the verbose stream
/// via `RUST_LOG=wgpu_core=info` (or similar) when debugging.
pub fn init() {
    INIT.call_once(|| {
        // Set the env-var only if the user hasn't already provided one.
        // We touch it from a single-threaded startup path (call_once on
        // the main thread), well before any other component spawns
        // threads that might also touch the environment. The intent is
        // "default filter unless the user overrode it"; users can still
        // opt back into the verbose stream with e.g.
        // `RUST_LOG=wgpu_core=info`.
        if std::env::var_os("RUST_LOG").is_none() {
            // 2024-edition note: when this crate eventually moves to
            // edition 2024 `set_var` becomes `unsafe`. Until then the
            // call is safe under the single-threaded-startup invariant
            // above.
            std::env::set_var(
                "RUST_LOG",
                "info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn",
            );
        }
        let mut builder =
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"));
        builder.format(|buf, record| {
            writeln!(buf, "[{}][NATIVE-POC] {}", record.level(), record.args())
        });
        // Best-effort init; if a logger was already installed (unlikely in
        // this binary) we silently continue.
        let _ = builder.try_init();
    });
}
