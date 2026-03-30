# Feature: Windows Shebang Emulation for Status Bar Custom Commands

## Overview

On Windows, the OS does not natively handle Unix shebang (`#!`) lines in script files. When a status bar custom command points to a script file (non-PE executable), the backend must parse the shebang line and invoke the specified interpreter manually. This is Windows-only logic; on Linux the kernel handles shebangs natively.

## Objectives

- Enable script-based status bar custom commands to work on Windows
- Detect PE executables vs script files by reading magic bytes
- Parse shebang lines and invoke the interpreter with the script as an argument

## User Stories

### US1: Run Script-Based Custom Command on Windows
As a Windows user, I want to register a script file (e.g., Python, Bun) as a status bar custom command, so that it executes correctly via its shebang-declared interpreter.

**Acceptance Criteria:**
- [ ] A script file with a valid shebang line executes using the declared interpreter
- [ ] A PE executable (.exe) runs directly without shebang parsing
- [ ] A non-PE file without a shebang line returns an error

## Technical Requirements

### Functional Requirements
- **FR1: PE Detection** - Read the first 2 bytes of the executable file. If they are `MZ` (0x4D 0x5A), treat the file as a PE executable and run it directly.
- **FR2: Shebang Parsing** - For non-PE files, read the first line. If it starts with `#!`, extract the interpreter path. Use the path as-is with no conversion or special handling (e.g., `#!/usr/bin/env bun` uses `/usr/bin/env` literally).
- **FR3: Interpreter Invocation** - Spawn the interpreter with the script file path as the first argument.
- **FR4: Error on Missing Shebang** - If a non-PE file does not have a shebang line, return an error.
- **FR5: Auto-trust Interpreter** - The interpreter extracted from the shebang is automatically allowed (no additional allowlist check). Registration of a script in `statusbar_custom_commands` implies trust of its shebang interpreter.
- **FR6: CREATE_NO_WINDOW Flag** - Set `CREATE_NO_WINDOW` (0x08000000) creation flag on Windows when spawning child processes to prevent console window flashing.

### Non-Functional Requirements
- **NFR1 - Platform Scope:** This logic applies only to Windows (`#[cfg(windows)]`). Linux code paths remain unchanged.
- **NFR2 - Performance:** File reads for PE detection should read only the minimum bytes needed (2 bytes for magic, ~256 bytes for shebang line).

## Implementation Approach

### Architecture

The change is localized to `src-tauri/src/commands/statusbar.rs`. The `run_statusbar_shell_command` function gains a Windows-specific code path:

```
run_statusbar_shell_command (existing)
  |
  +-- [Windows only] resolve_executable()
  |     |
  |     +-- read first 2 bytes
  |     +-- if MZ -> return executable path as-is
  |     +-- else -> parse_shebang()
  |           |
  |           +-- read first line
  |           +-- if #! -> extract interpreter path
  |           +-- else -> return error
  |
  +-- [Windows only] set CREATE_NO_WINDOW on Command
  +-- spawn process
```

### Data Flow

```
Custom command executable path
  -> read 2 bytes from file
  -> MZ? -> spawn executable directly (with CREATE_NO_WINDOW)
  -> not MZ? -> read first line
    -> #! found? -> spawn interpreter with script path as arg (with CREATE_NO_WINDOW)
    -> no #! -> return error
```

### Shebang Parsing Rules

1. Read the first line of the file (up to first `\n` or `\r\n`, max ~256 bytes)
2. Check if it starts with `#!`
3. Strip `#!` prefix and trim whitespace
4. The remaining string is the interpreter path (use as-is, no path conversion, no argument splitting)

### Security

The interpreter path from a shebang is auto-trusted. The security boundary is the `statusbar_custom_commands` setting: only executables explicitly registered by the user are considered. Once a user registers a script, its shebang interpreter is implicitly trusted.

### Dependencies

**Internal Dependencies:**
- `src-tauri/src/commands/statusbar.rs`: Modified file
- `StatusbarCustomCommand` struct in `settings.rs`: No changes needed

**External Dependencies:**
- `std::os::windows::process::CommandExt` for `creation_flags()`

### File Structure

```
src-tauri/src/commands/
  statusbar.rs   # Modified: add Windows shebang logic
```

## Test Scenarios

### Unit Tests
- [ ] PE file detection: file starting with `MZ` bytes is identified as PE
- [ ] Non-PE file detection: file starting with `#!` is identified as script
- [ ] Shebang parsing: `#!C:\Python\python.exe` extracts `C:\Python\python.exe`
- [ ] Shebang parsing with extra whitespace: `#!  C:\Python\python.exe` extracts `C:\Python\python.exe`
- [ ] Missing shebang: non-PE file without `#!` returns error
- [ ] Empty file returns error

### Edge Cases
- [ ] File that cannot be read (permissions, not found) returns appropriate error
- [ ] Binary file that is not PE and has no shebang returns error
- [ ] Shebang line with only `#!` and no path returns error
- [ ] Very long first line (no newline in first 256 bytes): truncate and attempt parse

## Error Handling

| Condition | Behavior |
|-----------|----------|
| File cannot be read | Return error with file path and OS error |
| Non-PE, no shebang | Return error: "No shebang found in script file" |
| Shebang with empty interpreter | Return error: "Empty interpreter path in shebang" |
| Interpreter not found on system | OS-level error propagated from `Command::new().output()` |

## Success Criteria

- [ ] All functional requirements are implemented and tested
- [ ] All unit tests pass
- [ ] Windows-only code is properly gated with `#[cfg(windows)]`
- [ ] Linux behavior is unchanged
- [ ] `CREATE_NO_WINDOW` flag is applied to all Windows process spawns in statusbar

## Open Questions

None.

## References

- `src-tauri/src/commands/statusbar.rs`: Current implementation
- `src-tauri/src/commands/config/settings.rs`: `StatusbarCustomCommand` struct
