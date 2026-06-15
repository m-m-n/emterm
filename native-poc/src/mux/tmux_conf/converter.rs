//! Convert parsed `tmux.conf` directives to eMterm mux settings.
//!
//! Ported from `src-tauri/src/mux/tmux_conf/converter.rs`. The output
//! is a flat list of `(key, value)` strings the importer
//! ([`crate::mux::tmux_import`]) applies onto `settings.json` as a JSON
//! patch. Anything we don't model (`if-shell`, format strings, unknown
//! bindings) becomes a warning instead.

use super::parser::TmuxDirective;

/// Conversion result: settings to apply + warnings for unsupported items.
#[derive(Debug, Default)]
pub struct ConversionResult {
    /// Settings to apply (key-value pairs targeting the `mux` section).
    pub settings: Vec<(String, String)>,
    /// Warning messages for unsupported / ignored directives.
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

/// Convert tmux key notation (`C-a`, `M-b`) to eMterm notation
/// (`Ctrl+A`, `Alt+B`).
fn convert_key_notation(tmux_key: &str) -> String {
    if let Some(suffix) = tmux_key.strip_prefix("C-") {
        format!("Ctrl+{}", suffix.to_uppercase())
    } else if let Some(suffix) = tmux_key.strip_prefix("M-") {
        format!("Alt+{}", suffix.to_uppercase())
    } else {
        tmux_key.to_string()
    }
}

/// Map a tmux command string to an eMterm mux action name. Returns
/// `None` for unsupported commands.
///
/// Allowlist source: [`crate::settings::MUX_ACTION_NAMES`] (the actions
/// `RawSettings::merge_into` will accept under `mux.keybinds.*`). The
/// `split-vertical` / `split-horizontal` / `next-pane` / `prev-pane` /
/// `close-pane` / `zoom-toggle` / `copy-mode` / `paste` actions were
/// removed by SPEC mux-feature-cleanup; terminal-multiplexer §FR10
/// requires the importer log and skip them instead of writing dead
/// `keybind.<removed-action>` entries to `settings.json`. WebView-side
/// converter still emits the legacy names so its frontend can ignore
/// them; native-poc has no such frontend, so we drop them entirely at
/// the converter so they never reach the patch.
fn tmux_command_to_action(command: &str) -> Option<&'static str> {
    let trimmed = command.trim();
    if trimmed.starts_with("detach-client") || trimmed == "detach" {
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
    } else {
        // Everything else — split-window / select-pane / last-pane /
        // kill-pane / resize-pane -Z / copy-mode / paste-buffer / unknown —
        // is unsupported. `convert_bind_key` will emit the existing
        // "unsupported command" warning so the user sees what was dropped.
        None
    }
}

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

#[cfg(test)]
mod tests {
    use super::super::parser::parse_tmux_conf;
    use super::*;

    #[test]
    fn convert_prefix() {
        let result = convert_directives(&parse_tmux_conf("set -g prefix C-a"));
        assert_eq!(result.settings.len(), 1);
        assert_eq!(
            result.settings[0],
            ("prefix".to_string(), "Ctrl+A".to_string())
        );
    }

    #[test]
    fn convert_mouse() {
        let result = convert_directives(&parse_tmux_conf("set -g mouse on"));
        assert_eq!(
            result.settings[0],
            ("mouse".to_string(), "true".to_string())
        );
    }

    #[test]
    fn convert_base_index() {
        let result = convert_directives(&parse_tmux_conf("set -g base-index 1"));
        assert_eq!(
            result.settings[0],
            ("base_index".to_string(), "1".to_string())
        );
    }

    #[test]
    fn convert_status_position() {
        let result = convert_directives(&parse_tmux_conf("set -g status-position top"));
        assert_eq!(
            result.settings[0],
            ("status_position".to_string(), "top".to_string())
        );
    }

    #[test]
    fn convert_bind_key_known_action() {
        let result = convert_directives(&parse_tmux_conf("bind c new-window"));
        assert_eq!(
            result.settings[0],
            ("keybind.new-window".to_string(), "c".to_string())
        );
    }

