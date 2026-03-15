# Implementation Plan: JSON/YAML Viewer

## Overview

Add JSON and YAML viewer functionality to eMterm, replicating the Markdown viewer's architecture (OSC 777 + session management + fullscreen overlay) with a two-pane outline view and RAW display mode.

## Objectives

- Implement `emterm json` and `emterm yaml` CLI commands
- Build a fullscreen data viewer with outline (tree + detail) and RAW display modes
- Support keyboard-driven navigation, view toggling, and JSON pretty-print

## Prerequisites

### Development Environment
- Rust toolchain (for Tauri backend and WASM)
- Bun (package manager and bundler)
- wasm-pack (for WASM module)

### Dependencies
- Existing Markdown viewer architecture (session pattern, fullscreen overlay, OSC dispatch)
- Tauri clipboard plugin (copy functionality)
- YAML parsing library for frontend (npm package)

## Architecture Overview

### Technology Stack
- **Backend (Rust)**: CLI commands, file I/O, OSC sequence generation
- **WASM (Rust)**: OSC 777 routing (existing, minor modification)
- **Frontend (TypeScript)**: Session management, JSON/YAML parsing, viewer UI, syntax highlighting

### Design Approach

Follow the established Markdown viewer pattern exactly:
1. CLI reads file → base64 encodes → chunks → OSC 777 sequences to stdout
2. WASM parser routes OSC 777 to TypeScript callback
3. TypeScript session manager assembles chunks, parses data, displays viewer
4. Fullscreen overlay with keyboard navigation

The viewer introduces a new concept not present in the Markdown viewer: a two-pane outline mode where the left pane shows a navigable key tree and the right pane shows the selected key's value. This is the default view, with RAW mode as a toggle alternative.

### Component Interaction

```
CLI (json/yaml) → OSC 777 → WASM → TypeScript OSC Handler
                                          ↓
                                DataViewerSessionManager
                                          ↓
                              ┌─── Parse JSON/YAML ───┐
                              ↓                        ↓
                         Success                    Failure
                              ↓                        ↓
                    DataViewerFullscreen      Error banner + RAW text
                     ┌────┴────┐
                  Outline    RAW
                  (default)  (toggle r)
```

## Implementation Phases

### Phase 1: Backend CLI & OSC Generation

**Goal**: `emterm json <file>` and `emterm yaml <file>` produce correct OSC 777 sequences on stdout.

**Files to Create**:
- `src-tauri/src/commands/json.rs` - JSON CLI command executor
- `src-tauri/src/commands/yaml.rs` - YAML CLI command executor

**Files to Modify**:
- `src-tauri/src/main.rs` - Register json/yaml subcommands
- `src-tauri/src/commands/mod.rs` - Export new modules
- `src-tauri/src/encoding/osc.rs` - Add OSC generators for json/yaml
- `src-tauri/locales/en.json` - CLI help text (English)
- `src-tauri/locales/ja.json` - CLI help text (Japanese)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| execute_json_command | Read file, encode, generate OSC, output to stdout | Valid file path | OSC 777 json sequences written to stdout |
| execute_yaml_command | Read file, encode, generate OSC, output to stdout | Valid file path | OSC 777 yaml sequences written to stdout |
| generate_json_osc | Build OSC 777 begin/chunk/end sequence for JSON | Session ID + chunks | Complete OSC sequence string |
| generate_yaml_osc | Build OSC 777 begin/chunk/end sequence for YAML | Session ID + chunks | Complete OSC sequence string |

**Processing Flow**:
1. Validate file exists and is a regular file
   - Not found → FileNotFound error
   - Is directory → NotAFile error
2. Read file content into memory
   - Read error → FileReadError
3. Base64 encode → chunk into 128KB segments
4. Generate OSC 777 sequence with format-specific command name
5. Wrap in tmux DCS passthrough if inside tmux
6. Write to stdout, flush

**Implementation Steps**:
1. **Add CLI subcommands** - Register "json" and "yaml" in clap command builder with i18n help text
2. **Create command executors** - Follow markdown command pattern: validate file, read, encode, chunk, generate OSC, output
3. **Add OSC generators** - Create generate_json_osc and generate_yaml_osc following generate_markdown_osc pattern
4. **Wire command routing** - Add match arms in main for json/yaml subcommands
5. **Add i18n strings** - CLI help text for both languages

