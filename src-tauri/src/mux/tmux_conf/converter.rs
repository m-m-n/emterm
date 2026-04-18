//! Convert parsed tmux.conf directives to eMterm mux settings.

use super::parser::TmuxDirective;

/// Conversion result: settings to apply + warnings for unsupported items.
#[derive(Debug, Default)]
pub struct ConversionResult {
    /// Settings to apply (key-value pairs for mux section).
    pub settings: Vec<(String, String)>,
    /// Warning messages for unsupported/ignored directives.
    pub warnings: Vec<String>,
}

/// Convert tmux directives to eMterm mux settings.
pub fn convert_directives(directives: &[TmuxDirective]) -> ConversionResult {
    let mut result = ConversionResult::default();

    for directive in directives {
        match directive {
            TmuxDirective::SetGlobal { option, value } => {
                convert_set_option(&mut result, option, value);
            }
            TmuxDirective::BindKey { key, command } => {
                convert_bind_key(&mut result, key, command);
            }
            TmuxDirective::UnbindKey { key } => {
                // Convert key notation but can't determine action without context
                let converted = convert_key_notation(key);
                result.warnings.push(format!(
                    "Skipped: unbind {} (unbind not supported, rebind in settings)",
                    converted
                ));
            }
            TmuxDirective::Unsupported { line, reason } => {
                result
                    .warnings
                    .push(format!("Skipped: {} ({})", line, reason));
            }
        }
    }

    result
}

fn convert_set_option(result: &mut ConversionResult, option: &str, value: &str) {
    match option {
        "prefix" | "prefix2" => {
            let converted = convert_key_notation(value);
            result.settings.push(("prefix".to_string(), converted));
        }
        "base-index" => {
            result
                .settings
                .push(("base_index".to_string(), value.to_string()));
        }
        "mouse" => {
            let v = match value {
                "on" => "true",
                "off" => "false",
                _ => value,
            };
            result.settings.push(("mouse".to_string(), v.to_string()));
        }
        "status-position" => {
            result
                .settings
                .push(("status_position".to_string(), value.to_string()));
        }
        "default-terminal" => {
            result.warnings.push(format!(
                "Note: default-terminal={} (eMterm uses xterm-256color)",
                value
            ));
        }
        // Options with format strings
        "status-left"
        | "status-right"
        | "status-style"
        | "window-status-format"
        | "window-status-current-format" => {
            if value.contains("#{") {
                result.warnings.push(format!(
                    "Skipped: {} (format strings not supported)",
                    option
                ));
            } else {
                result
                    .warnings
                    .push(format!("Skipped: {} (styling not yet supported)", option));
            }
        }
        _ => {
            result.warnings.push(format!(
                "Skipped: set -g {} {} (unknown option)",
                option, value
            ));
        }
    }
}

/// Convert tmux key notation (C-a, M-b) to eMterm notation (ctrl+a, alt+b).
fn convert_key_notation(tmux_key: &str) -> String {
    if let Some(suffix) = tmux_key.strip_prefix("C-") {
        format!("Ctrl+{}", suffix.to_uppercase())
    } else if let Some(suffix) = tmux_key.strip_prefix("M-") {
        format!("Alt+{}", suffix.to_uppercase())
    } else {
        tmux_key.to_string()
    }
}

/// Map tmux command string to eMterm mux action name.
/// Returns None for unsupported commands.
fn tmux_command_to_action(command: &str) -> Option<&'static str> {
    let trimmed = command.trim();
    if trimmed.starts_with("split-window -h") || trimmed.starts_with("split-window -bh") {
        Some("split-vertical")
    } else if trimmed.starts_with("split-window") {
        Some("split-horizontal")
    } else if trimmed.starts_with("select-pane") {
        Some("next-pane")
    } else if trimmed.starts_with("last-pane") {
        Some("prev-pane")
    } else if trimmed.starts_with("kill-pane")
        || (trimmed.starts_with("confirm-before") && trimmed.contains("kill-pane"))
    {
        Some("close-pane")
    } else if trimmed.starts_with("resize-pane -Z") {
        Some("zoom-toggle")
    } else if trimmed.starts_with("detach-client") || trimmed == "detach" {
        Some("detach")
    } else if trimmed.starts_with("new-window") {
        Some("new-window")
    } else if trimmed.starts_with("next-window") || trimmed == "next" {
        Some("next-window")
    } else if trimmed.starts_with("previous-window") || trimmed == "prev" {
        Some("prev-window")
    } else if trimmed.starts_with("rename-window")
        || (trimmed.starts_with("command-prompt") && trimmed.contains("rename-window"))
    {
        Some("rename-window")
    } else if trimmed.starts_with("copy-mode") {
        Some("copy-mode")
    } else if trimmed.starts_with("paste-buffer") {
        Some("paste")
    } else {
        None
    }
}

/// Convert a bind-key directive to eMterm action → key mapping.
fn convert_bind_key(result: &mut ConversionResult, key: &str, command: &str) {
    match tmux_command_to_action(command) {
        Some(action) => {
            let converted_key = convert_key_notation(key);
            result
                .settings
                .push((format!("keybind.{}", action), converted_key));
        }
        None => {
            result.warnings.push(format!(
                "Skipped: bind {} {} (unsupported command)",
                key, command
            ));
        }
    }
}