    #[test]
    fn convert_bind_key_with_modifier() {
        let result = convert_directives(&parse_tmux_conf("bind C-n new-window"));
        assert_eq!(
            result.settings[0],
            ("keybind.new-window".to_string(), "Ctrl+N".to_string())
        );
    }

    #[test]
    fn convert_bind_key_unknown_command() {
        let result = convert_directives(&parse_tmux_conf("bind r source-file ~/.tmux.conf"));
        assert!(result.settings.is_empty());
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("unsupported command")));
    }

    #[test]
    fn convert_bind_key_split_is_skipped() {
        // SPEC mux-feature-cleanup removed the `split-vertical` action;
        // terminal-multiplexer §FR10 mandates "logged and skipped" for
        // unsupported actions. The converter must NOT write a dead
        // `keybind.split-vertical` entry to settings.json.
        let result = convert_directives(&parse_tmux_conf("bind | split-window -h"));
        assert!(
            result.settings.is_empty(),
            "split-window must not produce a removed-action keybind"
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("unsupported command")),
            "split-window must emit an 'unsupported command' warning"
        );
    }

    #[test]
    fn convert_bind_key_copy_mode_is_skipped() {
        // Sibling regression for `copy-mode`, also removed by SPEC.
        let result = convert_directives(&parse_tmux_conf("bind [ copy-mode"));
        assert!(result.settings.is_empty());
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("unsupported command")));
    }

    #[test]
    fn convert_bind_key_pane_actions_are_skipped() {
        // Sibling regression: every removed pane action must produce no
        // settings and one warning each. Keeps this list in lockstep with
        // `tmux_command_to_action`'s allowlist.
        // Note: avoid `;` in the key column — `tokenize()` treats an
        // unescaped semicolon as a top-level statement separator, which
        // truncates the line before the command is reached. Real tmux
        // configs use `bind \;` for the literal-semicolon binding; the
        // pane-action coverage doesn't need to exercise that escape.
        for line in [
            "bind | split-window -h",
            "bind - split-window",
            "bind o select-pane -t :.+",
            "bind L last-pane",
            "bind x kill-pane",
            "bind z resize-pane -Z",
            "bind ] paste-buffer",
        ] {
            let result = convert_directives(&parse_tmux_conf(line));
            assert!(
                result.settings.is_empty(),
                "{line}: must not produce settings"
            );
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|w| w.contains("unsupported command")),
                "{line}: must emit unsupported-command warning"
            );
        }
    }

    #[test]
    fn convert_unbind_key() {
        let result = convert_directives(&parse_tmux_conf("unbind C-b"));
        assert!(result.settings.is_empty());
        assert!(result.warnings.iter().any(|w| w.contains("unbind")));
    }

    #[test]
    fn unsupported_generates_warning() {
        let result = convert_directives(&parse_tmux_conf("if-shell 'test -f foo' 'source foo'"));
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("Skipped"));
    }

    #[test]
    fn format_string_warning() {
        let result = convert_directives(&parse_tmux_conf("set -g status-left '#{session_name}'"));
        assert!(result.warnings.iter().any(|w| w.contains("format strings")));
    }

    #[test]
    fn convert_key_notation_cases() {
        assert_eq!(convert_key_notation("C-a"), "Ctrl+A");
        assert_eq!(convert_key_notation("M-b"), "Alt+B");
        assert_eq!(convert_key_notation("F12"), "F12");
        assert_eq!(convert_key_notation("n"), "n");
    }

    #[test]
    fn full_config_conversion() {
        let conf = "
# my tmux config
set -g prefix C-a
unbind C-b
set -g mouse on
set -g base-index 1
set -g status-position top
bind r source-file ~/.tmux.conf
bind c new-window
if-shell 'test -f ~/.local.conf' 'source ~/.local.conf'
";
        let result = convert_directives(&parse_tmux_conf(conf));
        // 5 settings: prefix, mouse, base-index, status-position, keybind.new-window
        assert_eq!(result.settings.len(), 5);
        // 3 warnings: unbind C-b, bind r (unsupported), if-shell
        assert_eq!(result.warnings.len(), 3);
    }
}
