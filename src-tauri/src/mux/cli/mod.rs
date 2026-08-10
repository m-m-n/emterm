//! CLI subcommands for the mux multiplexer.
//!
//! - `emterm mux` -- Start/attach to default session (long-running bridge)
//! - `emterm mux --daemon` -- Run as daemon process (internal)
//! - `emterm mux attach [session]` -- Attach to existing session (long-running bridge)
//! - `emterm mux ls` -- List sessions
//! - `emterm mux kill [session]` -- Kill a session
//! - `emterm mux new [name]` -- Create a new session
//!
//! `run` is the single entry point invoked from `main.rs`; it owns all
//! subcommand-argument parsing for the mux CLI so the binary entry point
//! stays a one-liner.

mod admin;
pub use admin::*;

mod agent_api;
pub use agent_api::*;

mod connect;
pub use connect::*;

mod daemon_cmd;
pub use daemon_cmd::*;

mod pane_cmd;
pub use pane_cmd::*;

/// Usage summary printed for an unknown `mux` subcommand (AC-1, task0005:
/// `upgrade` / `probe-handoff` registered here alongside the dispatch
/// table).
const MUX_USAGE: &str = "Available: ls / kill [session] / attach [session] / new-window / script / \
     switch-window <index> / send-keys / read / send / wait / upgrade / \
     probe-handoff / clear-logs / (no subcommand = start session)";

/// Dispatch `emterm mux …` subcommands. `args` is the slice that follows
/// the literal `mux` token (so `args[0]` is the next positional, e.g.
/// `attach` / `--daemon` / `ls`). Returns the desired process exit code.
///
/// All argument-parsing logic for the mux CLI lives here, mirroring the
/// `emterm::cli::run` pattern used for `markdown` / `json` / `yaml` /
/// `image`. Adding a new mux subcommand means editing this file only;
/// `main.rs` just forwards.
pub fn run(args: &[String]) -> i32 {
    // `--daemon` is recognised both as a bare flag and as a "subcommand"
    // alongside the positional ones, matching the legacy clap surface.
    let mut daemon_mode = false;
    let mut positional: Vec<&str> = Vec::new();
    for a in args {
        if a == "--daemon" {
            daemon_mode = true;
        } else {
            positional.push(a.as_str());
        }
    }

    if daemon_mode {
        return match execute_daemon() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("Daemon error: {e}");
                1
            }
        };
    }

    let sub = positional.first().copied();
    let rest: Vec<&str> = positional.iter().copied().skip(1).collect();

    let result: Result<(), Box<dyn std::error::Error>> = match sub {
        Some("ls") => execute_ls(),
        Some("kill") => execute_kill(rest.first().copied()),
        Some("attach") => execute_attach(rest.first().copied()),
        Some("script") => execute_script(),
        Some("clear-logs") => execute_clear_logs(),
        Some("new-window") => {
            let mut name: Option<&str> = None;
            let mut command: Option<&str> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "-n" | "--name" => name = iter.next(),
                    "-c" | "--command" => command = iter.next(),
                    other => {
                        eprintln!("Error: unknown argument to `new-window`: {other}");
                        return 2;
                    }
                }
            }
            execute_new_window(name, command)
        }
        Some("switch-window") => {
            let Some(arg) = rest.first().copied() else {
                eprintln!("Error: `switch-window` requires a 0-based window index");
                return 2;
            };
            let idx: u32 = match arg.parse() {
                Ok(v) => v,
                Err(_) => {
                    eprintln!("Error: window index must be a non-negative integer (got {arg:?})");
                    return 2;
                }
            };
            execute_switch_window(idx)
        }
        Some("send-keys") => {
            let mut target: Option<u32> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "-t" | "--target" => {
                        let Some(v) = iter.next() else {
                            eprintln!("Error: `--target` requires a window index");
                            return 2;
                        };
                        match v.parse() {
                            Ok(n) => target = Some(n),
                            Err(_) => {
                                eprintln!(
                                    "Error: target must be a non-negative integer (got {v:?})"
                                );
                                return 2;
                            }
                        }
                    }
                    other => {
                        eprintln!("Error: unknown argument to `send-keys`: {other}");
                        return 2;
                    }
                }
            }
            execute_send_keys(target)
        }
        Some("read") => {
            let mut pane: Option<&str> = None;
            let mut lines: Option<&str> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "--pane" => pane = iter.next(),
                    "--lines" => lines = iter.next(),
                    other => {
                        eprintln!("Error: unknown argument to `read`: {other}");
                        return 2;
                    }
                }
            }
            let Some(pane) = pane else {
                eprintln!("Error: `read` requires --pane <id|current>");
                return 2;
            };
            return execute_mux_read(pane, lines);
        }
        Some("send") => {
            let mut pane: Option<&str> = None;
            let mut text: Option<&str> = None;
            let mut use_stdin = false;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "--pane" => pane = iter.next(),
                    "--text" => text = iter.next(),
                    "--stdin" => use_stdin = true,
                    other => {
                        eprintln!("Error: unknown argument to `send`: {other}");
                        return 2;
                    }
                }
            }
            let Some(pane) = pane else {
                eprintln!("Error: `send` requires --pane <id|current>");
                return 2;
            };
            if text.is_some() == use_stdin {
                eprintln!("Error: `send` requires exactly one of --text <s> or --stdin");
                return 2;
            }
            return execute_mux_send(pane, text, use_stdin);
        }
        Some("wait") => {
            let mut pane: Option<&str> = None;
            let mut state: Option<&str> = None;
            let mut timeout: Option<&str> = None;
            let mut after: Option<&str> = None;
            let mut iter = rest.iter().copied();
            while let Some(a) = iter.next() {
                match a {
                    "--pane" => pane = iter.next(),
                    "--state" => state = iter.next(),
                    "--timeout" => timeout = iter.next(),
                    "--after" => after = iter.next(),
                    other => {
                        eprintln!("Error: unknown argument to `wait`: {other}");
                        return 2;
                    }
                }
            }
            let Some(pane) = pane else {
                eprintln!("Error: `wait` requires --pane <id|current>");
                return 2;
            };
            let Some(state) = state else {
                eprintln!("Error: `wait` requires --state <set> (comma-separated)");
                return 2;
            };
            return execute_mux_wait(pane, state, timeout, after);
        }
        Some("upgrade") => {
            return execute_upgrade();
        }
        Some("probe-handoff") => {
            return execute_probe_handoff();
        }
        Some(other) => {
            eprintln!("Error: unknown `mux` subcommand: {other}");
            eprintln!("{MUX_USAGE}");
            return 2;
        }
        None => execute_mux(),
    };

    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
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

/// Initialize env_logger with a component label prefix (e.g. "[DAEMON]", "[BRIDGE]").
fn init_mux_logger(component: &'static str) {
    use std::io::Write;

    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .format(move |buf, record| {
            writeln!(
                buf,
                "{} {}{} {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f%:z"),
                record.level(),
                component,
                record.args()
            )
        })
        .init();
}

#[cfg(test)]
mod tests;
