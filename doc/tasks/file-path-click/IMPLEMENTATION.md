# Implementation Plan: File Path Click-to-Open

## Overview

Extend the existing URL detection and Ctrl+click system to recognize file path patterns with line/column numbers (e.g., `src/foo.ts:42:10`) and open them in a configurable external editor.

## Objectives

- Add file path pattern detection alongside existing URL detection
- Render detected file paths with underline decoration
- Execute configurable editor commands on Ctrl+click with path resolution
- Add settings for file path detection toggle and editor command template

## Prerequisites

### Development Environment
- Rust toolchain (for Tauri backend)
- Bun (for TypeScript frontend)
- Tauri CLI

### Dependencies
- No new external dependencies required
- Uses existing `@tauri-apps/api/core` for `invoke()`
- Uses existing `std::process::Command` in Rust

### Knowledge Requirements
- Existing URL detection system (`url-detector.ts`)
- Existing Ctrl+click handler (`terminal-app/index.ts`)
- Settings pattern (Rust `serde(default)` + TypeScript `AppSettings`)
- Canvas renderer line rendering pipeline

## Architecture Overview

### Technology Stack
- **Frontend**: Vanilla TypeScript
- **Backend**: Rust (Tauri)
- **Rendering**: Canvas-based terminal renderer

### Design Approach
Extend existing URL detection module with parallel file path detection. The click handler first checks for URL matches (existing behavior), then falls back to file path matches. File paths are underlined during rendering using the same mechanism as ANSI underline attributes but applied by the renderer based on detection results.

### Component Interaction
```
url-detector.ts
  ├─ detectUrls() [existing]
  └─ detectFilePaths() [new]
       │
canvas-renderer.ts
  └─ renderLineText() → applies underline for detected ranges
       │
terminal-app/index.ts
  └─ handleUrlClick() → checks URL first, then file path
       │                  │
       │                  ├─ resolves relative path via state.workingDirectory
       │                  └─ invokes Tauri commands
       │
commands/editor.rs [new]
  ├─ check_file_exists()
  └─ open_file_in_editor()
```

## Implementation Phases

### Phase 1: File Path Detection Engine

**Goal**: Detect file path patterns in terminal text and expose detection API

**Files to Create**:
- None (extend existing file)

**Files to Modify**:
- `src/terminal/url-detector.ts`: Add `FilePathMatch` interface, `detectFilePaths()`, `findFilePathAtPosition()`
- `src/terminal/url-detector.test.ts`: Add comprehensive file path detection tests

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| FilePathMatch | Data structure for detected file path with line/col | Valid text input | Contains path, line, col, startCol, endCol |
| detectFilePaths() | Scan text line for file path patterns | Text string | Array of FilePathMatch with positions |
| findFilePathAtPosition() | Find file path at specific column | Text string + column index | FilePathMatch or null |

**Processing Flow**:
```
1. Receive text line
2. Apply regex to find file path patterns
   ├─ Must have file extension + colon + digits
   ├─ Must NOT be preceded by URL protocol (://)
   └─ Extracts: path, line number, optional column number
3. For each match, parse path/line/col components
4. Return array of FilePathMatch objects
```

**Implementation Steps**:

1. **Define FilePathMatch interface**
   - Similar to UrlMatch but with additional `path`, `line`, `col` fields

2. **Implement detectFilePaths()**
   - Regex-based detection with negative lookbehind for URL protocols
   - Parse matched string into path, line, column components
   - Handle edge cases: trailing punctuation, embedded in text

3. **Implement findFilePathAtPosition()**
   - Column-based lookup similar to findUrlAtPosition()

**Dependencies**:
- Requires: None
- Blocks: Phase 3

**Testing Approach**:

*Unit Tests*:

