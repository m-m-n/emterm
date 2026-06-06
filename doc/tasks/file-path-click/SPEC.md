# Feature: File Path Click-to-Open

## Overview

Enable clicking on file path patterns (e.g., `src/foo.ts:42:10`) in terminal output to open the file in an external editor at the specified line and column. This extends the existing URL detection system to recognize file path patterns with line numbers.

## Objectives

- Detect file paths with line/column numbers in terminal output
- Visually indicate detected paths with underline styling
- Open files in a configurable external editor via Ctrl+click
- Resolve relative paths using the shell's current working directory (OSC 7)

## User Stories

### US1: Open File from AI Tool Output
As a developer using Claude Code, I want to Ctrl+click on file paths like `src/foo.ts:42` in terminal output, so that I can quickly navigate to the referenced code in my editor.

**Acceptance Criteria:**
- [ ] File paths with line numbers are detected and underlined
- [ ] Ctrl+click opens the file in the configured editor at the correct line
- [ ] Relative paths are resolved against the shell's CWD

### US2: Open File from Compiler Error
As a developer, I want to Ctrl+click on file paths in compiler/test error output, so that I can quickly jump to the error location.

**Acceptance Criteria:**
- [ ] Both relative and absolute paths with line numbers are detected
- [ ] Column numbers are passed to the editor when present
- [ ] Works with various compiler output formats

### US3: Configure Editor Command
As a developer, I want to configure which editor command is used when clicking file paths, so that I can use my preferred editor.

**Acceptance Criteria:**
- [ ] Editor command template is configurable in settings
- [ ] Template supports `{file}`, `{line}`, `{col}` placeholders
- [ ] Default is `code --goto {file}:{line}:{col}`

## Technical Requirements

### Functional Requirements

- **FR1:** Detect file path patterns with line numbers in terminal text
  - Relative paths: `src/foo.ts:42`, `./path/file.rs:10:5`
  - Absolute paths: `/home/user/foo.rs:10`, `/home/user/foo.rs:10:5`
  - Must not match URL patterns (`http://`, `https://`)
  - Must not match time patterns (`12:30:45`)
  - No file extension restriction

- **FR2:** Display detected file paths with underline decoration (same as URLs)

- **FR3:** On Ctrl+click (Cmd+click on macOS):
  1. Extract file path, line, and column from the matched pattern
  2. Resolve relative paths using `TerminalState.workingDirectory` (set by OSC 7)
  3. Check file existence via Tauri backend
  4. If exists: execute editor command with placeholders replaced
  5. If not exists: show warning notification

- **FR4:** Settings:
  - `file_path_detection` (boolean, default: `true`): Enable/disable detection
  - `editor_command` (string, default: `code --goto {file}:{line}:{col}`): Editor command template

- **FR5:** File existence check via Tauri command before executing editor command

- **FR6:** Path resolution:
  - Strip `file://` prefix from CWD if present
  - Join CWD + relative path for relative paths
  - Use absolute paths as-is
  - If CWD is empty, pass relative path as-is to editor
  - After the existence check, canonicalize the resolved path to an absolute path before substituting it into the editor command (see Security Considerations)

### Non-Functional Requirements

- **NFR1 - Performance:** File path detection must not impact terminal rendering performance (same order as URL detection)
- **NFR2 - Security:** Sanitize file paths to prevent command injection when constructing editor command
- **NFR3 - Usability:** Detection settings independent from URL detection settings

## Implementation Approach

### Architecture

```
┌─────────────────────────────────────────────────┐
│              Frontend (TypeScript)               │
│                                                  │
│  url-detector.ts ──── detectFilePaths()          │
│       │                    │                     │
│  canvas-renderer.ts ── underline decoration      │
│       │                                          │
│  terminal-app/index.ts ── handleUrlClick()       │
│       │                    │                     │
│  settings/types.ts ── file_path_detection,       │
│                        editor_command            │
├──────────────────────┬──────────────────────────┤
│              Tauri Commands (Rust)                │
│                                                  │
│  check_file_exists(path) → bool                  │
│  open_file_in_editor(program, args) → Result     │
└──────────────────────────────────────────────────┘
```

### Data Flow

```
Terminal Output
    │
    ▼
detectFilePaths(text) ─── returns FilePathMatch[]
    │                      { path, line, col, startCol, endCol }
    ▼
Canvas Renderer ─── underline detected ranges
    │
    ▼ (Ctrl+click)
handleUrlClick()
    │
    ├─ findFilePathAtPosition(text, col)
    │
    ├─ Resolve relative path (CWD from OSC 7)
    │
    ├─ invoke("check_file_exists", { path })
    │       │
    │       ├─ true → parse template, invoke("open_file_in_editor", { program, args })
    │       │
    │       └─ false → show warning notification
    │
    └─ done
```

### Detection Regex

```typescript
// File path with line number (and optional column)
// Matches: src/foo.ts:42, ./path/file.rs:10:5, /home/user/file.py:100
// Does not match: http://..., 12:30:45
const FILE_PATH_REGEX = /(?<![a-zA-Z]:\/\/)(?:\.?\.?\/)?(?:[a-zA-Z0-9_@.-]+\/)*[a-zA-Z0-9_@.-]+\.[a-zA-Z0-9]+:\d+(?::\d+)?/g;
```

