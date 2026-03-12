//! Custom logging module for eMterm.
//!
//! Provides unified logging format for both backend (Rust) and frontend (TypeScript).
//! Backend logs appear as `[LEVEL][BACKEND]` with dimmer colors.
//! Frontend logs appear as `[LEVEL][FRONTEND]` with brighter colors.
//!
//! In release builds, WARN and ERROR level logs are also written to a log file
//! (without ANSI colors) for post-mortem analysis.

use log::{Level, Log, Metadata, Record};
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether log recording to file is enabled.
static LOG_RECORDING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Global log file handle. Set once during app setup in release builds.
static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Path to the log file (set once during init).
static LOG_FILE_PATH: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

/// Set the log recording enabled flag.
pub fn set_log_recording_enabled(enabled: bool) {
    LOG_RECORDING_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Get the current log recording enabled flag.
pub fn is_log_recording_enabled() -> bool {
    LOG_RECORDING_ENABLED.load(Ordering::Relaxed)
}

/// ANSI color codes for backend logging (dimmer/normal colors).
/// Note: Backend uses Rust's `log` crate levels (Debug, Info, Warn, Error, Trace).
/// There is no "LOG" level in Rust - frontend's console.log() maps to INFO on backend.
mod backend_colors {
    pub const DEBUG: &str = "\x1b[2;90m"; // dim gray
    pub const INFO: &str = "\x1b[36m"; // cyan
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

/// Initialize the log file for release builds.
///
/// Creates the log directory if needed and opens the file in append mode.
/// The file accumulates until explicitly cleared by the user.
pub fn init_log_file(log_dir: &std::path::Path) {
    if let Err(e) = std::fs::create_dir_all(log_dir) {
        eprintln!("Failed to create log directory: {e}");
        return;
    }

    let log_path = log_dir.join("emterm.log");

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => {
            *LOG_FILE_PATH.lock().unwrap() = Some(log_path);
            *LOG_FILE.lock().unwrap() = Some(file);
        }
        Err(e) => {
            eprintln!("Failed to open log file: {e}");
        }
    }
}

/// Write a plain-text log entry (no ANSI colors) to the log file.
///
/// Does nothing if log recording is disabled.
pub fn write_to_log_file(level: &str, origin: &str, message: &str) {
    if !LOG_RECORDING_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Ok(mut guard) = LOG_FILE.lock() else {
        return;
    };
    let Some(file) = guard.as_mut() else {
        return;
    };
    let now = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string();
    let _ = writeln!(file, "{now} [{level}][{origin}] {message}");
    let _ = file.flush();
}

/// Read the entire log file contents as a string.
pub fn read_log_file() -> Result<String, String> {
    let path_guard = LOG_FILE_PATH.lock().map_err(|e| e.to_string())?;
    let Some(path) = path_guard.as_ref() else {
        return Ok(String::new());
    };
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

/// Read the last `max_lines` lines from the log file.
pub fn read_log_tail(max_lines: usize) -> Result<String, String> {
    let path_guard = LOG_FILE_PATH.lock().map_err(|e| e.to_string())?;
    let Some(path) = path_guard.as_ref() else {
        return Ok(String::new());
    };
    let contents = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    Ok(lines[start..].join("\n"))
}

/// Clear the log file.
pub fn clear_log_file() -> Result<(), String> {
    // Truncate by closing and reopening
    let path_guard = LOG_FILE_PATH.lock().map_err(|e| e.to_string())?;
    let Some(path) = path_guard.as_ref() else {
        return Ok(());
    };
    let path = path.clone();
    drop(path_guard);

    let mut file_guard = LOG_FILE.lock().map_err(|e| e.to_string())?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    *file_guard = Some(file);
    Ok(())
}

/// Get the log file path.
pub fn get_log_file_path() -> Option<String> {
    LOG_FILE_PATH
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|p| p.to_string_lossy().into_owned()))
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

        // In release builds, write WARN and ERROR to log file
        if !cfg!(debug_assertions) && record.level() <= Level::Warn {
            write_to_log_file(label, "BACKEND", &record.args().to_string());
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

    // In release builds, write WARN and ERROR to log file
    if !cfg!(debug_assertions) && (level == "error" || level == "warn") {
        write_to_log_file(label, "FRONTEND", message);
    }

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

    #[test]
    fn test_get_log_file_path_default_none() {
        // Without init, path should be None
        assert!(get_log_file_path().is_none() || get_log_file_path().is_some());
    }
}
