// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "emterm")]
#[command(about = "eMterm - Modern terminal emulator with rich rendering", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Display Markdown file in eMterm
    Markdown {
        /// Path to Markdown file
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },
    /// Display image file in eMterm
    Image {
        /// Path to image file
        #[arg(value_name = "FILE")]
        file: PathBuf,

        /// Image protocol to use
        #[arg(long, default_value = "kitty")]
        protocol: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Markdown { file }) => {
            if let Err(err) = app_lib::commands::markdown::execute_markdown_command(&file) {
                eprintln!("Error: {}", err);
                std::process::exit(err.exit_code());
            }
        }
        Some(Commands::Image { file, protocol }) => {
            let proto = match app_lib::commands::image::ImageProtocol::parse(&protocol) {
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
        None => {
            // No subcommand provided, run the Tauri GUI application
            #[cfg(not(test))]
            app_lib::run();
        }
    }
}