Key considerations:
- Negative lookbehind `(?<![a-zA-Z]:\/\/)` to exclude URL protocols
- Must contain at least one `/` or start with `./` or `../` for relative paths, OR be an absolute path starting with `/`
- Must have a file extension (`.` followed by alphanumeric)
- Must have `:line` (and optional `:col`)
- Time patterns excluded because they lack a file extension before the `:`

### File Structure

```
src/
├── terminal/
│   ├── url-detector.ts          # Extended with file path detection
│   └── url-detector.test.ts     # Extended with file path tests
├── terminal-app/
│   └── index.ts                 # Extended handleUrlClick for file paths
├── settings/
│   ├── types.ts                 # Add file_path_detection, editor_command
│   └── settings-sections.ts     # Add File Path Detection UI section
src-tauri/
├── src/
│   ├── commands/
│   │   ├── mod.rs               # Add editor module
│   │   ├── config.rs            # Add file_path_detection, editor_command to AppSettings
│   │   └── editor.rs            # New: check_file_exists, open_file_in_editor
│   └── lib.rs                   # Register new commands
```

### Settings Schema

**Rust** (`settings.rs`):
```rust
pub struct Settings {
    // ... existing fields ...
    #[serde(default = "default_true")]
    pub file_path_detection: bool,

    #[serde(default = "default_editor_command")]
    pub editor_command: String,
}

fn default_editor_command() -> String {
    "code --goto {file}:{line}:{col}".to_string()
}
```

**TypeScript** (`types.ts`):
```typescript
interface AppSettings {
    // ... existing fields ...
    file_path_detection: boolean;
    editor_command: string;
}
```

### Tauri Commands

```rust
#[tauri::command]
async fn check_file_exists(path: String) -> Result<bool, String> {
    Ok(std::path::Path::new(&path).exists())
}

#[tauri::command]
async fn open_file_in_editor(program: String, args: Vec<String>) -> Result<(), String> {
    // Execute program with args as child process (non-blocking)
    // No shell interpretation - args passed directly
}
```

### Dependencies

**Internal Dependencies:**
- `url-detector.ts`: Extend with `detectFilePaths()` and `findFilePathAtPosition()`
- `terminal-app/index.ts`: Extend `handleUrlClick()` to check file paths
- `TerminalState.workingDirectory`: Used for relative path resolution
- Settings system: Add new settings fields
- Notification system: For file-not-found warnings

**External Dependencies:**
- `@tauri-apps/api/core`: `invoke()` for Tauri commands
- `std::process::Command` (Rust): For executing editor command

## Test Scenarios

### Unit Tests (url-detector.ts)

- [ ] Detect `src/foo.ts:42` → path=`src/foo.ts`, line=42, col=1
- [ ] Detect `src/foo.ts:42:10` → path=`src/foo.ts`, line=42, col=10
- [ ] Detect `/home/user/file.rs:10` → absolute path
- [ ] Detect `./src/foo.ts:42` → relative path with `./`
- [ ] Detect `../lib/bar.py:5:3` → relative path with `../`
- [ ] Do NOT detect `http://example.com:8080` → URL, not file path
- [ ] Do NOT detect `https://example.com/path:443` → URL
- [ ] Do NOT detect `12:30:45` → time pattern
- [ ] Do NOT detect `foo.ts` → no line number
- [ ] Detect path at end of line: `error in src/foo.ts:42`
- [ ] Detect path at start of line: `src/foo.ts:42: error msg`
- [ ] Detect multiple paths on one line
- [ ] `findFilePathAtPosition()` returns correct match at column

### Unit Tests (Rust commands)

- [ ] `check_file_exists` returns true for existing file
- [ ] `check_file_exists` returns false for non-existing file
- [ ] `open_file_in_editor` parses command with spaces correctly

### Integration Tests

- [ ] Settings save/load with new fields
- [ ] Click handler delegates to file path handler when URL not found

## Security Considerations

- **Command Injection:** File paths must be sanitized before being inserted into the editor command template. Use proper argument escaping or pass arguments as an array rather than shell string. Canonicalize the path to an absolute path before substitution so a path beginning with `-` is not interpreted as an editor option.
- **Path Traversal:** Not a significant concern since we're opening files in an editor (read-only navigation, not file modification).
- **Input Validation:** Validate that line and column numbers are positive integers.

## Error Handling

| Error | Condition | Handling |
|-------|-----------|---------|
| File not found | Resolved path does not exist | Show warning notification |
| Editor command failed | Command execution error | Log error, show notification |
| Editor command empty | User cleared the setting | Ignore click (no action) |
| CWD unknown | OSC 7 not received | Pass relative path as-is |

## Success Criteria

- [ ] All file path patterns are correctly detected and underlined
- [ ] Ctrl+click opens the correct file at the correct line in the configured editor
- [ ] Relative paths resolve correctly against shell CWD
- [ ] File-not-found shows a user-friendly warning
- [ ] Settings UI allows configuring detection and editor command
- [ ] No false positives on URLs or time patterns
- [ ] All unit tests pass
- [ ] No command injection vulnerabilities