| ID | Scenario | Input | Expected |
|----|----------|-------|----------|
| FP-01 | Relative path with line | `src/foo.ts:42` | path=`src/foo.ts`, line=42, col=1 |
| FP-02 | Relative path with line+col | `src/foo.ts:42:10` | path=`src/foo.ts`, line=42, col=10 |
| FP-03 | Absolute path | `/home/user/file.rs:10` | path=`/home/user/file.rs`, line=10 |
| FP-04 | Dot-relative path | `./src/foo.ts:42` | path=`./src/foo.ts`, line=42 |
| FP-05 | Parent-relative path | `../lib/bar.py:5:3` | path=`../lib/bar.py`, line=5, col=3 |
| FP-06 | URL exclusion (http) | `http://example.com:8080` | No match |
| FP-07 | URL exclusion (https) | `https://example.com/path:443` | No match |
| FP-08 | Time pattern exclusion | `12:30:45` | No match |
| FP-09 | No line number | `foo.ts` | No match |
| FP-10 | Path at end of line | `error in src/foo.ts:42` | Match found |
| FP-11 | Path at start of line | `src/foo.ts:42: error msg` | Match found |
| FP-12 | Multiple paths on line | Two paths | Both detected |
| FP-13 | findFilePathAtPosition | Click on path | Correct match returned |
| FP-14 | findFilePathAtPosition miss | Click outside path | null returned |

**Acceptance Criteria**:
- [ ] All file path patterns from SPEC are correctly detected
- [ ] No false positives on URLs or time patterns
- [ ] Position-based lookup works correctly

**Estimated Effort**: 小

---

### Phase 2: Settings and Tauri Commands

**Goal**: Add settings fields and backend commands for file existence check and editor execution

**Files to Create**:
- `src-tauri/src/commands/editor.rs`: New Tauri commands for file operations

**Files to Modify**:
- `src-tauri/src/commands/mod.rs`: Register editor module
- `src-tauri/src/commands/config.rs`: Add `file_path_detection` and `editor_command` fields to AppSettings
- `src-tauri/src/lib.rs`: Register new Tauri commands
- `src/settings/types.ts`: Add TypeScript fields to AppSettings interface
- `src/settings/settings-sections.ts`: Add File Path Detection settings UI
- `src/i18n/locales/en.json`: Add English translation keys
- `src/i18n/locales/ja.json`: Add Japanese translation keys
- `src-tauri/locales/en.json`: Add Rust-side English translations (if validation needed)
- `src-tauri/locales/ja.json`: Add Rust-side Japanese translations (if validation needed)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| AppSettings.file_path_detection | Toggle file path detection | Settings loaded | Boolean indicating feature state |
| AppSettings.editor_command | Editor command template | Settings loaded | Template string with placeholders |
| check_file_exists | Verify file exists on disk | Absolute path provided | Boolean result |
| open_file_in_editor | Execute editor command | Program name + args array | Editor process launched |

**Processing Flow**:
```
Settings Addition:
1. Add fields to Rust AppSettings with serde defaults
2. Add fields to TypeScript AppSettings interface
3. Add UI controls in settings section
4. Follow existing pattern: serde(default) + deserialize_null_with/default

Tauri Commands:
1. check_file_exists: receive path → check existence → return bool
2. open_file_in_editor: receive program + args array → spawn process (no shell)
```

**Implementation Steps**:

1. **Add Rust settings fields**
   - `file_path_detection`: bool, default true
   - `editor_command`: String, default `code --goto {file}:{line}:{col}`
   - Follow existing `serde(default)` + `deserialize_null_with!` pattern

2. **Create editor commands module**
   - `check_file_exists`: path existence check
   - `open_file_in_editor`: receive program + args array, spawn process directly (no shell)
   - Security: frontend parses template into program + args, backend receives already-split arguments

3. **Add TypeScript settings fields**
   - Mirror Rust struct in `AppSettings` interface
   - Add settings UI section with toggle and text input

4. **Add i18n keys**
   - English and Japanese labels for new settings

**Dependencies**:
- Requires: None (parallel with Phase 1)
- Blocks: Phase 3

**Testing Approach**:

*Unit Tests (Rust)*:

