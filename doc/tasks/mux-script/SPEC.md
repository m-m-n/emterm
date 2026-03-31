# Feature: Mux Script Subcommand

## Overview

Add `emterm mux script` subcommand that starts the mux daemon without attaching a bridge process. This enables shell scripts to initialize mux sessions (create windows, send keys) before the user attaches from the GUI.

## Objectives

- Allow scripted mux session initialization without GUI attachment
- Reuse existing `ensure_daemon_running()` infrastructure
- Provide idempotent daemon startup (safe to call multiple times)

## User Stories

### US1: Script-based mux initialization
As a user, I want to run a shell script that sets up my mux environment, so that I can start working immediately after attaching.

**Acceptance Criteria:**
- [ ] `emterm mux script` starts the daemon if not running
- [ ] `emterm mux script` exits successfully if daemon is already running
- [ ] `new-window` and `send-keys` work after `emterm mux script`
- [ ] `emterm mux` (attach) connects to the pre-initialized session

**Example script:**
```bash
#!/bin/bash
# ~/bin/init-mux

emterm mux script
emterm mux new-window -n editor -c "nvim"
emterm mux new-window -n monitor -c "htop"
printf 'cd ~/project\r' | emterm mux send-keys -t 0
```

## Technical Requirements

### Functional Requirements
- **FR1:** `emterm mux script` starts the mux daemon process without starting a bridge
- **FR2:** If the daemon is already running, exit with code 0 (no error)
- **FR3:** Print the daemon socket path to stdout on success

### Non-Functional Requirements
- **NFR1 - Simplicity:** Implementation reuses `ensure_daemon_running()` with no new logic

## Implementation Approach

### Modified Files

1. **`src-tauri/src/main.rs`** - Add `script` subcommand to clap definition
2. **`src-tauri/src/mux/cli.rs`** - Add `execute_script()` function

### Implementation Detail

`execute_script()`:
```rust
pub fn execute_script() -> Result<(), Box<dyn std::error::Error>> {
    let sock_path = daemon::ensure_daemon_running()
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    println!("{}", sock_path.display());
    Ok(())
}
```

### CLI Definition

```
emterm mux script    Start daemon without attaching (for scripted initialization)
```

No options or arguments.

## Test Scenarios

### Unit Tests
- [ ] `execute_script` starts daemon and returns socket path
- [ ] `execute_script` succeeds when daemon is already running

### Edge Cases
- [ ] Running `emterm mux script` inside an existing mux session (EMTERM_MUX check)

## Success Criteria

- [ ] `emterm mux script` starts daemon without blocking
- [ ] Shell script workflow (script → new-window → send-keys → attach) works end-to-end
- [ ] No regression in existing `emterm mux` behavior
