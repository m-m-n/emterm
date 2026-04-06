//! Mux status bar engine: settings loading, command execution, template resolution,
//! and periodic StatusUpdate generation.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::commands::config::{AppSettings, MuxStatusbarSettings};
use crate::mux::ipc::protocol::{MessageType, MuxMessage, StatusUpdateMsg};

/// Timeout for command execution (5 seconds).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Render interval (1 second).
const RENDER_INTERVAL: Duration = Duration::from_secs(1);

/// Cached output for a single registered command.
struct CommandState {
    executable: PathBuf,
    interval: Duration,
    cached_output: String,
}

/// Active pane ID tracker, shared between the connection handler and the engine.
pub type SharedActivePaneId = Arc<std::sync::Mutex<Option<u32>>>;

/// Shared CWD map: pane_id -> pane's own cwd Arc, registered once on pane creation.
/// Reading the cwd only requires locking the outer map briefly to clone the Arc,
/// then locking the inner Arc to read the Option<String>.
pub type SharedPaneCwdMap =
    Arc<std::sync::Mutex<HashMap<u32, Arc<std::sync::Mutex<Option<String>>>>>>;

/// Status bar engine: manages command execution, template resolution, and
/// periodic StatusUpdate generation.
pub struct StatusBarEngine {
    settings: MuxStatusbarSettings,
    hostname: String,
    command_states: HashMap<String, CommandState>,
    last_sent: Option<(String, String)>,
    active_pane_id: SharedActivePaneId,
    pane_cwd_map: SharedPaneCwdMap,
    settings_error: Option<String>,
}

impl StatusBarEngine {
    /// Create a new StatusBarEngine by reading settings from settings.json.
    ///
    /// Returns the engine and optionally an initial error StatusUpdate to send.
    pub fn new(active_pane_id: SharedActivePaneId, pane_cwd_map: SharedPaneCwdMap) -> Self {
        let (settings, settings_error) = load_statusbar_settings();

        let hostname = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_default();

        let mut command_states = HashMap::new();
        for (name, cmd) in &settings.commands {
            let executable = expand_tilde(&cmd.executable);
            command_states.insert(
                name.clone(),
                CommandState {
                    executable,
                    interval: Duration::from_millis(cmd.interval_ms.max(1000)),
                    cached_output: String::new(),
                },
            );
        }

        Self {
            settings,
            hostname,
            command_states,
            last_sent: None,
            active_pane_id,
            pane_cwd_map,
            settings_error,
        }
    }

    /// Whether the engine is enabled and should run timers.
    pub fn is_enabled(&self) -> bool {
        self.settings.enabled
    }

    /// Get the initial error message to send, if any (settings parse failure).
    pub fn initial_error_update(&self) -> Option<MuxMessage> {
        self.settings_error.as_ref().map(|err| {
            let msg = StatusUpdateMsg {
                left: err.clone(),
                right: String::new(),
            };
            MuxMessage::control(MessageType::StatusUpdate, 0, &msg)
        })
    }

    /// Whether the templates contain any variables that need periodic resolution.
    pub fn has_template_variables(&self) -> bool {
        self.settings.left.contains('{') || self.settings.right.contains('{')
    }

    /// Create a render interval timer (1 second), delayed so the first tick
    /// doesn't fire immediately before any commands have run.
    pub fn render_interval(&self) -> tokio::time::Interval {
        tokio::time::interval_at(
            tokio::time::Instant::now() + RENDER_INTERVAL,
            RENDER_INTERVAL,
        )
    }

    /// Get command names and their intervals for setting up per-command timers.
    pub fn command_intervals(&self) -> Vec<(String, Duration)> {
        self.command_states
            .iter()
            .map(|(name, state)| (name.clone(), state.interval))
            .collect()
    }

    /// Get the executable path for a registered command, if it exists.
    pub fn get_command_executable(&self, name: &str) -> Option<PathBuf> {
        self.command_states.get(name).map(|s| s.executable.clone())
    }

    /// Update the cached output for a command (called from spawned task results).
    /// Only updates if output is Some; None retains the previous cached value.
    pub fn update_command_cache(&mut self, name: &str, output: Option<String>) {
        if let Some(value) = output {
            if let Some(state) = self.command_states.get_mut(name) {
                state.cached_output = value;
            }
        }
    }

