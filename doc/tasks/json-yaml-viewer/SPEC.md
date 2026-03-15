# Feature: JSON/YAML Viewer

## Overview

Add JSON and YAML viewer functionality to eMterm, following the same architecture as the existing Markdown viewer. Users can view structured data files with an interactive outline panel (tree navigation + detail view) and a RAW display mode, toggled with keyboard shortcuts.

## Objectives

- Provide `emterm json` and `emterm yaml` CLI commands for viewing structured data
- Display JSON/YAML with a two-pane outline view (tree + detail) as the default
- Support RAW view toggle and JSON pretty-print toggle
- Apply syntax highlighting for keys, strings, numbers, booleans, and null values

## User Stories

### US1: View JSON File with Outline
As a developer, I want to view a JSON file with an outline panel, so that I can navigate the structure and inspect individual keys.

**Acceptance Criteria:**
- [ ] `emterm json <file>` opens the viewer in outline mode
- [ ] Left pane shows a fully expanded key tree
- [ ] Selecting a key shows its value (in JSON format) in the right pane
- [ ] Syntax highlighting is applied

### US2: View YAML File with Outline
As a developer, I want to view a YAML file with an outline panel, so that I can navigate the structure and inspect individual keys.

**Acceptance Criteria:**
- [ ] `emterm yaml <file>` opens the viewer in outline mode
- [ ] Left pane shows a fully expanded key tree
- [ ] Selecting a key shows its value (in YAML format) in the right pane
- [ ] Syntax highlighting is applied

### US3: Toggle Between Outline and RAW View
As a developer, I want to toggle between outline and RAW view, so that I can see both the structure and the full file content.

**Acceptance Criteria:**
- [ ] Pressing `r` switches from outline to RAW view
- [ ] Pressing `r` again switches back to outline view
- [ ] RAW view shows the full file content with syntax highlighting
- [ ] RAW view includes a copy button

### US4: Pretty-Print JSON in RAW View
As a developer, I want to pretty-print JSON in RAW view, so that I can see minified JSON in a readable format.

**Acceptance Criteria:**
- [ ] Pressing `p` in JSON RAW view toggles pretty-print formatting
- [ ] `p` key has no effect in YAML RAW view or outline view

### US5: View Invalid JSON/YAML
As a developer, I want to open an invalid JSON/YAML file, so that I can still see the raw content with an error message.

**Acceptance Criteria:**
- [ ] Viewer opens with an error message at the top
- [ ] File content is displayed as raw text
- [ ] Outline view is unavailable (tree cannot be built)

## Technical Requirements

### Functional Requirements
- **FR1:** CLI commands `emterm json <file>` and `emterm yaml <file>` read a file, base64-encode it, chunk it into 128KB segments, and output OSC 777 sequences to stdout
- **FR2:** OSC 777 sequences use separate commands: `emterm;json;begin/chunk/end` and `emterm;yaml;begin/chunk/end`
- **FR3:** Frontend parses received data as JSON or YAML and displays a fullscreen overlay viewer
- **FR4:** Outline view: left pane shows a fully expanded key tree (no folding); right pane shows the selected key's value in the original format (JSON or YAML) with syntax highlighting
- **FR5:** RAW view: displays the entire file content with syntax highlighting and a copy button
- **FR6:** `r` key toggles between outline and RAW view
- **FR7:** `p` key toggles JSON pretty-print in RAW view (JSON only)
- **FR8:** Syntax highlighting for: keys, strings, numbers, booleans, null
- **FR9:** Parse errors: show error message banner + RAW text fallback; outline view disabled
- **FR10:** Copy button in RAW view copies content to clipboard via Tauri clipboard plugin

### Non-Functional Requirements
- **NFR1 - Performance:** No file size limit; outline rendering should complete within 1 second for typical files
- **NFR2 - Security:** HTML sanitization via DOMPurify for any rendered content (consistent with Markdown viewer)
- **NFR3 - Platform:** Linux and Windows support; tmux DCS passthrough support
- **NFR4 - Architecture:** Follow the same pattern as the Markdown viewer (OSC 777 + session manager + fullscreen overlay)

## Implementation Approach

### Architecture