**Dependencies**: None (first phase)

**Testing Approach**:
- Unit: OSC generator produces correct sequence format for json/yaml
- Unit: Command executor validates file correctly (not found, not a file, read error)
- Integration: CLI produces expected OSC output for sample files

**Acceptance Criteria**:
- [ ] `emterm json test.json` outputs valid OSC 777 json sequences
- [ ] `emterm yaml test.yaml` outputs valid OSC 777 yaml sequences
- [ ] tmux passthrough wrapping works
- [ ] Error messages display correctly for invalid paths

**Estimated Effort**: small

---

### Phase 2: Frontend Session Manager & Data Parsing

**Goal**: TypeScript receives OSC sequences, assembles data, and parses JSON/YAML with error handling.

**Files to Create**:
- `src/data-viewer/types.ts` - Type definitions (DataFormat, DataViewerSession, ParseResult, etc.)
- `src/data-viewer/session.ts` - DataViewerSessionManager
- `src/data-viewer/parser.ts` - JSON/YAML parsing with error reporting

**Files to Modify**:
- `src/terminal/state.ts` - Add DataViewerSessionManager instance and getter
- `src/terminal/handlers/osc_handlers.ts` - Route json/yaml commands to DataViewerSessionManager
- `src/terminal/handlers/types.ts` - Add getDataViewerManager to TerminalStateAccessor interface
- `package.json` - Add YAML parsing dependency

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| DataViewerSessionManager | Manage OSC sessions for json/yaml data | Container set | Sessions tracked, data assembled and parsed |
| DataParser | Parse JSON/YAML strings, return structured result or error | Raw string + format | ParseResult with parsed data or error message |

**Processing Flow**:
1. OSC handler receives emterm extension with params[0] = "json" or "yaml"
2. Route to DataViewerSessionManager.handleCommand
3. Begin: create session with format (json/yaml)
4. Chunk: accumulate base64-decoded chunks
5. End: assemble chunks → parse with DataParser → display or show error
   - Parse success → build tree structure, show fullscreen viewer in outline mode
   - Parse failure → show fullscreen viewer in error+RAW mode

**Implementation Steps**:
1. **Define types** - DataFormat enum, session interface, parse result, tree node structure
2. **Create session manager** - Follow MarkdownSessionManager pattern with begin/chunk/end handling
3. **Create data parser** - JSON via built-in parser, YAML via library; return structured result with tree
4. **Wire OSC routing** - Add json/yaml dispatch in handleEmtermExtension
5. **Register in TerminalState** - Instantiate manager, add getter, update accessor interface

**Dependencies**: Requires Phase 1 (backend generates the sequences)

**Testing Approach**:
- Unit: Session manager handles begin/chunk/end lifecycle correctly
- Unit: Parser handles valid/invalid JSON and YAML
- Unit: Tree builder creates correct hierarchy from parsed data
- Integration: Full OSC sequence → parsed data flow

**Acceptance Criteria**:
- [ ] Session manager assembles chunked data correctly
- [ ] JSON parsing succeeds for valid JSON, returns error for invalid
- [ ] YAML parsing succeeds for valid YAML, returns error for invalid
- [ ] Tree structure correctly represents nested objects/arrays
- [ ] OSC routing dispatches json/yaml to the correct manager

**Estimated Effort**: medium

---

### Phase 3: Syntax Highlighter & RAW View

**Goal**: Display file content as syntax-highlighted text with copy button.

**Files to Create**:
- `src/data-viewer/highlighter.ts` - Token-based syntax highlighter for JSON/YAML
- `src/data-viewer/raw-view.ts` - RAW display component with copy button
- `src/styles/data-viewer.css` - Viewer styles (layout, highlighting colors, etc.)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| DataHighlighter | Tokenize JSON/YAML and produce highlighted HTML | Raw text + format | HTML string with color-coded spans |
| RawView | Render highlighted content in scrollable container with copy button | Highlighted HTML | DOM element ready for display |

**Processing Flow**:
1. Receive raw text and format
2. Tokenize into semantic categories: key, string, number, boolean, null, punctuation
3. Wrap each token in span with appropriate CSS class
4. Sanitize output via DOMPurify
5. Render in scrollable container with copy button

