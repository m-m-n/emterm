//! CLI subcommand dispatcher.
//!
//! Entry point: [`run`] takes the post-binary-name slice of argv (so
//! `args[0]` is the subcommand) and returns an exit code suitable for
//! `std::process::exit`. Subcommands are bare words (`markdown`, `json`,
//! `yaml`, `image`, `html`); the dispatch arm in `main.rs` only delegates
//! here when one of these matches, so `--`-prefixed child-process flags
//! retain their existing hand-rolled path.

pub mod agent_status;
pub mod encoding;
pub mod error;
pub mod html;
pub mod image;
pub mod json;
pub mod markdown;
pub mod messages;
pub mod protocols;
pub mod tmux;
pub mod validation;
pub mod yaml;

use crate::i18n::Locale;
use std::sync::Mutex;

/// Cached active locale for one CLI invocation.
///
/// Stored in a `Mutex<Option<Locale>>` (not `OnceLock`) so test code can
/// swap the value between cases via [`set_active_locale_for_test`].
static ACTIVE_LOCALE: Mutex<Option<Locale>> = Mutex::new(None);

/// Resolve the active [`Locale`] once per process and cache the result.
///
/// On first call this reads only the `language` field from
/// `settings.json` (bypassing the full [`crate::settings_core::Settings`]
/// loader, which pulls in heavy modules unnecessary for CLI dispatch)
/// and resolves it through [`crate::i18n::resolve`]. Subsequent calls
/// return the cached value.
pub fn active_locale() -> Locale {
    let mut guard = ACTIVE_LOCALE.lock().expect("active_locale mutex poisoned");
    if let Some(loc) = *guard {
        return loc;
    }
    let language = load_language_only();
    let loc = crate::i18n::resolve(language);
    *guard = Some(loc);
    loc
}

/// Read only the `language` field from settings.json, without invoking
/// the full settings loader. Returns [`crate::settings_core::Language::Auto`]
/// when the file is absent, unreadable, or has no `language` field.
fn load_language_only() -> crate::settings_core::Language {
    use crate::settings_core::Language;
    let Some(path) = crate::settings_core::settings_path() else {
        return Language::Auto;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Language::Auto;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Language::Auto;
    };
    let Some(s) = value.get("language").and_then(|v| v.as_str()) else {
        return Language::Auto;
    };
    Language::parse_or_warn(s)
}

/// Test-only setter for the cached active locale.
#[cfg(test)]
pub fn set_active_locale_for_test(loc: Locale) {
    let mut guard = ACTIVE_LOCALE.lock().expect("active_locale mutex poisoned");
    *guard = Some(loc);
}

/// CLI dispatch entry point.
///
/// `args[0]` is expected to be the subcommand name (one of `markdown`,
/// `json`, `yaml`, `image`). The remaining positional argument is the
/// file path; `image` additionally accepts `--protocol kitty|sixel`.
///
/// Returns 0 on success, or the error's `exit_code()` on failure.
pub fn run(args: &[String]) -> i32 {
    // Build a synthetic argv with a program name prefix so clap's
    // bin/about strings work as expected. We use a derive-based clap
    // app for ergonomic parsing.
    let mut argv: Vec<String> = Vec::with_capacity(args.len() + 1);
    argv.push("emterm".to_string());
    argv.extend_from_slice(args);

    let loc = active_locale();
    let cli = match build_command(loc).try_get_matches_from(argv) {
        Ok(m) => m,
        Err(e) => {
            // clap prints its own help / version / error text.
            let _ = e.print();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };

    let result = match cli.subcommand() {
        Some(("markdown", sub)) => {
            let file: &std::path::PathBuf = sub.get_one("file").expect("required by clap");
            markdown::execute_markdown_command(file)
        }
        Some(("json", sub)) => {
            let file: &std::path::PathBuf = sub.get_one("file").expect("required by clap");
            json::execute_json_command(file)
        }
        Some(("yaml", sub)) => {
            let file: &std::path::PathBuf = sub.get_one("file").expect("required by clap");
            yaml::execute_yaml_command(file)
        }
        Some(("html", sub)) => {
            let file: &std::path::PathBuf = sub.get_one("file").expect("required by clap");
            html::execute_html_command(file)
        }
        Some(("image", sub)) => {
            let file: &std::path::PathBuf = sub.get_one("file").expect("required by clap");
            let protocol: &String = sub
                .get_one("protocol")
                .expect("clap default ensures presence");
            match image::ImageProtocol::parse(protocol) {
                Ok(proto) => image::execute_image_command(file, proto),
                Err(e) => Err(e),
            }
        }
        Some(("agent-status", sub)) => {
            let state: &String = sub.get_one("state").expect("required by clap");
            let name: Option<&String> = sub.get_one("name");
            agent_status::execute_agent_status_command(state, name.map(String::as_str))
        }
        _ => {
            // Should be unreachable: clap requires a subcommand. Treat
            // as a usage error.
            eprintln!("emterm: missing subcommand");
            return 2;
        }
    };

    match result {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("Error: {}", err);
            err.exit_code()
        }
    }
}

