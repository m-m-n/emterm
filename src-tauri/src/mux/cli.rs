//! CLI subcommands for the mux multiplexer.
//!
//! - `emterm mux` — Start/attach to default session
//! - `emterm mux --daemon` — Run as daemon process (internal)
//! - `emterm mux attach [session]` — Attach to existing session
//! - `emterm mux ls` — List sessions
//! - `emterm mux kill [session]` — Kill a session
//! - `emterm mux new [name]` — Create a new session

use super::daemon;

/// Check if running inside eMterm (TERM_PROGRAM=emterm).
fn check_emterm_environment() -> Result<(), String> {
    match std::env::var("TERM_PROGRAM") {
        Ok(val) if val == "emterm" => Ok(()),
        _ => Err("emterm mux must be run inside eMterm terminal".to_string()),
    }
}

/// Check for nesting (EMTERM_MUX=1).
fn check_nesting() -> Result<(), String> {
    if std::env::var("EMTERM_MUX").is_ok() {
        Err("Cannot nest mux sessions (EMTERM_MUX is set)".to_string())
    } else {
        Ok(())
    }
}

/// Execute the `emterm mux --daemon` command (runs the daemon).
pub fn execute_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(daemon::run_daemon())?;
    Ok(())
}

/// Execute the `emterm mux` command (start/attach).
pub fn execute_mux() -> Result<(), Box<dyn std::error::Error>> {
    check_emterm_environment().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    check_nesting().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let sock_path = daemon::socket_path();

    // Start daemon if not running
    if !sock_path.exists() || !daemon::is_daemon_running(&sock_path) {
        // Spawn daemon as background process
        let exe = std::env::current_exe()?;
        let _child = std::process::Command::new(exe)
            .args(["mux", "--daemon"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        // Wait for daemon to start (check first, then sleep with backoff)
        let mut started = false;
        for i in 0..50 {
            if daemon::is_daemon_running(&sock_path) {
                started = true;
                break;
            }
            // Exponential backoff: 10ms, 10ms, 20ms, 40ms, 80ms, 100ms cap
            let delay = std::cmp::min(10 * (1 << i.min(4)), 100);
            std::thread::sleep(std::time::Duration::from_millis(delay));
        }

        if !started {
            return Err("Failed to start mux daemon".into());
        }
    }

    // Auto-import tmux.conf on first mux startup
    import_tmux_conf_if_needed();

    // Output OSC sequence to signal GUI
    let sock_str = sock_path.to_string_lossy();
    // session_id 0 = create/attach default session
    print!("\x1b]777;emterm;mux;attach;{};0\x1b\\", sock_str);
    // Flush immediately — print! without newline stays in stdout buffer
    use std::io::Write;
    let _ = std::io::stdout().flush();

    // CLI exits immediately after emitting OSC.
    // The GUI handles mux mode lifecycle (attach/detach) independently.
    // The daemon keeps running in the background.

    Ok(())
}

/// Execute the `emterm mux ls` command.
pub fn execute_ls() -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::socket_path();
    if !daemon::is_daemon_running(&sock_path) {
        println!("No mux daemon running");
        return Ok(());
    }

    // TODO: Connect and request session list
    println!("(session listing not yet implemented)");
    Ok(())
}

/// Execute the `emterm mux kill` command.
pub fn execute_kill(_session: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Connect and send kill request
    println!("(kill not yet implemented)");
    Ok(())
}

/// Import tmux.conf settings on first mux startup.
///
/// Reads the eMterm settings file directly (without AppHandle),
/// checks `tmux_conf_imported` flag, and applies tmux.conf conversions.
fn import_tmux_conf_if_needed() {
    let settings_path = match settings_file_path() {
        Some(p) => p,
        None => return,
    };

    let mut settings = if settings_path.exists() {
        match std::fs::read_to_string(&settings_path) {
            Ok(contents) => serde_json::from_str::<crate::commands::config::AppSettings>(&contents)
                .unwrap_or_default(),
            Err(_) => crate::commands::config::AppSettings::default(),
        }
    } else {
        crate::commands::config::AppSettings::default()
    };

    // Skip if already imported
    if settings.mux.tmux_conf_imported {
        return;
    }

    // Mark as imported (even if no tmux.conf exists, don't retry)
    settings.mux.tmux_conf_imported = true;

    // Try auto-import
    if let Some(result) = super::tmux_conf::converter::auto_import_tmux_conf() {
        for (key, value) in &result.settings {
            match key.as_str() {
                "prefix" => settings.mux.prefix = value.clone(),
                "base_index" => {
                    if let Ok(v) = value.parse::<u32>() {
                        settings.mux.base_index = v;
                    }
                }
                "mouse" => {
                    settings.mux.mouse = value == "true";
                }
                "status_position" => {
                    settings.mux.status_position = value.clone();
                }
                k if k.starts_with("keybind.") => {
                    let bind_key = k.strip_prefix("keybind.").unwrap().to_string();
                    settings.mux.keybinds.insert(bind_key, value.clone());
                }
                _ => {}
            }
        }

        for warning in &result.warnings {
            log::warn!("tmux.conf import: {}", warning);
        }

        if !result.settings.is_empty() {
            log::info!(
                "tmux.conf: imported {} settings ({} warnings)",
                result.settings.len(),
                result.warnings.len()
            );
        }
    }

    // Save settings
    if let Some(parent) = settings_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("Failed to create config directory: {}", e);
            return;
        }
    }
    match serde_json::to_string_pretty(&settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&settings_path, json) {
                log::warn!("Failed to save settings after tmux.conf import: {}", e);
            }
        }
        Err(e) => {
            log::warn!("Failed to serialize settings: {}", e);
        }
    }
}