**Implementation Steps**:
1. **Build tokenizer** - Lightweight lexer that classifies JSON/YAML tokens by type
2. **Create highlight renderer** - Convert tokens to HTML spans with CSS classes
3. **Create RAW view component** - Scrollable container, copy button, keyboard scroll support
4. **Add CSS styles** - Token colors, layout, copy button positioning
5. **JSON pretty-print** - Re-serialize parsed JSON with indentation for `p` key toggle

**Dependencies**: Requires Phase 2 (parsed data and raw text available)

**Testing Approach**:
- Unit: Highlighter produces correct tokens for JSON samples
- Unit: Highlighter produces correct tokens for YAML samples
- Unit: Pretty-print produces correctly formatted JSON
- Unit: Copy button extracts correct text content

**Acceptance Criteria**:
- [ ] JSON tokens are correctly color-coded (keys, strings, numbers, booleans, null)
- [ ] YAML tokens are correctly color-coded
- [ ] Copy button copies raw content to clipboard
- [ ] Pretty-print toggle reformats JSON correctly
- [ ] HTML output is sanitized against XSS

**Estimated Effort**: medium

---

### Phase 4: Outline View (Tree + Detail Pane)

**Goal**: Two-pane layout with navigable key tree on the left and selected value display on the right.

**Files to Create**:
- `src/data-viewer/outline.ts` - Outline component (tree panel + detail panel)
- `src/data-viewer/tree-builder.ts` - Build tree node structure from parsed data

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OutlineView | Render two-pane layout with tree and detail | Parsed tree structure | Interactive outline with selection |
| TreeBuilder | Convert parsed JSON/YAML to flat tree node list | Parsed data object | Array of TreeNode with depth, key, value, path |

**Processing Flow**:
1. Build tree nodes from parsed data (recursive traversal)
   - Each node: key name, depth level, value reference, path from root
   - Array elements: index as key (e.g., "[0]", "[1]")
   - Leaf nodes: store primitive value
   - Branch nodes: store sub-object/array reference
2. Render left pane: flat list of nodes with indentation based on depth
   - All nodes visible (no folding)
   - Selected node highlighted
3. Render right pane: selected node's value formatted as JSON/YAML with highlighting
4. Keyboard navigation: Up/Down moves selection, updates right pane

**Implementation Steps**:
1. **Build tree structure** - Recursive traversal producing flat node list with depth/path metadata
2. **Create tree panel** - Render nodes as scrollable list with visual indentation and selection state
3. **Create detail panel** - Display selected node's value with syntax highlighting
4. **Add keyboard navigation** - Arrow keys for selection, Home/End for first/last
5. **Wire initial state** - Root selected by default showing entire document

**Dependencies**: Requires Phase 2 (tree structure), Phase 3 (highlighter for detail pane)

**Testing Approach**:
- Unit: Tree builder produces correct nodes for nested objects
- Unit: Tree builder handles arrays, mixed types, empty containers
- Unit: Selection state updates correctly on navigation
- E2E (Docker): Outline displays and keyboard navigation works

**Acceptance Criteria**:
- [ ] Tree displays all keys at all nesting levels with proper indentation
- [ ] Selecting a key shows its value in the right pane
- [ ] Root is selected by default, showing entire document
- [ ] Arrow keys navigate tree items
- [ ] Detail pane content updates on selection change
- [ ] Array elements shown with index keys

**Estimated Effort**: medium

---

### Phase 5: Fullscreen Overlay & Integration

**Goal**: Complete fullscreen viewer with mode toggling, all keyboard shortcuts, and app integration.

**Files to Create**:
- `src/data-viewer/fullscreen.ts` - Fullscreen overlay controller (combines outline and RAW views)

**Files to Modify**:
- `src/terminal-app/index.ts` - Wire DataViewerSessionManager container and IME callbacks

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| DataViewerFullscreen | Manage fullscreen overlay lifecycle, view mode toggling, keyboard dispatch | Parsed data + raw text | Interactive fullscreen viewer |

**Processing Flow**:
1. Show: create overlay in overlay-root container
   - Default to outline mode (or RAW-only if parse error)
   - Set up keyboard listener on document (capture phase)
   - Save and manage focus
   - Notify IME handler to blur