    /// Resolve templates and produce a StatusUpdate message if content changed.
    /// Returns None if content is the same as last send (differential).
    pub fn render(&mut self) -> Option<MuxMessage> {
        let cwd = self.get_active_pane_cwd();
        let left = resolve_template(
            &self.settings.left,
            &self.command_states,
            &self.hostname,
            &cwd,
        );
        let right = resolve_template(
            &self.settings.right,
            &self.command_states,
            &self.hostname,
            &cwd,
        );

        let current = (left, right);
        if self.last_sent.as_ref() == Some(&current) {
            return None;
        }

        self.last_sent = Some(current.clone());
        let msg = StatusUpdateMsg {
            left: current.0,
            right: current.1,
        };
        Some(MuxMessage::control(MessageType::StatusUpdate, 0, &msg))
    }

    /// Force-render (ignore diff) for RequestStatusUpdate responses.
    pub fn force_render(&mut self) -> MuxMessage {
        let cwd = self.get_active_pane_cwd();
        let left = resolve_template(
            &self.settings.left,
            &self.command_states,
            &self.hostname,
            &cwd,
        );
        let right = resolve_template(
            &self.settings.right,
            &self.command_states,
            &self.hostname,
            &cwd,
        );

        self.last_sent = Some((left.clone(), right.clone()));
        let msg = StatusUpdateMsg { left, right };
        MuxMessage::control(MessageType::StatusUpdate, 0, &msg)
    }

    /// Get the active pane's working directory (for command execution).
    pub fn active_cwd(&self) -> String {
        self.get_active_pane_cwd()
    }

    fn get_active_pane_cwd(&self) -> String {
        let active_id = *self.active_pane_id.lock().unwrap();
        match active_id {
            Some(pane_id) => {
                let cwd_arc = {
                    let map = self.pane_cwd_map.lock().unwrap();
                    map.get(&pane_id).cloned()
                };
                match cwd_arc {
                    Some(arc) => arc.lock().unwrap().clone().unwrap_or_default(),
                    None => String::new(),
                }
            }
            None => String::new(),
        }
    }
}

