//! Custom logging module for eMterm.
//!
//! Provides unified logging format for both backend (Rust) and frontend (TypeScript).
//! Backend logs appear as `[LEVEL][BACKEND]` with dimmer colors.
//! Frontend logs appear as `[LEVEL][FRONTEND]` with brighter colors.

use log::{Level, Log, Metadata, Record};

/// ANSI color codes for backend logging (dimmer/normal colors).
mod backend_colors {
    pub const DEBUG: &str = "\x1b[2;90m"; // dim gray
    pub const INFO: &str = "\x1b[36m"; // cyan
    pub const LOG: &str = "\x1b[32m"; // green (mapped from Info in some contexts)
    pub const WARN: &str = "\x1b[33m"; // yellow
    pub const ERROR: &str = "\x1b[31m"; // red
    pub const RESET: &str = "\x1b[0m";
}

/// ANSI color codes for frontend logging (brighter colors).
pub mod frontend_colors {
    pub const DEBUG: &str = "\x1b[90m"; // bright black/gray
    pub const INFO: &str = "\x1b[96m"; // bright cyan
    pub const LOG: &str = "\x1b[92m"; // bright green
    pub const WARN: &str = "\x1b[93m"; // bright yellow
    pub const ERROR: &str = "\x1b[91m"; // bright red
    pub const RESET: &str = "\x1b[0m";
}

/// Custom logger for backend Rust code.
///
/// Formats log messages as `[LEVEL][BACKEND] message` with appropriate colors.
pub struct BackendLogger {
    level: Level,
}

impl BackendLogger {
    /// Creates a new BackendLogger with the specified maximum log level.
    pub fn new(level: Level) -> Self {
        Self { level }
    }

    /// Initializes the global logger with this BackendLogger.
    ///
    /// # Panics
    ///
    /// Panics if a logger has already been set.
    pub fn init(level: Level) {
        let logger = Box::new(Self::new(level));
        log::set_boxed_logger(logger).expect("Failed to set logger");
        log::set_max_level(level.to_level_filter());
    }
}

impl Log for BackendLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let (color, label) = match record.level() {
            Level::Error => (backend_colors::ERROR, "ERROR"),
            Level::Warn => (backend_colors::WARN, "WARN"),
            Level::Info => (backend_colors::INFO, "INFO"),
            Level::Debug => (backend_colors::DEBUG, "DEBUG"),
            Level::Trace => (backend_colors::DEBUG, "TRACE"),
        };

        let message = format!(
            "{}[{}][BACKEND]{} {}",
            color,
            label,
            backend_colors::RESET,
            record.args()
        );

        // ERROR and WARN go to stderr, others to stdout
        if record.level() <= Level::Warn {
            eprintln!("{}", message);
        } else {
            println!("{}", message);
        }
    }

    fn flush(&self) {}
}

/// Formats a frontend log message with the appropriate color and label.
///
/// Returns the formatted string ready for output.
pub fn format_frontend_log(level: &str, message: &str) -> String {
    let (color, label) = match level {
        "error" => (frontend_colors::ERROR, "ERROR"),
        "warn" => (frontend_colors::WARN, "WARN"),
        "info" => (frontend_colors::INFO, "INFO"),
        "debug" => (frontend_colors::DEBUG, "DEBUG"),
        _ => (frontend_colors::LOG, "LOG"),
    };

    format!(
        "{}[{}][FRONTEND]{} {}",
        color,
        label,
        frontend_colors::RESET,
        message
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_frontend_log() {
        let msg = format_frontend_log("debug", "test message");
        assert!(msg.contains("[DEBUG][FRONTEND]"));
        assert!(msg.contains("test message"));

        let msg = format_frontend_log("error", "error message");
        assert!(msg.contains("[ERROR][FRONTEND]"));

        let msg = format_frontend_log("log", "log message");
        assert!(msg.contains("[LOG][FRONTEND]"));
    }

    #[test]
    fn test_backend_logger_enabled() {
        let logger = BackendLogger::new(Level::Info);

        // Info level logger should enable Error, Warn, Info
        assert!(logger.enabled(&log::Metadata::builder().level(Level::Error).build()));
        assert!(logger.enabled(&log::Metadata::builder().level(Level::Warn).build()));
        assert!(logger.enabled(&log::Metadata::builder().level(Level::Info).build()));

        // But not Debug or Trace
        assert!(!logger.enabled(&log::Metadata::builder().level(Level::Debug).build()));
        assert!(!logger.enabled(&log::Metadata::builder().level(Level::Trace).build()));
    }
}