2. Keyboard dispatch:
   - Escape → close viewer
   - `r` → toggle outline/RAW mode (disabled on parse error)
   - `p` → toggle JSON pretty-print (RAW mode + JSON format only)
   - Navigation keys → delegate to active view (outline or RAW)
3. Close: remove overlay, restore focus, notify IME handler to focus

**Implementation Steps**:
1. **Create fullscreen controller** - Overlay creation, lifecycle management, following Markdown fullscreen pattern
2. **Implement mode toggling** - Switch between outline and RAW DOM subtrees on `r` key
3. **Implement pretty-print toggle** - Re-render RAW view with formatted/original JSON on `p` key
4. **Wire error mode** - Parse error forces RAW-only with error banner, disables `r` key
5. **Integrate with terminal-app** - Set container, wire IME blur/focus callbacks
6. **Add status bar** - Show current mode indicator and available shortcuts at bottom

**Dependencies**: Requires Phase 3 (RAW view), Phase 4 (outline view)

**Testing Approach**:
- Unit: Mode toggle switches DOM correctly
- Unit: Pretty-print toggle re-renders content
- Unit: Error mode disables outline toggle
- E2E (Docker): Full viewer lifecycle with keyboard interactions
- Manual: Visual appearance, layout proportions

**Acceptance Criteria**:
- [ ] Viewer opens in fullscreen overlay within overlay-root
- [ ] `r` toggles between outline and RAW mode
- [ ] `p` toggles pretty-print in JSON RAW mode only
- [ ] Escape closes viewer and restores terminal focus
- [ ] IME blur/focus coordinated on show/hide
- [ ] Error mode shows banner and RAW text, disables outline toggle
- [ ] Status bar shows current mode and available shortcuts

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/
├── main.rs                          # + json/yaml subcommands
├── commands/
│   ├── mod.rs                       # + json, yaml module exports
│   ├── json.rs                      # NEW: JSON CLI command
│   └── yaml.rs                      # NEW: YAML CLI command
├── encoding/
│   └── osc.rs                       # + generate_json_osc, generate_yaml_osc
└── locales/
    ├── en.json                      # + CLI help text
    └── ja.json                      # + CLI help text

src/
├── data-viewer/
│   ├── types.ts                     # NEW: Type definitions
│   ├── session.ts                   # NEW: DataViewerSessionManager
│   ├── parser.ts                    # NEW: JSON/YAML parser
│   ├── highlighter.ts               # NEW: Syntax highlighter
│   ├── raw-view.ts                  # NEW: RAW display component
│   ├── outline.ts                   # NEW: Outline (tree + detail)
│   ├── tree-builder.ts              # NEW: Tree node builder
│   └── fullscreen.ts               # NEW: Fullscreen overlay controller
├── styles/
│   └── data-viewer.css              # NEW: Viewer styles
├── terminal/
│   ├── state.ts                     # + DataViewerSessionManager
│   └── handlers/
│       ├── osc_handlers.ts          # + json/yaml routing
│       └── types.ts                 # + getDataViewerManager
└── terminal-app/
    └── index.ts                     # + container/IME wiring
```

## Testing Strategy

- **Unit tests**: Parser, highlighter, tree builder, session manager lifecycle — target 80%+ coverage for core logic
- **Integration tests**: CLI → OSC output, OSC → session → parsed data
- **E2E (Docker)**: Viewer opens, keyboard navigation, mode toggling, close
- **Manual**: Visual appearance, syntax highlighting colors, layout proportions

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| yaml (npm) | latest | YAML parsing in frontend |
| dompurify | existing | HTML sanitization |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Large file performance in outline tree | Medium | Medium | Virtual scrolling for tree panel if needed |
| YAML anchor/alias complexity | Low | Low | Use established YAML library that resolves references |
| CSS layout conflicts with existing styles | Low | Medium | Namespace all data-viewer styles |

## Open Questions

None — all requirements resolved during specification.

## Success Metrics

- [ ] All 10 functional requirements (FR1-FR10) implemented
- [ ] All 4 non-functional requirements (NFR1-NFR4) met
- [ ] Unit test coverage ≥ 80% for core logic
- [ ] No regression in existing Markdown viewer
- [ ] Keyboard shortcuts work as specified
