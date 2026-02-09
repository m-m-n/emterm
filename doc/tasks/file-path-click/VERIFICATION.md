# Verification Document: File Path Click-to-Open

## Overview
**Feature**: File Path Click-to-Open
**SPEC.md**: `doc/tasks/file-path-click/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/file-path-click/IMPLEMENTATION.md`

## Build Verification

### Build Command
```bash
# Rust build
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo build --manifest-path src-tauri/Cargo.toml"

# TypeScript type check
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun run typecheck"
```

### Expected Result
- Exit code: 0
- No error messages
- No type errors

## Test Verification

### Test Command
```bash
# Rust tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo test --manifest-path src-tauri/Cargo.toml"

# TypeScript tests
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "bun test"
```

### Coverage Target
- **Minimum**: 80%
- **Target**: 90% for url-detector.ts

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | Detect `src/foo.ts:42` | path=`src/foo.ts`, line=42, col=1 | Unit |
| TS-02 | Detect `src/foo.ts:42:10` | path=`src/foo.ts`, line=42, col=10 | Unit |
| TS-03 | Detect `/home/user/file.rs:10` | Absolute path detected | Unit |
| TS-04 | Detect `./src/foo.ts:42` | Relative path with `./` | Unit |
| TS-05 | Detect `../lib/bar.py:5:3` | Relative path with `../` | Unit |
| TS-06 | Do NOT detect `http://example.com:8080` | No match (URL) | Unit |
| TS-07 | Do NOT detect `https://example.com/path:443` | No match (URL) | Unit |
| TS-08 | Do NOT detect `12:30:45` | No match (time) | Unit |
| TS-09 | Do NOT detect `foo.ts` | No match (no line number) | Unit |
| TS-10 | Detect path at end of line | Match found in `error in src/foo.ts:42` | Unit |
| TS-11 | Detect path at start of line | Match found in `src/foo.ts:42: error msg` | Unit |
| TS-12 | Detect multiple paths on one line | Both detected | Unit |
| TS-13 | `findFilePathAtPosition()` correct | Returns match at clicked column | Unit |
| TS-14 | `check_file_exists` existing file | Returns true | Unit (Rust) |
| TS-15 | `check_file_exists` non-existing file | Returns false | Unit (Rust) |
| TS-16 | `open_file_in_editor` command parsing | Parses correctly | Unit (Rust) |
| TS-17 | Settings save/load with new fields | Values preserved | Integration |

## Code Quality Verification

### Format Check
```bash
# TypeScript (no formatter configured, skip)
# Rust
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo fmt --manifest-path src-tauri/Cargo.toml -- --check"
```

### Static Analysis
```bash
docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings"
```

## File Structure Verification

### Files to Create
- `src-tauri/src/commands/editor.rs` - Tauri commands for file existence check and editor execution

### Files to Modify
- `src/terminal/url-detector.ts` - Add file path detection functions
- `src/terminal/url-detector.test.ts` - Add file path detection tests
- `src/terminal-app/index.ts` - Extend handleUrlClick() for file paths
- `src/terminal/canvas-renderer.ts` - Add underline rendering for file paths
- `src/settings/types.ts` - Add file_path_detection, editor_command fields
- `src/settings/settings-sections.ts` - Add File Path Detection settings UI
- `src/i18n/locales/en.json` - Add English translation keys
- `src/i18n/locales/ja.json` - Add Japanese translation keys
- `src-tauri/src/commands/mod.rs` - Register editor module
- `src-tauri/src/commands/config.rs` - Add settings fields to AppSettings
- `src-tauri/src/lib.rs` - Register new Tauri commands

## SPEC.md Compliance

### Success Criteria

| ID | Criterion from SPEC.md | How to Verify |
|----|------------------------|---------------|
| SC-01 | All file path patterns correctly detected and underlined | Unit tests TS-01 through TS-12 + manual visual check |
| SC-02 | Ctrl+click opens correct file at correct line in configured editor | Manual test: Ctrl+click on path → verify editor opens |
| SC-03 | Relative paths resolve correctly against shell CWD | Unit test for path resolution + manual test |
| SC-04 | File-not-found shows user-friendly warning | Manual test: click on non-existing path |
| SC-05 | Settings UI allows configuring detection and editor command | Manual test: verify settings panel |
| SC-06 | No false positives on URLs or time patterns | Unit tests TS-06, TS-07, TS-08 |
| SC-07 | All unit tests pass | Automated: `bun test` + `cargo test` |
| SC-08 | No command injection vulnerabilities | Code review: verify no shell execution |

### Functional Requirements Coverage

| Requirement | Implementation Phase | Verification |
|-------------|---------------------|--------------|
| FR1 - Detect file path patterns | Phase 1 | Unit tests TS-01 to TS-12 |
| FR2 - Underline decoration | Phase 3 | Manual visual verification |
| FR3 - Ctrl+click behavior | Phase 3 | Manual test + code review |
| FR4 - Settings fields | Phase 2 | Unit test TS-17 + manual UI check |
| FR5 - File existence check | Phase 2 | Unit tests TS-14, TS-15 |
| FR6 - Path resolution | Phase 3 | Unit test + manual test |

## E2E Testing (Docker)

### Setup
- Compose: `docker-compose.e2e.yml`
- Run: `docker compose -f docker-compose.e2e.yml run --rm --no-deps build sh -c "..."`

### Basic Functionality
- [ ] Rust build succeeds
- [ ] TypeScript type check passes
- [ ] All Rust unit tests pass
- [ ] All TypeScript unit tests pass

### Edge Cases
- [ ] Settings with missing new fields deserialize correctly (backward compat)
- [ ] Empty editor_command field handled gracefully

## Manual Testing (E2E Not Possible)

Items requiring Tauri desktop environment:

### Visual Verification
- [ ] File paths with line numbers are displayed with underline in terminal
- [ ] URLs are still displayed with underline (no regression)
- [ ] Underline does not appear when file_path_detection is disabled

### Click Behavior
- [ ] Ctrl+click on `src/foo.ts:42` opens VS Code at line 42
- [ ] Ctrl+click on `/absolute/path/file.rs:10:5` opens at line 10, col 5
- [ ] Ctrl+click on URL still opens browser (no regression)
- [ ] Ctrl+click on non-existing file shows warning notification
- [ ] Plain click (no modifier) does not trigger file open
- [ ] Feature disabled: Ctrl+click on file path does nothing

### Settings UI
- [ ] File Path Detection toggle appears in Terminal section
- [ ] Editor Command text input appears with default value
- [ ] Changing editor command takes effect on next click
- [ ] Toggling detection off hides underlines

### Path Resolution
- [ ] Relative path `src/foo.ts:42` resolves from shell CWD
- [ ] After `cd` in terminal, relative paths resolve from new CWD
- [ ] Absolute path `/home/user/file.ts:10` works regardless of CWD

## Security Verification

### Security Checks
- [ ] Editor command is split into program + args (not passed through shell)
- [ ] File path with spaces handled correctly (no injection)
- [ ] File path with special characters (`;`, `|`, `&`) does not cause command injection
- [ ] Line and column numbers validated as positive integers

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 2 | - | ✅ | - |
| Tests | 17 | ✅ | ✅ | - |
| Code Quality | 2 | - | ✅ | - |
| File Structure | 12 | ✅ | - | - |
| SPEC Compliance | 8 | Partial | - | ✅ |
| E2E Testing | 4 | - | ✅ | - |
| Manual Testing | 14 | - | - | ✅ |
| Security | 4 | - | - | ✅ |

**Total**: 17 automated test items, 8 E2E (Docker) items, 18 manual items