/// Construct the clap command tree with locale-aware help text.
fn build_command(loc: Locale) -> clap::Command {
    use clap::{Arg, Command};

    Command::new("emterm")
        .about(messages::cli_about(loc))
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("markdown")
                .about(messages::cli_markdown_about(loc))
                .arg(
                    Arg::new("file")
                        .help(messages::cli_markdown_file(loc))
                        .required(true)
                        .value_parser(clap::value_parser!(std::path::PathBuf)),
                ),
        )
        .subcommand(
            Command::new("json")
                .about(messages::cli_json_about(loc))
                .arg(
                    Arg::new("file")
                        .help(messages::cli_json_file(loc))
                        .required(true)
                        .value_parser(clap::value_parser!(std::path::PathBuf)),
                ),
        )
        .subcommand(
            Command::new("yaml")
                .about(messages::cli_yaml_about(loc))
                .arg(
                    Arg::new("file")
                        .help(messages::cli_yaml_file(loc))
                        .required(true)
                        .value_parser(clap::value_parser!(std::path::PathBuf)),
                ),
        )
        .subcommand(
            Command::new("html")
                .about(messages::cli_html_about(loc))
                .arg(
                    Arg::new("file")
                        .help(messages::cli_html_file(loc))
                        .required(true)
                        .value_parser(clap::value_parser!(std::path::PathBuf)),
                ),
        )
        .subcommand(
            Command::new("image")
                .about(messages::cli_image_about(loc))
                .arg(
                    Arg::new("file")
                        .help(messages::cli_image_file(loc))
                        .required(true)
                        .value_parser(clap::value_parser!(std::path::PathBuf)),
                )
                .arg(
                    Arg::new("protocol")
                        .long("protocol")
                        .help(messages::cli_image_protocol(loc))
                        .default_value("kitty")
                        .value_parser(clap::builder::NonEmptyStringValueParser::new()),
                ),
        )
        .subcommand(
            Command::new("agent-status")
                .about(messages::cli_agent_status_about(loc))
                .arg(
                    Arg::new("state")
                        .help(messages::cli_agent_status_state(loc))
                        .required(true)
                        .value_parser(clap::builder::PossibleValuesParser::new([
                            "idle", "working", "blocked", "done", "clear",
                        ])),
                )
                .arg(
                    Arg::new("name")
                        .long("name")
                        .help(messages::cli_agent_status_name(loc))
                        .value_parser(clap::builder::NonEmptyStringValueParser::new()),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_command_accepts_markdown_subcommand() {
        let m = build_command(Locale::En)
            .try_get_matches_from(["emterm", "markdown", "foo.md"])
            .expect("parse should succeed");
        assert_eq!(m.subcommand_name(), Some("markdown"));
    }

    #[test]
    fn build_command_accepts_html_subcommand() {
        let m = build_command(Locale::En)
            .try_get_matches_from(["emterm", "html", "foo.html"])
            .expect("parse should succeed");
        assert_eq!(m.subcommand_name(), Some("html"));
    }

    #[test]
    fn build_command_accepts_image_with_protocol() {
        let m = build_command(Locale::En)
            .try_get_matches_from(["emterm", "image", "foo.png", "--protocol", "sixel"])
            .expect("parse should succeed");
        let sub = m.subcommand_matches("image").unwrap();
        let protocol: &String = sub.get_one("protocol").unwrap();
        assert_eq!(protocol, "sixel");
    }

    #[test]
    fn build_command_defaults_image_protocol_to_kitty() {
        let m = build_command(Locale::En)
            .try_get_matches_from(["emterm", "image", "foo.png"])
            .expect("parse should succeed");
        let sub = m.subcommand_matches("image").unwrap();
        let protocol: &String = sub.get_one("protocol").unwrap();
        assert_eq!(protocol, "kitty");
    }

    #[test]
    fn build_command_rejects_unknown_subcommand() {
        let result = build_command(Locale::En).try_get_matches_from(["emterm", "explode", "foo"]);
        assert!(result.is_err());
    }

    #[test]
    fn build_command_accepts_agent_status_with_name() {
        let m = build_command(Locale::En)
            .try_get_matches_from(["emterm", "agent-status", "working", "--name", "claude"])
            .expect("parse should succeed");
        let sub = m.subcommand_matches("agent-status").unwrap();
        let state: &String = sub.get_one("state").unwrap();
        let name: &String = sub.get_one("name").unwrap();
        assert_eq!(state, "working");
        assert_eq!(name, "claude");
    }

    #[test]
    fn build_command_accepts_agent_status_clear() {
        let m = build_command(Locale::En)
            .try_get_matches_from(["emterm", "agent-status", "clear"])
            .expect("parse should succeed");
        let sub = m.subcommand_matches("agent-status").unwrap();
        let state: &String = sub.get_one("state").unwrap();
        assert_eq!(state, "clear");
        assert!(sub.get_one::<String>("name").is_none());
    }

    // AC-8: an invalid state value is a usage error (clap rejects it
    // during argument parsing, before dispatch ever runs).
    #[test]
    fn build_command_rejects_invalid_agent_status_state() {
        let result =
            build_command(Locale::En).try_get_matches_from(["emterm", "agent-status", "sleeping"]);
        assert!(result.is_err());
    }

    #[test]
    fn run_agent_status_invalid_state_returns_usage_exit_code() {
        let code = run(&["agent-status".to_string(), "sleeping".to_string()]);
        assert_eq!(code, 2, "invalid state should map to the usage exit code");
    }
}