/// Expand `~` to home directory in executable path.
pub fn expand_tilde(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = home_dir() {
            return PathBuf::from(home).join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

/// Get the user's home directory (cross-platform).
fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()
}

/// Resolve a template string, replacing variables with cached values.
///
/// Variables:
/// - `{cmd:name}` -> cached command output
/// - `{hostname}` -> system hostname
/// - `{cwd}` -> active pane's working directory
///
/// Unknown variables are left as-is.
fn resolve_template(
    template: &str,
    commands: &HashMap<String, CommandState>,
    hostname: &str,
    cwd: &str,
) -> String {
    if template.is_empty() {
        return String::new();
    }

    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            // Try to find closing brace
            let mut var_name = String::new();
            let mut found_close = false;
            for inner_ch in chars.by_ref() {
                if inner_ch == '}' {
                    found_close = true;
                    break;
                }
                var_name.push(inner_ch);
            }

            if !found_close {
                // No closing brace found, output as-is
                result.push('{');
                result.push_str(&var_name);
                continue;
            }

            // Resolve the variable
            if var_name == "hostname" {
                result.push_str(hostname);
            } else if var_name == "cwd" {
                result.push_str(cwd);
            } else if let Some(cmd_name) = var_name.strip_prefix("cmd:") {
                if let Some(state) = commands.get(cmd_name) {
                    result.push_str(&state.cached_output);
                } else {
                    // Unknown command - leave as-is
                    result.push('{');
                    result.push_str(&var_name);
                    result.push('}');
                }
            } else {
                // Unknown variable - leave as-is
                result.push('{');
                result.push_str(&var_name);
                result.push('}');
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Execute a command with a 5-second timeout.
/// Returns the first line of stdout (trimmed), or None on timeout/error.
///
/// `cwd` is the active pane's working directory (from OSC 7 detection).
/// Falls back to HOME if empty or non-existent.
pub async fn execute_command(executable: &PathBuf, cwd: &str) -> Option<String> {
    let fallback_dir = home_dir().unwrap_or_else(|| {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| ".".to_string())
    });
    let work_dir = if !cwd.is_empty() && std::path::Path::new(cwd).is_dir() {
        cwd
    } else {
        &fallback_dir
    };

    let result = tokio::time::timeout(
        COMMAND_TIMEOUT,
        tokio::process::Command::new(executable)
            .current_dir(work_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let first_line = stdout.lines().next().unwrap_or("").trim().to_string();
            Some(first_line)
        }
        Ok(Ok(output)) => {
            log::warn!(
                "Command {:?} exited with status {}",
                executable,
                output.status
            );
            None
        }
        Ok(Err(e)) => {
            log::warn!("Command {:?} execution error: {}", executable, e);
            None
        }
        Err(_) => {
            log::warn!("Command {:?} timed out after 5s", executable);
            None
        }
    }
}

/// Load statusbar settings from settings.json.
/// Returns (settings, optional_error_message).
fn load_statusbar_settings() -> (MuxStatusbarSettings, Option<String>) {
    let settings_path = match settings_file_path() {
        Some(p) => p,
        None => {
            return (
                MuxStatusbarSettings::default(),
                Some("Settings file path not found".to_string()),
            );
        }
    };

    if !settings_path.exists() {
        // Create default settings file
        let dir = settings_path.parent().unwrap();
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("Failed to create settings dir: {}", e);
        } else {
            let default_settings = AppSettings::default();
            if let Ok(json) = serde_json::to_string_pretty(&default_settings) {
                if let Err(e) = std::fs::write(&settings_path, json) {
                    log::warn!("Failed to write default settings: {}", e);
                }
            }
        }
        return (MuxStatusbarSettings::default(), None);
    }

    let contents = match std::fs::read_to_string(&settings_path) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("settings.json: read error: {}", e);
            log::warn!("{}", msg);
            return (MuxStatusbarSettings::default(), Some(msg));
        }
    };

    // Parse as AppSettings to get mux.statusbar section
    match serde_json::from_str::<AppSettings>(&contents) {
        Ok(settings) => (settings.mux.statusbar, None),
        Err(e) => {
            let msg = format!("settings.json: parse error: {}", e);
            log::warn!("{}", msg);
            (MuxStatusbarSettings::default(), Some(msg))
        }
    }
}

/// Resolve the eMterm settings file path without AppHandle.
/// Reuses the same logic as tmux_import.rs.
fn settings_file_path() -> Option<PathBuf> {
    let config_base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            // Windows: use APPDATA (e.g., C:\Users\<user>\AppData\Roaming)
            std::env::var("APPDATA")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(
        config_base
            .join("net.laser5.app.emterm")
            .join("settings.json"),
    )
}

/// Detect OSC 7 sequence in PTY output bytes and extract the CWD path.
///
/// OSC 7 format: ESC ] 7 ; file://hostname/path ST
/// ST can be: ESC \ (0x1B 0x5C) or BEL (0x07)
///
/// Returns the extracted path on success, or None.
pub fn detect_osc7_cwd(data: &[u8]) -> Option<String> {
    // Look for ESC ] 7 ; pattern
    let pattern = b"\x1b]7;";
    let pos = data.windows(pattern.len()).position(|w| w == pattern)?;
    let start = pos + pattern.len();

    // Find the ST terminator (ESC \ or BEL)
    let rest = &data[start..];
    let end = rest.iter().enumerate().find_map(|(i, &b)| {
        if b == 0x07 {
            Some(i)
        } else if b == 0x1b && rest.get(i + 1) == Some(&0x5c) {
            Some(i)
        } else {
            None
        }
    })?;

    let uri = std::str::from_utf8(&rest[..end]).ok()?;

    // Strip file://hostname/ prefix to get the path
    if let Some(after_scheme) = uri.strip_prefix("file://") {
        // Find the first / after the hostname
        if let Some(slash_pos) = after_scheme.find('/') {
            let path = &after_scheme[slash_pos..];
            // URL-decode the path
            let decoded = url_decode(path);
            return Some(decoded);
        }
    }

    None
}