| ID | Scenario | Expected |
|----|----------|----------|
| RS-01 | check_file_exists for existing file | Returns true |
| RS-02 | check_file_exists for non-existing file | Returns false |
| RS-03 | open_file_in_editor with valid command | Process spawned successfully |
| RS-04 | Settings default values | file_path_detection=true, editor_command=default |
| RS-05 | Settings deserialization with null | Falls back to defaults |

*Unit Tests (TypeScript)*:

| ID | Scenario | Expected |
|----|----------|----------|
| TS-01 | Settings mock includes new fields | No type errors |

**Acceptance Criteria**:
- [ ] Settings save and load correctly with new fields
- [ ] Backward compatible with existing settings files (missing fields get defaults)
- [ ] Tauri commands execute correctly
- [ ] Settings UI renders correctly

**Estimated Effort**: 小

---

### Phase 3: Click Handler and Underline Rendering

**Goal**: Integrate file path detection into click handling and visual rendering

**Files to Modify**:
- `src/terminal-app/index.ts`: Extend `handleUrlClick()` to handle file paths
- `src/terminal/canvas-renderer.ts`: Add underline rendering for detected file paths

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| handleUrlClick (extended) | Handle Ctrl+click for URLs and file paths | Click event with modifier | URL opened or file opened in editor |
| renderLineText (extended) | Apply underline to detected file paths | Line text available | File paths rendered with underline |

**Processing Flow**:
```
Click Handler Extension:
1. Existing URL check (findUrlAtPosition)
   ├─ URL found → open URL (existing behavior)
   └─ URL not found → check file path
2. Check file_path_detection setting
3. findFilePathAtPosition(text, col)
   ├─ No match → do nothing
   └─ Match found:
       a. Resolve path (relative → absolute using CWD)
       b. invoke("check_file_exists", { path: resolvedPath })
          ├─ false → show warning notification
          └─ true → parse template into program + args
              → invoke("open_file_in_editor", { program, args })

Underline Rendering:
1. During renderLineText, detect file paths in line text
2. For each detected file path range, apply underline decoration
3. Respect file_path_detection setting
```

**Implementation Steps**:

1. **Extend handleUrlClick()**
   - After URL check fails, attempt file path detection
   - Resolve relative paths using `state.workingDirectory`
   - Strip `file://` prefix from CWD if present
   - Call Tauri commands for existence check and editor launch
   - Show notification on file not found or command error

2. **Add underline rendering for file paths**
   - In `renderLineText()` or `renderSpanText()`, detect file paths in line text
   - Draw underline for detected ranges using existing `drawUnderline()` method
   - Cache detection results per line to avoid re-detection during rendering
   - Respect `file_path_detection` setting

3. **Path resolution logic**
   - If path starts with `/`: use as-is (absolute)
   - If CWD available and path is relative: join CWD + path
   - If CWD empty: pass relative path as-is
   - Handle `file://` prefix in CWD (strip before joining)

**Dependencies**:
- Requires: Phase 1 (detection functions), Phase 2 (settings and commands)
- Blocks: None

**Testing Approach**:

*Integration Tests*:

| ID | Scenario | Expected |
|----|----------|----------|
| IT-01 | Click on file path triggers editor | Editor command invoked via Tauri |
| IT-02 | Click on URL still opens browser | Existing behavior preserved |
| IT-03 | File not found shows warning | Notification displayed |
| IT-04 | Feature disabled via settings | Click does nothing |
| IT-05 | Empty editor command | Click does nothing |
| IT-06 | Relative path resolution with CWD | Correct absolute path passed |
| IT-07 | Relative path without CWD | Relative path passed as-is |

*Manual Testing (E2E)*:
- [ ] Ctrl+click on `src/foo.ts:42` opens VS Code at correct line
- [ ] File paths in terminal output are visually underlined
- [ ] Settings changes take effect immediately
- [ ] Works with different editor commands (vim, emacs, etc.)

**Acceptance Criteria**:
- [ ] Ctrl+click opens file in configured editor at correct line
- [ ] Relative paths resolved correctly against shell CWD
- [ ] File-not-found shows warning notification
- [ ] URLs still work as before
- [ ] Underline displayed for detected file paths
- [ ] Feature can be toggled independently from URL detection

