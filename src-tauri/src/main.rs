// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(
    all(not(debug_assertions), feature = "gui"),
    windows_subsystem = "windows"
)]

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
        .version(env!("APP_VERSION"))
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
        .subcommand(
            Command::new("json")
                .about(t!("cli.jsonAbout").to_string())
                .arg(
                    Arg::new("file")
                        .help(t!("cli.jsonFile").to_string())
                        .value_name("FILE")
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("yaml")
                .about(t!("cli.yamlAbout").to_string())
                .arg(
                    Arg::new("file")
                        .help(t!("cli.yamlFile").to_string())
                        .value_name("FILE")
                        .required(true),
                ),
        )
        .subcommand(
            Command::new("download")
                .about(t!("cli.downloadAbout").to_string())
                .arg(
                    Arg::new("file")
                        .help(t!("cli.downloadFile").to_string())
                        .value_name("FILE"),
                )
                .arg(
                    Arg::new("name")
                        .long("name")
                        .help(t!("cli.downloadName").to_string())
                        .value_name("NAME"),
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
        Some(("json", sub_matches)) => {
            let file = PathBuf::from(sub_matches.get_one::<String>("file").unwrap());
            if let Err(err) = app_lib::commands::json::execute_json_command(&file) {
                eprintln!("Error: {}", err);
                std::process::exit(err.exit_code());
            }
        }
        Some(("yaml", sub_matches)) => {
            let file = PathBuf::from(sub_matches.get_one::<String>("file").unwrap());
            if let Err(err) = app_lib::commands::yaml::execute_yaml_command(&file) {
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
        Some(("download", sub_matches)) => {
            use std::io::IsTerminal;

            let file = sub_matches.get_one::<String>("file");
            let name = sub_matches.get_one::<String>("name");

            let result = if let Some(file_path) = file {
                app_lib::commands::download::execute_download_command(&PathBuf::from(file_path))
            } else {
                // stdin mode: --name is required, and stdin must not be a TTY
                match name {
                    Some(n) => {
                        if std::io::stdin().is_terminal() {
                            eprintln!("Error: stdin is a TTY. Provide a file path or pipe data.");
                            std::process::exit(1);
                        }
                        app_lib::commands::download::execute_download_from_stdin(n)
                    }
                    None => Err(app_lib::error::CommandError::NameRequired),
                }
            };

            if let Err(err) = result {
                eprintln!("Error: {}", err);
                std::process::exit(err.exit_code());
            }
        }
        _ => {
            // No subcommand provided
            #[cfg(feature = "gui")]
            {
                #[cfg(not(test))]
                app_lib::run();
            }
            #[cfg(not(feature = "gui"))]
            {
                // CLI-only build: show help when no subcommand provided
                build_cli().print_help().ok();
                std::process::exit(0);
            }
        }
    }
}
