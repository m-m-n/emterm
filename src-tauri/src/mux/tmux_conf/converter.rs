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
                result
                    .settings
                    .push((format!("keybind.{}", key), command.clone()));
            }
            TmuxDirective::UnbindKey { key } => {
                result.settings.push((
                    format!("keybind.{}", key),
                    String::new(), // empty = unbound
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
            let converted = convert_prefix_key(value);
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

/// Convert tmux prefix notation (C-a, M-b) to eMterm notation (ctrl+a, alt+b).
fn convert_prefix_key(tmux_key: &str) -> String {
    if tmux_key.starts_with("C-") {
        format!("ctrl+{}", &tmux_key[2..])
    } else if tmux_key.starts_with("M-") {
        format!("alt+{}", &tmux_key[2..])
    } else {
        tmux_key.to_string()
    }
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
            ("prefix".to_string(), "ctrl+a".to_string())
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
    fn test_convert_bind_key() {
        let directives = parse_tmux_conf("bind r source-file ~/.tmux.conf");
        let result = convert_directives(&directives);
        assert_eq!(result.settings[0].0, "keybind.r");
    }

    #[test]
    fn test_convert_unbind_key() {
        let directives = parse_tmux_conf("unbind C-b");
        let result = convert_directives(&directives);
        assert_eq!(
            result.settings[0],
            ("keybind.C-b".to_string(), String::new())
        );
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
    fn test_convert_prefix_key_notation() {
        assert_eq!(convert_prefix_key("C-a"), "ctrl+a");
        assert_eq!(convert_prefix_key("M-b"), "alt+b");
        assert_eq!(convert_prefix_key("F12"), "F12");
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
if-shell 'test -f ~/.local.conf' 'source ~/.local.conf'
";
        let directives = parse_tmux_conf(conf);
        let result = convert_directives(&directives);

        // 5 settings: prefix, unbind C-b, mouse, base-index, status-position, bind r
        assert_eq!(result.settings.len(), 6);
        // 1 warning: if-shell
        assert_eq!(result.warnings.len(), 1);
    }
}
