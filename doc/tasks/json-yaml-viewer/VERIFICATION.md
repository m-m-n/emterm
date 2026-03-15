# Verification Document: JSON/YAML Viewer

## Overview
**Feature**: JSON/YAML Viewer
**SPEC.md**: `doc/tasks/json-yaml-viewer/SPEC.md`
**IMPLEMENTATION.md**: `doc/tasks/json-yaml-viewer/IMPLEMENTATION.md`

## Build Verification
- Command: `bun tauri build`
- Expected: exit code 0, no errors

## Test Verification
- Command: `cargo test --manifest-path src-tauri/Cargo.toml && bun test`
- Coverage target: minimum 80%, target 90% for core logic

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-01 | JSON parser: valid JSON | Parsed object returned | Unit |
| TS-02 | JSON parser: invalid JSON | Error with message | Unit |
| TS-03 | YAML parser: valid YAML | Parsed object returned | Unit |
| TS-04 | YAML parser: invalid YAML | Error with message | Unit |
| TS-05 | Tree builder: nested object | Correct tree structure with depth/path | Unit |
| TS-06 | Tree builder: arrays | Index-based keys ([0], [1], ...) | Unit |
| TS-07 | Pretty-print: minified JSON | Formatted output with indentation | Unit |
| TS-08 | Highlighter: JSON tokens | Correct color classes for each type | Unit |
| TS-09 | Highlighter: YAML tokens | Correct color classes for each type | Unit |
| TS-10 | CLI: emterm json produces OSC | Correct OSC 777 json sequences | Integration |
| TS-11 | CLI: emterm yaml produces OSC | Correct OSC 777 yaml sequences | Integration |
| TS-12 | Session manager: begin/chunk/end | Data assembled and parsed | Integration |
| TS-13 | Empty JSON object {} | Tree with no children, empty detail | Unit |
| TS-14 | Empty JSON array [] | Tree with no children, empty detail | Unit |
| TS-15 | Empty YAML document | Handled gracefully | Unit |
| TS-16 | Deeply nested structure (100+ levels) | Tree renders without crash | Unit |
| TS-17 | JSON with Unicode characters | Correct display | Unit |
| TS-18 | YAML with anchors and aliases | Resolved correctly | Unit |
| TS-19 | YAML with multi-line strings | Preserved correctly | Unit |
| TS-20 | Copy button extracts correct text | Clipboard content matches display | Unit |

## Code Quality Verification
- Format: (no project-wide formatter configured)
- Static analysis: `bun run typecheck`

## File Structure Verification

### Files to Create
- `src-tauri/src/commands/json.rs` - JSON CLI command executor
- `src-tauri/src/commands/yaml.rs` - YAML CLI command executor
- `src/data-viewer/types.ts` - Type definitions
- `src/data-viewer/session.ts` - DataViewerSessionManager
- `src/data-viewer/parser.ts` - JSON/YAML parser
- `src/data-viewer/highlighter.ts` - Syntax highlighter
- `src/data-viewer/raw-view.ts` - RAW display component
- `src/data-viewer/outline.ts` - Outline (tree + detail)
- `src/data-viewer/tree-builder.ts` - Tree node builder
- `src/data-viewer/fullscreen.ts` - Fullscreen overlay controller
- `src/styles/data-viewer.css` - Viewer styles

### Files to Modify
- `src-tauri/src/main.rs` - Register json/yaml subcommands
- `src-tauri/src/commands/mod.rs` - Export new modules
- `src-tauri/src/encoding/osc.rs` - Add generate_json_osc, generate_yaml_osc
- `src-tauri/locales/en.json` - CLI help text
- `src-tauri/locales/ja.json` - CLI help text
- `src/terminal/state.ts` - Add DataViewerSessionManager
- `src/terminal/handlers/osc_handlers.ts` - Route json/yaml commands
- `src/terminal/handlers/types.ts` - Add getDataViewerManager
- `src/terminal-app/index.ts` - Wire container and IME callbacks
- `package.json` - Add YAML parsing dependency

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-01 | All FR (FR1-FR10) implemented and tested | Run test suite, verify all pass |
| SC-02 | Keyboard shortcuts work as specified | E2E test: invoke each shortcut |
| SC-03 | Syntax highlighting visually clear | Manual visual inspection |
| SC-04 | Error handling for invalid files | Unit test: parse error path |
| SC-05 | tmux passthrough works | Manual test inside tmux |
| SC-06 | No Markdown viewer regression | Run existing markdown E2E tests |

### Functional Requirements Coverage

| Requirement | Phase | Verification |
|-------------|-------|--------------|
| FR1: CLI commands | Phase 1 | Integration test: CLI output |
| FR2: OSC 777 sequences | Phase 1 | Unit test: sequence format |
| FR3: Frontend parsing + overlay | Phase 2 | Unit + integration tests |
| FR4: Outline view | Phase 4 | Unit test: tree builder; E2E: display |
| FR5: RAW view + highlighting + copy | Phase 3 | Unit test: highlighter; E2E: display |
| FR6: Toggle outline/RAW | Phase 5 | E2E test: r key toggle |
| FR7: JSON pretty-print | Phase 3+5 | Unit test: formatter; E2E: p key |
| FR8: Syntax highlighting | Phase 3 | Unit test: token classification |
| FR9: Parse error handling | Phase 2+5 | Unit test: error path; E2E: display |
| FR10: Copy button | Phase 3 | Unit test: clipboard content |

## E2E Testing (Docker)

- [ ] `emterm json` opens viewer with outline display
- [ ] `emterm yaml` opens viewer with outline display
- [ ] Tree navigation with arrow keys updates detail pane
- [ ] `r` key toggles between outline and RAW view
- [ ] `p` key toggles JSON pretty-print in RAW mode
- [ ] `p` key has no effect in YAML RAW mode
- [ ] Escape closes viewer and returns to terminal
- [ ] Invalid JSON shows error banner + RAW text
- [ ] Invalid YAML shows error banner + RAW text
- [ ] Copy button copies RAW content to clipboard
- [ ] Existing markdown E2E tests still pass

## Manual Testing (E2E Not Possible)

- [ ] Syntax highlighting colors are visually distinct and readable
- [ ] Outline tree indentation is clear for deep nesting
- [ ] Two-pane layout proportions are reasonable
- [ ] Status bar text is legible
- [ ] Large file (10MB+) opens without excessive delay
- [ ] tmux DCS passthrough works correctly
- [ ] Works on both Linux and Windows

## Performance Verification
- No file size limit: 10MB+ file opens without crash
- Outline rendering: completes within 1 second for typical files (< 1MB)

## Security Verification
- [ ] HTML output sanitized via DOMPurify (no raw innerHTML without sanitization)
- [ ] No external network requests from viewer
- [ ] File read errors do not leak sensitive path information

## Verification Summary

| Category | Items | Automated | E2E (Docker) | Manual |
|----------|-------|-----------|--------------|--------|
| Build | 1 | 1 | 0 | 0 |
| Unit Tests | 20 | 20 | 0 | 0 |
| Code Quality | 1 | 1 | 0 | 0 |
| File Structure | 21 | 1 | 0 | 0 |
| SPEC Compliance | 6 | 3 | 2 | 1 |
| FR Coverage | 10 | 6 | 4 | 0 |
| E2E Scenarios | 11 | 0 | 11 | 0 |
| Manual Scenarios | 7 | 0 | 0 | 7 |
| Performance | 2 | 0 | 0 | 2 |
| Security | 3 | 1 | 0 | 2 |
| **Total** | **82** | **33** | **17** | **12** |
