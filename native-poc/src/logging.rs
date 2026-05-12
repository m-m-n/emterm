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
/// Reads `RUST_LOG`; defaults to `info` when unset.
pub fn init() {
    INIT.call_once(|| {
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