/// Auto-import tmux.conf from the user's home directory.
/// Returns None if the file doesn't exist or HOME is not set.
/// Returns Some(result) if the file was found and parsed.
pub fn auto_import_tmux_conf() -> Option<ConversionResult> {
    let home = std::env::var("HOME").ok()?;
    let conf_path = std::path::PathBuf::from(home).join(".tmux.conf");
    if !conf_path.exists() {
        return None;
    }
    let contents = std::fs::read_to_string(&conf_path).ok()?;
    let directives = super::parser::parse_tmux_conf(&contents);
    let result = convert_directives(&directives);
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse_tmux_conf;
    use super::*;

    #[test]
    fn test_convert_prefix() {
        let directives = parse_tmux_conf("set -g prefix C-a");
        let result = convert_directives(&directives);
        assert_eq!(result.settings.len(), 1);
        assert_eq!(
            result.settings[0],
            ("prefix".to_string(), "Ctrl+A".to_string())
        );
    }

    #[test]
    fn test_convert_mouse() {
        let directives = parse_tmux_conf("set -g mouse on");
        let result = convert_directives(&directives);
        assert_eq!(
            result.settings[0],
            ("mouse".to_string(), "true".to_string())
        );
    }

    #[test]
    fn test_convert_base_index() {
        let directives = parse_tmux_conf("set -g base-index 1");
        let result = convert_directives(&directives);
        assert_eq!(
            result.settings[0],
            ("base_index".to_string(), "1".to_string())
        );
    }

    #[test]
    fn test_convert_status_position() {
        let directives = parse_tmux_conf("set -g status-position top");
        let result = convert_directives(&directives);
        assert_eq!(
            result.settings[0],
            ("status_position".to_string(), "top".to_string())
        );
    }

    #[test]
    fn test_convert_bind_key_known_action() {
        let directives = parse_tmux_conf("bind c new-window");
        let result = convert_directives(&directives);
        assert_eq!(
            result.settings[0],
            ("keybind.new-window".to_string(), "c".to_string())
        );
    }

    #[test]
    fn test_convert_bind_key_with_modifier() {
        let directives = parse_tmux_conf("bind C-n new-window");
        let result = convert_directives(&directives);
        assert_eq!(
            result.settings[0],
            ("keybind.new-window".to_string(), "Ctrl+N".to_string())
        );
    }

    #[test]
    fn test_convert_bind_key_unknown_command() {
        let directives = parse_tmux_conf("bind r source-file ~/.tmux.conf");
        let result = convert_directives(&directives);
        assert!(result.settings.is_empty());
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("unsupported command"))
        );
    }

    #[test]
    fn test_convert_bind_key_split() {
        let directives = parse_tmux_conf("bind | split-window -h");
        let result = convert_directives(&directives);
        assert_eq!(
            result.settings[0],
            ("keybind.split-vertical".to_string(), "|".to_string())
        );
    }

    #[test]
    fn test_convert_unbind_key() {
        let directives = parse_tmux_conf("unbind C-b");
        let result = convert_directives(&directives);
        assert!(result.settings.is_empty());
        assert!(result.warnings.iter().any(|w| w.contains("unbind")));
    }

    #[test]
    fn test_unsupported_generates_warning() {
        let directives = parse_tmux_conf("if-shell 'test -f foo' 'source foo'");
        let result = convert_directives(&directives);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("Skipped"));
    }

    #[test]
    fn test_format_string_warning() {
        let directives = parse_tmux_conf("set -g status-left '#{session_name}'");
        let result = convert_directives(&directives);
        assert!(result.warnings.iter().any(|w| w.contains("format strings")));
    }

    #[test]
    fn test_convert_key_notation() {
        assert_eq!(convert_key_notation("C-a"), "Ctrl+A");
        assert_eq!(convert_key_notation("M-b"), "Alt+B");
        assert_eq!(convert_key_notation("F12"), "F12");
        assert_eq!(convert_key_notation("n"), "n");
    }

    #[test]
    fn test_full_config_conversion() {
        let conf = "
# My tmux config
set -g prefix C-a
unbind C-b
set -g mouse on
set -g base-index 1
set -g status-position top
bind r source-file ~/.tmux.conf
bind c new-window
if-shell 'test -f ~/.local.conf' 'source ~/.local.conf'
";
        let directives = parse_tmux_conf(conf);
        let result = convert_directives(&directives);

        // 5 settings: prefix, mouse, base-index, status-position, keybind.new-window
        assert_eq!(result.settings.len(), 5);
        // 3 warnings: unbind C-b, bind r (unsupported command), if-shell
        assert_eq!(result.warnings.len(), 3);
    }

    #[test]
    fn test_auto_import_no_home() {
        temp_env::with_var_unset("HOME", || {
            assert!(auto_import_tmux_conf().is_none());
        });
    }

    #[test]
    fn test_auto_import_no_file() {
        temp_env::with_var(
            "HOME",
            Some("/tmp/nonexistent_test_dir_auto_import"),
            || {
                assert!(auto_import_tmux_conf().is_none());
            },
        );
    }

    #[test]
    fn test_auto_import_with_file() {
        let dir = tempfile::tempdir().unwrap();
        let conf_path = dir.path().join(".tmux.conf");
        std::fs::write(&conf_path, "set -g prefix C-a\nset -g mouse on\n").unwrap();

        temp_env::with_var("HOME", Some(dir.path().to_str().unwrap()), || {
            let result = auto_import_tmux_conf();
            assert!(result.is_some());
            let result = result.unwrap();
            assert!(
                result
                    .settings
                    .iter()
                    .any(|(k, v)| k == "prefix" && v == "Ctrl+A")
            );
            assert!(
                result
                    .settings
                    .iter()
                    .any(|(k, v)| k == "mouse" && v == "true")
            );
        });
    }
}