**Estimated Effort**: 中

---

## Complete File Structure

```
src/
├── terminal/
│   ├── url-detector.ts          # Add FilePathMatch, detectFilePaths(), findFilePathAtPosition()
│   └── url-detector.test.ts     # Add file path detection tests
├── terminal-app/
│   └── index.ts                 # Extend handleUrlClick() for file paths
├── settings/
│   ├── types.ts                 # Add file_path_detection, editor_command
│   └── settings-sections.ts     # Add File Path Detection UI section
├── i18n/
│   └── locales/
│       ├── en.json              # Add translation keys
│       └── ja.json              # Add translation keys
src-tauri/
├── src/
│   ├── commands/
│   │   ├── mod.rs               # Add editor module
│   │   ├── config.rs            # Add settings fields
│   │   └── editor.rs            # New: check_file_exists, open_file_in_editor
│   └── lib.rs                   # Register new commands
├── locales/
│   ├── en.json                  # Add backend translation keys (if needed)
│   └── ja.json                  # Add backend translation keys (if needed)
```

## Testing Strategy

### Unit Testing

**Approach**:
- TypeScript: Bun test runner
- Rust: cargo test

**Test Coverage Goals**:
- File path detection: 90%+ (critical for avoiding false positives)
- Tauri commands: 80%+
- Settings: Covered by existing test patterns

### E2E Testing (Docker)

- [ ] Build succeeds with new code
- [ ] TypeScript type check passes
- [ ] All unit tests pass
- [ ] Settings save/load with new fields

### Manual Testing (E2E Not Possible)

- [ ] Visual underline appears on file paths in terminal
- [ ] Ctrl+click opens file in editor
- [ ] File-not-found warning appears for missing files
- [ ] Settings UI works correctly

## Dependencies

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1 + Phase 2 (can be parallel, no inter-dependency)
2. Phase 3 (depends on both Phase 1 and Phase 2)

### Component Dependencies
- `url-detector.ts` file path functions → used by `canvas-renderer.ts` and `terminal-app/index.ts`
- `commands/editor.rs` → registered in `lib.rs`, invoked from frontend
- Settings fields → used throughout frontend for feature toggle

## Risk Assessment

### Technical Risks

1. **Regex False Positives**
   - **Risk**: File path regex matches unintended patterns
   - **Likelihood**: Medium
   - **Impact**: Medium (user annoyance)
   - **Mitigation**: Comprehensive test suite, negative lookbehind for URLs, require file extension

2. **Command Injection via File Path**
   - **Risk**: Malicious file paths could execute arbitrary commands
   - **Likelihood**: Low (requires attacker-controlled terminal output)
   - **Impact**: High (arbitrary code execution)
   - **Mitigation**: Split command template into program + args array, do not pass through shell

3. **Rendering Performance**
   - **Risk**: File path detection on every line slows down rendering
   - **Likelihood**: Low (regex is fast)
   - **Impact**: Medium (typing latency)
   - **Mitigation**: Cache detection results per line, skip detection when feature disabled

## Security Considerations

- Editor command must be split into program and arguments without shell interpretation
- File paths with special characters must be properly handled
- Line and column numbers validated as positive integers

## Success Criteria

- [ ] All file path patterns correctly detected and underlined
- [ ] Ctrl+click opens correct file at correct line in configured editor
- [ ] Relative paths resolve correctly against shell CWD
- [ ] File-not-found shows user-friendly warning
- [ ] Settings UI allows configuring detection and editor command
- [ ] No false positives on URLs or time patterns
- [ ] All unit tests pass
- [ ] No command injection vulnerabilities

## References

- **Specification**: `doc/tasks/file-path-click/SPEC.md`
- **Requirements**: `doc/tasks/file-path-click/要件定義書.md`
- **Existing URL Detection**: `src/terminal/url-detector.ts`
- **Existing Click Handler**: `src/terminal-app/index.ts` (lines 610-643)
- **Settings Pattern**: `src-tauri/src/commands/config.rs`
