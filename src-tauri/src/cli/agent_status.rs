//! `agent-status` subcommand handler.
//!
//! `emterm agent-status <idle|working|blocked|done> [--name <n>]` or
//! `emterm agent-status clear`. Stateless: builds the OSC 777
//! `emterm;agent-status;…` sequence via the core module
//! ([`crate::agent_status`]) and writes it to stdout, wrapped in tmux DCS
//! passthrough when running inside tmux (mirrors markdown/json/yaml/image
//! — see `crate::cli::tmux`).

use crate::agent_status::{self, AgentState, AgentStatusEvent};
use crate::cli::error::CommandError;
use crate::cli::tmux;
use std::io::{self, Write};

/// Map the CLI's `state` positional argument to an [`AgentStatusEvent`].
///
/// `clap`'s `PossibleValuesParser` (wired in `cli::build_command`)
/// already restricts the raw string to one of
/// `idle|working|blocked|done|clear` before this is ever called, so the
/// fallback arm is unreachable in practice, not a second validation pass
/// (AC-8: an invalid value never reaches here — clap rejects it with the
/// usage exit code during argument parsing).
fn state_arg_to_event(state_arg: &str, name: Option<&str>) -> AgentStatusEvent {
    match state_arg {
        "clear" => AgentStatusEvent::Clear,
        "idle" => AgentStatusEvent::Set {
            state: AgentState::Idle,
            name: name.map(str::to_string),
        },
        "working" => AgentStatusEvent::Set {
            state: AgentState::Working,
            name: name.map(str::to_string),
        },
        "blocked" => AgentStatusEvent::Set {
            state: AgentState::Blocked,
            name: name.map(str::to_string),
        },
        "done" => AgentStatusEvent::Set {
            state: AgentState::Done,
            name: name.map(str::to_string),
        },
        other => unreachable!("clap PossibleValuesParser should have rejected {other:?}"),
    }
}

/// Executes the `agent-status` subcommand: builds the wire sequence via
/// the core module and writes it to stdout (tmux DCS-passthrough wrapped
/// when running inside tmux).
pub fn execute_agent_status_command(
    state_arg: &str,
    name: Option<&str>,
) -> Result<(), CommandError> {
    let event = state_arg_to_event(state_arg, name);
    let sequence = agent_status::build(&event);
    output_to_stdout(&tmux::passthrough_if_needed(&sequence))?;
    Ok(())
}

/// Writes sequence to stdout with proper flushing
fn output_to_stdout(sequence: &str) -> io::Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(sequence.as_bytes())?;
    handle.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_arg_to_event_maps_working_with_name() {
        let event = state_arg_to_event("working", Some("claude"));
        assert_eq!(
            event,
            AgentStatusEvent::Set {
                state: AgentState::Working,
                name: Some("claude".to_string())
            }
        );
    }

    #[test]
    fn state_arg_to_event_maps_all_set_states_without_name() {
        for (arg, expected) in [
            ("idle", AgentState::Idle),
            ("working", AgentState::Working),
            ("blocked", AgentState::Blocked),
            ("done", AgentState::Done),
        ] {
            assert_eq!(
                state_arg_to_event(arg, None),
                AgentStatusEvent::Set {
                    state: expected,
                    name: None
                }
            );
        }
    }

    #[test]
    fn state_arg_to_event_maps_clear() {
        assert_eq!(state_arg_to_event("clear", None), AgentStatusEvent::Clear);
    }

    #[test]
    fn execute_agent_status_command_working_with_name_succeeds() {
        let result = execute_agent_status_command("working", Some("claude"));
        assert!(result.is_ok());
    }

    #[test]
    fn execute_agent_status_command_clear_succeeds() {
        let result = execute_agent_status_command("clear", None);
        assert!(result.is_ok());
    }

    // AC-6: the CLI writes exactly `agent_status::build(&event)` — no
    // extra framing — so the byte-exactness proven by
    // `crate::agent_status`'s `build_*_wire_format` tests carries over
    // here; this test only pins the plumbing (event -> build input).
    #[test]
    fn execute_agent_status_command_builds_expected_event() {
        let event = state_arg_to_event("blocked", Some("claude"));
        let expected = agent_status::build(&event);
        assert_eq!(
            expected,
            "\x1b]777;emterm;agent-status;v=1;state=blocked;name=claude\x1b\\"
        );
    }

    // AC-6 (tmux half): the same sequence composes correctly with the
    // shared tmux DCS-passthrough wrapper (`crate::cli::tmux`), which is
    // exhaustively unit-tested on its own in `cli::tmux`.
    #[test]
    fn agent_status_sequence_wraps_correctly_for_tmux() {
        let event = state_arg_to_event("working", Some("claude"));
        let sequence = agent_status::build(&event);
        let wrapped = tmux::wrap_each_sequence_for_test(&sequence);
        assert_eq!(
            wrapped,
            "\x1bPtmux;\x1b\x1b]777;emterm;agent-status;v=1;state=working;name=claude\x1b\x1b\\\x1b\\"
        );
    }
}