/// Resolve the eMterm settings file path without AppHandle.
///
/// Uses XDG_CONFIG_HOME or ~/.config as base, matching Tauri's
/// `app_config_dir()` behavior on Linux.
fn settings_file_path() -> Option<std::path::PathBuf> {
    let config_base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(
        config_base
            .join("net.laser5.app.emterm")
            .join("settings.json"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_nesting_not_set() {
        // In test environment, EMTERM_MUX should not be set
        unsafe { std::env::remove_var("EMTERM_MUX") };
        assert!(check_nesting().is_ok());
    }

    #[test]
    fn test_check_nesting_set() {
        unsafe { std::env::set_var("EMTERM_MUX", "1") };
        assert!(check_nesting().is_err());
        unsafe { std::env::remove_var("EMTERM_MUX") };
    }

    #[test]
    fn test_check_emterm_not_set() {
        unsafe { std::env::remove_var("TERM_PROGRAM") };
        assert!(check_emterm_environment().is_err());
    }

    #[test]
    fn test_check_emterm_set() {
        unsafe { std::env::set_var("TERM_PROGRAM", "emterm") };
        assert!(check_emterm_environment().is_ok());
        unsafe { std::env::remove_var("TERM_PROGRAM") };
    }

    #[test]
    fn test_check_emterm_wrong_value() {
        unsafe { std::env::set_var("TERM_PROGRAM", "other-terminal") };
        assert!(check_emterm_environment().is_err());
        unsafe { std::env::remove_var("TERM_PROGRAM") };
    }

    #[test]
    fn test_settings_file_path_with_home() {
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        unsafe { std::env::set_var("HOME", "/tmp/test_home") };
        let path = settings_file_path().unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("/tmp/test_home/.config/net.laser5.app.emterm/settings.json")
        );
    }

    #[test]
    fn test_settings_file_path_with_xdg() {
        unsafe { std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdg_config") };
        let path = settings_file_path().unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("/tmp/xdg_config/net.laser5.app.emterm/settings.json")
        );
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }
}