**Data Flow:**
```
CLI (emterm json/yaml <file>)
  → Read file, Base64 encode, chunk (128KB)
  → Generate OSC 777 sequences (begin/chunk/end)
  → stdout (with tmux DCS wrapping if needed)
  → WASM parser receives OSC 777
  → Fire callback to TypeScript (action type 100)
  → OSC handler dispatches to DataViewerSessionManager
  → Session assembles chunks, decodes Base64
  → Parse JSON/YAML in frontend
  → Display in fullscreen overlay (outline or RAW mode)
```

**Component Diagram:**
```
┌─────────────────────────────────────────────────────┐
│ Rust Backend                                        │
│  ├─ commands/json.rs    (CLI: emterm json)           │
│  ├─ commands/yaml.rs    (CLI: emterm yaml)           │
│  └─ encoding/osc.rs     (generate_json_osc/yaml_osc)│
├─────────────────────────────────────────────────────┤
│ WASM Parser                                         │
│  └─ osc_handler.rs      (route json/yaml commands)  │
├─────────────────────────────────────────────────────┤
│ TypeScript Frontend                                 │
│  ├─ data-viewer/session.ts    (session manager)     │
│  ├─ data-viewer/parser.ts     (JSON/YAML parsing)   │
│  ├─ data-viewer/outline.ts    (tree + detail pane)  │
│  ├─ data-viewer/raw-view.ts   (RAW display)         │
│  ├─ data-viewer/highlighter.ts(syntax highlighting) │
│  └─ data-viewer/fullscreen.ts (overlay controller)  │
└─────────────────────────────────────────────────────┘
```

### OSC Sequence Format

**JSON:**
```
ESC ] 777 ; emterm ; json ; begin ; id={uuid} ; version=1.0 ESC \
ESC ] 777 ; emterm ; json ; chunk ; id={uuid} ; seq=N ; data={base64} ESC \
ESC ] 777 ; emterm ; json ; end ; id={uuid} ESC \
```

**YAML:**
```
ESC ] 777 ; emterm ; yaml ; begin ; id={uuid} ; version=1.0 ESC \
ESC ] 777 ; emterm ; yaml ; chunk ; id={uuid} ; seq=N ; data={base64} ESC \
ESC ] 777 ; emterm ; yaml ; end ; id={uuid} ESC \
```

### Keyboard Shortcuts

| Key | Outline View | RAW View |
|-----|-------------|----------|
| Escape | Close viewer | Close viewer |
| r | Switch to RAW | Switch to Outline |
| p | (no effect) | JSON: toggle pretty-print |
| Up/Down | Navigate tree items | Scroll |
| Page Up/Down | Page scroll tree | Page scroll |
| Home/End | First/last tree item | Top/bottom |
| Space | (no effect) | Page down |
| Shift+Space | (no effect) | Page up |

### Outline View Layout

```
┌─────────────────────────────────────────┐
│ [Outline]                               │
├──────────────┬──────────────────────────┤
│ Tree (left)  │ Detail (right)           │
│ ├─ key1      │ {                        │
│ ├─ key2 ◀   │   "sub1": "value",      │
│ │  ├─ sub1   │   "sub2": 42            │
│ │  └─ sub2   │ }                        │
│ └─ key3      │                          │
├──────────────┴──────────────────────────┤
│ [r] Toggle  [Esc] Close                 │
└─────────────────────────────────────────┘
```

- Tree shows all keys at all nesting levels, always fully expanded
- No folding/collapsing of tree nodes
- Selecting a key displays its value in the right pane as JSON or YAML
- Initial selection: root level (entire document content in right pane)

### RAW View Layout

```
┌─────────────────────────────────────────┐
│ [RAW]                                   │
├─────────────────────────────────────────┤
│ {                                       │
│   "key1": "value1",                     │
│   "key2": { ... },                      │
│   "key3": [1, 2, 3]                     │
│ }                                [Copy] │
├─────────────────────────────────────────┤
│ [r] Toggle  [p] Pretty  [Esc] Close    │
└─────────────────────────────────────────┘
```

- Displays full file content with syntax highlighting
- Copy button copies content to clipboard
- `p` toggles pretty-print (JSON only)

### Error Display

When JSON/YAML parsing fails:
- Error message banner at the top of the viewer
- File content displayed as plain text (no syntax highlighting)
- Outline view is disabled; only RAW view is available
- `r` key has no effect (stays in RAW mode)

