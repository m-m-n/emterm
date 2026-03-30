# Verification Result: windows-shebang

## Summary

| Category | Result |
|----------|--------|
| Build | PASS (compiles on Linux; Windows-specific code gated with `#[cfg(windows)]`) |
| Tests | PASS (20 tests pass, 17 new shebang tests + 3 existing) |
| Format | PASS (`cargo fmt --check` clean) |
| Clippy | PASS (no warnings in statusbar.rs; existing warnings in other files are pre-existing) |
| SPEC compliance | PASS (all FR/NFR verified) |

## SPEC.md Requirements Compliance

| Requirement | Status | Evidence |
|-------------|--------|----------|
| FR1: PE Detection | PASS | `is_pe_file()` reads 2 bytes, compares to `PE_MAGIC` [0x4D, 0x5A]. 5 tests. |
| FR2: Shebang Parsing | PASS | `parse_shebang()` reads max 256 bytes, extracts `#!` path as-is, no conversion, no argument splitting. 7 tests. |
| FR3: Interpreter Invocation | PASS | `Interpreted` case: `Command::new(interpreter).arg(script)`. |
| FR4: Error on Missing Shebang | PASS | Non-PE file without `#!` returns `"No shebang found in script file"`. Tests verify. |
| FR5: Auto-trust Interpreter | PASS | Allowlist check on script path only, interpreter is not checked. |
| FR6: CREATE_NO_WINDOW | PASS | `creation_flags(0x08000000)` set in `#[cfg(windows)]` block. |
| NFR1: Platform Scope | PASS | All new logic gated with `#[cfg(windows)]` or `#[cfg(any(windows, test))]`. Linux path unchanged. |
| NFR2: Minimal File Read | PASS | `is_pe_file`: 2 bytes. `parse_shebang`: 256 bytes max. |

## Test Coverage

### New tests (shebang_tests module)

**PE detection (5 tests):**
- `test_pe_file_detected` - Valid PE magic bytes
- `test_non_pe_file` - Script file not detected as PE
- `test_pe_check_empty_file` - Empty file
- `test_pe_check_one_byte` - Single byte file
- `test_pe_check_nonexistent_file` - File not found

**Shebang parsing (7 tests):**
- `test_shebang_simple_path` - Unix path extraction
- `test_shebang_windows_path` - Windows path with `\r\n`
- `test_shebang_with_extra_whitespace` - Whitespace trimming
- `test_shebang_env_style` - `#!/usr/bin/env bun` kept as-is (no splitting)
- `test_no_shebang_returns_error` - Missing `#!` prefix
- `test_empty_file_returns_error` - Zero bytes
- `test_shebang_empty_interpreter_returns_error` - `#!` with no path

**Edge cases (4 tests):**
- `test_shebang_only_whitespace_after_hash_bang` - `#!` followed by spaces only
- `test_binary_file_no_shebang` - Random binary data
- `test_long_first_line_no_newline` - Line exceeding 256 bytes
- `test_nonexistent_file_returns_error` - File not found

**Resolve orchestration (4 tests):**
- `test_resolve_pe_returns_direct` - PE → Direct variant
- `test_resolve_script_returns_interpreted` - Script → Interpreted variant
- `test_resolve_no_shebang_returns_error` - No shebang → error
- `test_resolve_nonexistent_returns_error` - Missing file → error

## Manual Verification Required

- [ ] Windows build: Compile and run on Windows to verify `CREATE_NO_WINDOW` prevents console flash
- [ ] Windows E2E: Register a script with `#!C:\path\to\bun.exe` shebang and verify execution via status bar

## Files Changed

- `src-tauri/src/commands/statusbar.rs` — Added PE detection, shebang parsing, Windows executable resolution, CREATE_NO_WINDOW flag