/// Simple percent-decoding for file paths (handles %XX sequences).
/// Collects decoded bytes into a Vec<u8> to correctly handle multi-byte UTF-8.
fn url_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        if b == b'%' {
            let hi = iter.next();
            let lo = iter.next();
            if let (Some(h), Some(l)) = (hi, lo) {
                let hex = [h, l];
                if let Ok(s) = std::str::from_utf8(&hex) {
                    if let Ok(val) = u8::from_str_radix(s, 16) {
                        bytes.push(val);
                        continue;
                    }
                }
                // Invalid hex - output as-is
                bytes.push(b'%');
                bytes.push(h);
                bytes.push(l);
            }
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Template Resolution Tests ----

    #[test]
    fn test_resolve_template_empty() {
        let cmds = HashMap::new();
        assert_eq!(resolve_template("", &cmds, "host", "/home"), "");
    }

    #[test]
    fn test_resolve_template_no_variables() {
        let cmds = HashMap::new();
        assert_eq!(
            resolve_template("plain text", &cmds, "host", "/home"),
            "plain text"
        );
    }

    #[test]
    fn test_resolve_template_hostname() {
        let cmds = HashMap::new();
        assert_eq!(
            resolve_template("Host: {hostname}", &cmds, "myhost", "/home"),
            "Host: myhost"
        );
    }

    #[test]
    fn test_resolve_template_cwd() {
        let cmds = HashMap::new();
        assert_eq!(
            resolve_template("{cwd}", &cmds, "host", "/home/user"),
            "/home/user"
        );
    }

    #[test]
    fn test_resolve_template_cmd() {
        let mut cmds = HashMap::new();
        cmds.insert(
            "branch".to_string(),
            CommandState {
                executable: PathBuf::from("/usr/bin/git-branch"),
                interval: Duration::from_millis(5000),
                cached_output: "main".to_string(),
            },
        );
        assert_eq!(
            resolve_template("{cmd:branch}", &cmds, "host", "/home"),
            "main"
        );
    }

    #[test]
    fn test_resolve_template_unknown_variable_left_as_is() {
        let cmds = HashMap::new();
        assert_eq!(
            resolve_template("{unknown}", &cmds, "host", "/home"),
            "{unknown}"
        );
    }

    #[test]
    fn test_resolve_template_unknown_cmd_left_as_is() {
        let cmds = HashMap::new();
        assert_eq!(
            resolve_template("{cmd:nonexistent}", &cmds, "host", "/home"),
            "{cmd:nonexistent}"
        );
    }

    #[test]
    fn test_resolve_template_mixed() {
        let mut cmds = HashMap::new();
        cmds.insert(
            "uptime".to_string(),
            CommandState {
                executable: PathBuf::from("uptime"),
                interval: Duration::from_millis(10000),
                cached_output: "2 days".to_string(),
            },
        );
        assert_eq!(
            resolve_template(
                "{hostname} | {cmd:uptime} | {cwd}",
                &cmds,
                "server1",
                "/var/log"
            ),
            "server1 | 2 days | /var/log"
        );
    }

    #[test]
    fn test_resolve_template_unclosed_brace() {
        let cmds = HashMap::new();
        assert_eq!(
            resolve_template("{hostname", &cmds, "host", "/home"),
            "{hostname"
        );
    }

    // ---- OSC 7 Detection Tests ----

    #[test]
    fn test_detect_osc7_with_esc_st() {
        let data = b"\x1b]7;file://myhost/home/user\x1b\\";
        assert_eq!(detect_osc7_cwd(data), Some("/home/user".to_string()));
    }

    #[test]
    fn test_detect_osc7_with_bel_st() {
        let data = b"\x1b]7;file://myhost/home/user\x07";
        assert_eq!(detect_osc7_cwd(data), Some("/home/user".to_string()));
    }

    #[test]
    fn test_detect_osc7_embedded_in_data() {
        let data = b"some output\x1b]7;file://host/tmp\x1b\\more data";
        assert_eq!(detect_osc7_cwd(data), Some("/tmp".to_string()));
    }

    #[test]
    fn test_detect_osc7_no_pattern() {
        let data = b"normal pty output without osc7";
        assert_eq!(detect_osc7_cwd(data), None);
    }

    #[test]
    fn test_detect_osc7_no_st() {
        let data = b"\x1b]7;file://host/home/user";
        assert_eq!(detect_osc7_cwd(data), None);
    }

    #[test]
    fn test_detect_osc7_url_encoded_path() {
        let data = b"\x1b]7;file://host/home/my%20folder\x1b\\";
        assert_eq!(detect_osc7_cwd(data), Some("/home/my folder".to_string()));
    }

    #[test]
    fn test_detect_osc7_empty_hostname() {
        let data = b"\x1b]7;file:///home/user\x1b\\";
        assert_eq!(detect_osc7_cwd(data), Some("/home/user".to_string()));
    }

    // ---- Tilde Expansion Tests ----

    #[test]
    fn test_expand_tilde_with_home() {
        unsafe { std::env::set_var("HOME", "/home/testuser") };
        let result = expand_tilde("~/bin/script.sh");
        assert_eq!(result, PathBuf::from("/home/testuser/bin/script.sh"));
    }

    #[test]
    fn test_expand_tilde_absolute_path() {
        let result = expand_tilde("/usr/bin/date");
        assert_eq!(result, PathBuf::from("/usr/bin/date"));
    }

    #[test]
    fn test_expand_tilde_no_slash() {
        // "~foo" should NOT be expanded (only "~/" prefix)
        let result = expand_tilde("~foo");
        assert_eq!(result, PathBuf::from("~foo"));
    }

    // ---- URL Decode Tests ----

    #[test]
    fn test_url_decode_no_encoding() {
        assert_eq!(url_decode("/home/user"), "/home/user");
    }

    #[test]
    fn test_url_decode_space() {
        assert_eq!(url_decode("/home/my%20folder"), "/home/my folder");
    }

    #[test]
    fn test_url_decode_multiple() {
        assert_eq!(url_decode("/a%20b%2Fc"), "/a b/c");
    }

    #[test]
    fn test_url_decode_multibyte_utf8() {
        assert_eq!(
            url_decode("/home/user/%E4%B8%AD%E6%96%87"),
            "/home/user/中文"
        );
    }

    // ---- Settings Loading Tests ----

    #[test]
    fn test_load_settings_missing_file() {
        // Use a temp dir that doesn't have settings.json
        let temp_dir = std::env::temp_dir().join("emterm_test_missing");
        let _ = std::fs::remove_dir_all(&temp_dir);
        unsafe { std::env::set_var("XDG_CONFIG_HOME", temp_dir.to_str().unwrap()) };
        let (settings, error) = load_statusbar_settings();
        assert!(!settings.enabled);
        assert!(error.is_none()); // Missing file creates default, no error
        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }

    #[test]
    fn test_load_settings_invalid_json() {
        let temp_dir = std::env::temp_dir().join("emterm_test_invalid");
        let settings_dir = temp_dir.join("net.laser5.app.emterm");
        std::fs::create_dir_all(&settings_dir).unwrap();
        std::fs::write(settings_dir.join("settings.json"), "not json{{{").unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", temp_dir.to_str().unwrap()) };
        let (settings, error) = load_statusbar_settings();
        assert!(!settings.enabled);
        assert!(error.is_some());
        assert!(error.unwrap().contains("parse error"));
        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }

    #[test]
    fn test_load_settings_valid_json() {
        let temp_dir = std::env::temp_dir().join("emterm_test_valid");
        let settings_dir = temp_dir.join("net.laser5.app.emterm");
        std::fs::create_dir_all(&settings_dir).unwrap();
        let json = r#"{
            "mux": {
                "statusbar": {
                    "enabled": true,
                    "left": "test left",
                    "right": "test right",
                    "commands": {}
                }
            }
        }"#;
        std::fs::write(settings_dir.join("settings.json"), json).unwrap();
        unsafe { std::env::set_var("XDG_CONFIG_HOME", temp_dir.to_str().unwrap()) };
        let (settings, error) = load_statusbar_settings();
        assert!(settings.enabled);
        assert_eq!(settings.left, "test left");
        assert_eq!(settings.right, "test right");
        assert!(error.is_none());
        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }
}
