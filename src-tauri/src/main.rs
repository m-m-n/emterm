// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

rust_i18n::i18n!("locales", fallback = "en");

use clap::{Arg, Command};
use rust_i18n::t;
use std::path::PathBuf;

const SUPPORTED_LOCALES: &[&str] = &["en", "ja"];

/// Resolves the system locale to a supported language code.
///
/// Uses `sys_locale::get_locale()` for OS detection, then splits by
/// multiple separators (`-`, `_`, `.`) to handle formats like
/// `ja_JP`, `ja_JP.UTF-8`, `ja-JP`.
/// Falls back to "en" for unsupported locales.
fn resolve_system_locale() -> String {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "en".to_string());
    let base = locale.split(&['-', '_', '.'][..]).next().unwrap_or("en");
    if SUPPORTED_LOCALES.contains(&base) {
        base.to_string()
    } else {
        "en".to_string()
    }
}

/// Builds the CLI command using the clap builder API with localized strings.
fn build_cli() -> Command {
    Command::new("emterm")
        .about(t!("cli.about").to_string())
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand(
            Command::new("markdown")
                .about(t!("cli.markdownAbout").to_string())
                .arg(
                    Arg::new("file")
                        .help(t!("cli.markdownFile").to_string())
                        .value_name("FILE")
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("image")
                .about(t!("cli.imageAbout").to_string())
                .arg(
                    Arg::new("file")
                        .help(t!("cli.imageFile").to_string())
                        .value_name("FILE")
                        .required(true),
                )
                .arg(
                    Arg::new("protocol")
                        .long("protocol")
                        .help(t!("cli.imageProtocol").to_string())
                        .default_value("kitty"),
                ),
        )
}

fn main() {
    // Resolve system locale and set backend locale before argument parsing
    let locale = resolve_system_locale();
    rust_i18n::set_locale(&locale);

    let matches = build_cli().get_matches();

    match matches.subcommand() {
        Some(("markdown", sub_matches)) => {
            let file = PathBuf::from(sub_matches.get_one::<String>("file").unwrap());
            if let Err(err) = app_lib::commands::markdown::execute_markdown_command(&file) {
                eprintln!("Error: {}", err);
                std::process::exit(err.exit_code());
            }
        }
        Some(("image", sub_matches)) => {
            let file = PathBuf::from(sub_matches.get_one::<String>("file").unwrap());
            let protocol = sub_matches
                .get_one::<String>("protocol")
                .map(|s| s.as_str())
                .unwrap_or("kitty");

            let proto = match app_lib::commands::image::ImageProtocol::parse(protocol) {
                Ok(p) => p,
                Err(err) => {
                    eprintln!("Error: {}", err);
                    std::process::exit(err.exit_code());
                }
            };

            if let Err(err) = app_lib::commands::image::execute_image_command(&file, proto) {
                eprintln!("Error: {}", err);
                std::process::exit(err.exit_code());
            }
        }
        _ => {
            // No subcommand provided, run the Tauri GUI application
            #[cfg(not(test))]
            app_lib::run();
        }
    }
}
