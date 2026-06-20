//! Line-oriented `tmux.conf` parser.
//!
//! Ported from `src-tauri/src/mux/tmux_conf/parser.rs` to support
//! native-poc's one-shot tmux.conf import (`mux::tmux_import`). Parses
//! tmux configuration directives on a best-effort basis. Supports:
//! `set`, `bind-key`, `unbind-key`. Ignores `if-shell`, `run-shell`,
//! format strings, hooks, and plugins.

/// A parsed `tmux.conf` directive.
#[derive(Debug, Clone, PartialEq)]
pub enum TmuxDirective {
    /// `set -g option value` or `set-option -g option value`.
    SetGlobal { option: String, value: String },
    /// `bind-key key command [args...]` or `bind key command [args...]`.
    BindKey { key: String, command: String },
    /// `unbind-key key` or `unbind key`.
    UnbindKey { key: String },
    /// Directive recognised but not supported by the importer.
    Unsupported { line: String, reason: String },
}

/// Parse a `tmux.conf` file's contents into directives. Empty / comment
/// lines are dropped silently.
pub fn parse_tmux_conf(contents: &str) -> Vec<TmuxDirective> {
    contents
        .lines()
        .filter_map(|line| parse_line(line.trim()))
        .collect()
}

fn parse_line(line: &str) -> Option<TmuxDirective> {
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let tokens = tokenize(line);
    if tokens.is_empty() {
        return None;
    }

    let cmd = tokens[0].as_str();
    match cmd {
        "set" | "set-option" => parse_set(&tokens),
        "bind" | "bind-key" => parse_bind(&tokens),
        "unbind" | "unbind-key" => parse_unbind(&tokens),
        "if-shell" | "if" => Some(TmuxDirective::Unsupported {
            line: line.to_string(),
            reason: "if-shell/conditional not supported".to_string(),
        }),
        "run-shell" | "run" => Some(TmuxDirective::Unsupported {
            line: line.to_string(),
            reason: "run-shell/external commands not supported".to_string(),
        }),
        "set-hook" => Some(TmuxDirective::Unsupported {
            line: line.to_string(),
            reason: "hooks not supported".to_string(),
        }),
        _ => Some(TmuxDirective::Unsupported {
            line: line.to_string(),
            reason: format!("unknown command: {}", cmd),
        }),
    }
}

fn parse_set(tokens: &[String]) -> Option<TmuxDirective> {
    let mut i = 1;
    while i < tokens.len() {
        if tokens[i] == "-g" || tokens[i] == "-ga" {
            i += 1;
        } else if tokens[i].starts_with('-') {
            i += 1;
        } else {
            break;
        }
    }

    if i + 1 >= tokens.len() {
        return None;
    }

    let option = tokens[i].clone();
    let value = tokens[i + 1..].join(" ");
    Some(TmuxDirective::SetGlobal { option, value })
}

fn parse_bind(tokens: &[String]) -> Option<TmuxDirective> {
    let mut i = 1;
    while i < tokens.len() && tokens[i].starts_with('-') {
        i += 1;
    }
    if i >= tokens.len() {
        return None;
    }
    let key = tokens[i].clone();
    let command = if i + 1 < tokens.len() {
        tokens[i + 1..].join(" ")
    } else {
        String::new()
    };
    Some(TmuxDirective::BindKey { key, command })
}

fn parse_unbind(tokens: &[String]) -> Option<TmuxDirective> {
    let mut i = 1;
    while i < tokens.len() && tokens[i].starts_with('-') {
        i += 1;
    }
    if i >= tokens.len() {
        return None;
    }
    Some(TmuxDirective::UnbindKey {
        key: tokens[i].clone(),
    })
}

/// Splits on whitespace, respecting single / double quotes. A `;` at the
/// top level terminates the line so trailing chained commands are
/// ignored (matching the WebView importer's behaviour).
fn tokenize(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for ch in line.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            ';' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                break;
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_and_comments() {
        assert!(parse_tmux_conf("").is_empty());
        assert!(parse_tmux_conf("# comment\n  \n").is_empty());
    }

    #[test]
    fn parse_set_global() {
        let directives = parse_tmux_conf("set -g prefix C-a");
        assert_eq!(directives.len(), 1);
        match &directives[0] {
            TmuxDirective::SetGlobal { option, value } => {
                assert_eq!(option, "prefix");
                assert_eq!(value, "C-a");
            }
            _ => panic!("expected SetGlobal"),
        }
    }

    #[test]
    fn parse_set_option_global() {
        let directives = parse_tmux_conf("set-option -g status-position top");
        assert_eq!(directives.len(), 1);
        match &directives[0] {
            TmuxDirective::SetGlobal { option, value } => {
                assert_eq!(option, "status-position");
                assert_eq!(value, "top");
            }
            _ => panic!("expected SetGlobal"),
        }
    }

    #[test]
    fn parse_bind_key() {
        let directives = parse_tmux_conf("bind-key r source-file ~/.tmux.conf");
        assert_eq!(directives.len(), 1);
        match &directives[0] {
            TmuxDirective::BindKey { key, command } => {
                assert_eq!(key, "r");
                assert_eq!(command, "source-file ~/.tmux.conf");
            }
            _ => panic!("expected BindKey"),
        }
    }

    #[test]
    fn parse_unbind() {
        let directives = parse_tmux_conf("unbind C-b");
        assert_eq!(directives.len(), 1);
        match &directives[0] {
            TmuxDirective::UnbindKey { key } => assert_eq!(key, "C-b"),
            _ => panic!("expected UnbindKey"),
        }
    }

    #[test]
    fn parse_if_shell_unsupported() {
        let directives =
            parse_tmux_conf("if-shell 'test -f ~/.local.conf' 'source-file ~/.local.conf'");
        assert_eq!(directives.len(), 1);
        match &directives[0] {
            TmuxDirective::Unsupported { reason, .. } => assert!(reason.contains("if-shell")),
            _ => panic!("expected Unsupported"),
        }
    }

    #[test]
    fn parse_run_shell_unsupported() {
        let directives = parse_tmux_conf("run-shell 'echo hello'");
        assert_eq!(directives.len(), 1);
        match &directives[0] {
            TmuxDirective::Unsupported { reason, .. } => assert!(reason.contains("run-shell")),
            _ => panic!("expected Unsupported"),
        }
    }

    #[test]
    fn parse_multiple_directives() {
        let conf = "
set -g prefix C-a
set -g status-position top
bind-key % split-window -h
# comment
unbind C-b
";
        assert_eq!(parse_tmux_conf(conf).len(), 4);
    }

    #[test]
    fn tokenize_with_quotes() {
        let tokens = tokenize("set -g status-left '[#S] '");
        assert_eq!(tokens, vec!["set", "-g", "status-left", "[#S] "]);
    }

    #[test]
    fn tokenize_semicolon_separator() {
        let tokens = tokenize("bind r source-file ~/.tmux.conf; display-message 'Reloaded'");
        assert_eq!(tokens, vec!["bind", "r", "source-file", "~/.tmux.conf"]);
    }
}