### Dependencies

**Internal Dependencies:**
- Markdown viewer architecture (session management, fullscreen overlay, OSC dispatch)
- Tauri clipboard plugin (copy functionality)
- WASM OSC parser (OSC 777 routing)

**External Dependencies (Frontend):**
- JSON parsing: built-in `JSON.parse()`
- YAML parsing: `yaml` npm package (or similar)
- Syntax highlighting: custom lightweight highlighter (token-based coloring for JSON/YAML)

### File Structure

```
src-tauri/src/
├── commands/
│   ├── json.rs              # CLI: emterm json
│   └── yaml.rs              # CLI: emterm yaml
├── encoding/
│   └── osc.rs               # + generate_json_osc, generate_yaml_osc

wasm/src/
└── osc_handler.rs           # + route json/yaml OSC commands

src/
├── data-viewer/
│   ├── types.ts             # DataFormat, DataViewerBlock, etc.
│   ├── session.ts           # DataViewerSessionManager
│   ├── parser.ts            # JSON/YAML parse + error handling
│   ├── outline.ts           # Tree panel + detail panel
│   ├── raw-view.ts          # RAW display with highlighting
│   ├── highlighter.ts       # Syntax highlighting for JSON/YAML
│   └── fullscreen.ts        # Fullscreen overlay controller
├── terminal/handlers/
│   └── osc_handlers.ts      # + dispatch json/yaml to DataViewerSessionManager
└── terminal/
    └── state.ts             # + DataViewerSessionManager instance
```

## Test Scenarios

### Unit Tests
- [ ] JSON parser: valid JSON → parsed object
- [ ] JSON parser: invalid JSON → error with message
- [ ] YAML parser: valid YAML → parsed object
- [ ] YAML parser: invalid YAML → error with message
- [ ] Tree builder: nested object → correct tree structure
- [ ] Tree builder: arrays → correct tree structure
- [ ] Pretty-print: minified JSON → formatted output
- [ ] Highlighter: JSON tokens → correct color classes

### Integration Tests
- [ ] CLI: `emterm json <file>` produces correct OSC sequences
- [ ] CLI: `emterm yaml <file>` produces correct OSC sequences
- [ ] Session manager: begin/chunk/end → assembled data

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/` (31 spec files)
**Run command**: `./scripts/run-e2e-docker.sh`
- [ ] Existing E2E tests pass without regression
- [ ] JSON viewer opens and displays outline
- [ ] YAML viewer opens and displays outline
- [ ] `r` key toggles between outline and RAW
- [ ] `p` key toggles JSON pretty-print
- [ ] Escape closes viewer

### Edge Cases
- [ ] Empty JSON object `{}`
- [ ] Empty JSON array `[]`
- [ ] Empty YAML document
- [ ] Deeply nested structure (100+ levels)
- [ ] Large file (10MB+)
- [ ] JSON with Unicode characters
- [ ] YAML with anchors and aliases
- [ ] YAML with multi-line strings
- [ ] Binary file (should show error + raw)

## Security Considerations

- **Input Validation:** File existence and readability checked in Rust backend
- **XSS Prevention:** DOMPurify sanitization for all rendered HTML content
- **Data Protection:** No external network requests; all processing is local

## Error Handling

### Error Cases

| Scenario | Behavior |
|----------|----------|
| File not found | CLI error message, viewer not opened |
| File is directory | CLI error message, viewer not opened |
| File read error | CLI error message, viewer not opened |
| JSON parse error | Viewer opens with error banner + RAW text |
| YAML parse error | Viewer opens with error banner + RAW text |

## Success Criteria

- [ ] All functional requirements (FR1-FR10) are implemented and tested
- [ ] All test scenarios pass
- [ ] Keyboard shortcuts work as specified
- [ ] Syntax highlighting is visually clear and correct
- [ ] Error handling works for invalid files
- [ ] tmux passthrough works correctly
- [ ] No regression in existing Markdown viewer functionality

## References

- Markdown viewer implementation: `src/markdown/`
- OSC encoding: `src-tauri/src/encoding/osc.rs`
- Existing CLI commands: `src-tauri/src/main.rs`
