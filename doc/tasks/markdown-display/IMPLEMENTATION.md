# Implementation Plan: Markdown Display Feature

## Overview
- **Specification**: [SPEC.md](./SPEC.md)
- **Requirements**: [要件定義書.md](./要件定義書.md)
- **Status**: Draft
- **Last Updated**: 2026-01-03

## Current State Analysis

### Existing Implementation
1. **Rust OSC 777 Parser** (`src-tauri/src/ansi/parser.rs` L645-655)
   - Already parses OSC 777 sequences
   - Emits `OscAction::EmtermExtension { verb, params }`
   - Current format: `verb;param1;param2;...`
   - **Required change**: Update to handle `emterm;markdown;verb;params...` structure

2. **TypeScript Types** (`src/types/terminal.ts` L80)
   - `OscAction` already has `EmtermExtension` variant: `{ action: "EmtermExtension"; verb: string; params: string[] }`

3. **TerminalState** (`src/terminal/state.ts` L910-913)
   - Has placeholder for `EmtermExtension` handling (currently no-op)

4. **Dependencies** (`package.json`)
   - `marked` and `dompurify` need to be added

## Implementation Phases

### Phase 1: Backend Protocol Enhancement
**Goal**: Update Rust parser to properly structure OSC 777 emterm;markdown sequences
**Estimated Effort**: Small

#### Components
1. **Enhanced OSC 777 Parser**
   - Location: `src-tauri/src/ansi/parser.rs`
   - Responsibility: Parse `emterm;markdown;verb;params` structure and emit structured action
   - Interface:
     ```rust
     // Current: OscAction::EmtermExtension { verb: String, params: Vec<String> }
     // After parsing "emterm;markdown;begin;id=xxx;format=gfm":
     //   verb = "emterm"
     //   params = ["markdown", "begin", "id=xxx", "format=gfm"]
     // Frontend will interpret params[0] as command, params[1] as verb
     ```
   - Dependencies: None (existing parser infrastructure)

#### Tasks
- [ ] Update OSC 777 parsing to validate `emterm` namespace prefix
- [ ] Ensure params are passed through correctly for frontend processing
- [ ] Add unit tests for markdown sequence parsing

#### Acceptance Criteria
- OSC 777 sequences with `emterm;markdown` prefix are parsed correctly
- Invalid sequences (missing namespace) emit `OscAction::Unknown`
- All existing OSC 777 tests pass
- New tests cover begin/chunk/end verb parsing

---

### Phase 2: TypeScript Type Definitions
**Goal**: Define type contracts for Markdown session management
**Estimated Effort**: Small

#### Components
1. **Markdown Types Module**
   - Location: `src/markdown/types.ts`
   - Responsibility: Define interfaces for session, command, and block data
   - Interface:
     ```typescript
     interface MarkdownSession {
       id: string;
       format: "commonmark" | "gfm";
       version: number;
       render: "inline" | "block";
       chunks: Map<number, string>;
       nextSeq: number;
       createdAt: number;
       dataSize: number;
     }

     interface MarkdownCommand {
       verb: "begin" | "chunk" | "end";
       params: Record<string, string>;
     }

     interface MarkdownBlock {
       id: string;
       html: string;
       startRow: number;
       rowCount: number;
       visible: boolean;
     }
     ```
   - Dependencies: None

#### Tasks
- [ ] Create `src/markdown/types.ts` with session, command, and block interfaces
- [ ] Export types from `src/markdown/index.ts`

#### Acceptance Criteria
- All types from SPEC.md section 3.2 are defined
- Types are exported and importable from `src/markdown`

---

### Phase 3: Session Manager Implementation
**Goal**: Implement session lifecycle management (begin/chunk/end)
**Estimated Effort**: Medium

#### Components
1. **MarkdownSessionManager**
   - Location: `src/markdown/session.ts`
   - Responsibility: Manage session lifecycle, chunk accumulation, and timeout
   - Interface:
     ```typescript
     class MarkdownSessionManager {
       static readonly MAX_SESSION_SIZE = 2 * 1024 * 1024; // 2MB
       static readonly SESSION_TIMEOUT = 30 * 1000; // 30s
       static readonly MAX_SESSIONS = 10;

       handleCommand(verb: string, params: string[]): MarkdownBlock | null;
       getSession(id: string): MarkdownSession | undefined;
       cleanupExpiredSessions(): void;
       get sessionCount(): number;
       dispose(): void;
     }
     ```
   - Dependencies: `MarkdownRenderer` (Phase 4)

2. **Parameter Parser Utility**
   - Location: `src/markdown/session.ts` (internal)
   - Responsibility: Parse `key=value` parameter strings
   - Interface:
     ```typescript
     function parseParams(params: string[]): Record<string, string>;
     ```
   - Dependencies: None

#### Tasks
- [ ] Implement `MarkdownSessionManager` class
- [ ] Implement `handleBegin` - create session with validation
- [ ] Implement `handleChunk` - Base64 decode and accumulate
- [ ] Implement `handleEnd` - assemble chunks and trigger render
- [ ] Implement timeout cleanup with `setInterval`
- [ ] Implement size limit enforcement
- [ ] Add comprehensive unit tests

#### Acceptance Criteria
- Sessions are created with valid UUID and optional parameters
- Invalid sessions (no ID, max sessions reached) are rejected with warning
- Chunks are decoded and accumulated by sequence number
- Size limit (2MB) is enforced per session
- Sessions timeout after 30 seconds
- Maximum 10 concurrent sessions enforced

---

### Phase 4: Markdown Renderer Implementation
**Goal**: Render Markdown to sanitized HTML
**Estimated Effort**: Medium

#### Components
1. **MarkdownRenderer**
   - Location: `src/markdown/renderer.ts`
   - Responsibility: Parse Markdown, sanitize HTML, manage DOM blocks
   - Interface:
     ```typescript
     class MarkdownRenderer {
       render(markdown: string, format: "commonmark" | "gfm"): string;
       insertBlock(block: MarkdownBlock, container: HTMLElement): HTMLElement;
       removeBlock(id: string): void;
       getBlock(id: string): HTMLElement | undefined;
       dispose(): void;
     }
     ```
   - Dependencies: `marked`, `dompurify`

2. **DOMPurify Configuration**
   - Location: `src/markdown/renderer.ts` (internal)
   - Responsibility: Configure allowed tags/attributes for XSS protection
   - Interface:
     ```typescript
     const PURIFY_CONFIG: DOMPurify.Config = {
       ALLOWED_TAGS: [...],
       ALLOWED_ATTR: [...],
       FORBID_TAGS: ["script", "style", "iframe", ...],
       FORBID_ATTR: ["onerror", "onclick", ...],
     };
     ```
   - Dependencies: `dompurify`

#### Tasks
- [ ] Add `marked`, `dompurify`, `highlight.js`, `mermaid` dependencies to package.json
- [ ] Create TypeScript type declarations for dependencies (if needed)
- [ ] Implement `MarkdownRenderer` class
- [ ] Configure `marked` for CommonMark and GFM modes
- [ ] Integrate `highlight.js` for syntax highlighting in code blocks
- [ ] Integrate `mermaid` for diagram rendering (flowcharts, sequence diagrams, etc.)
- [ ] Configure DOMPurify with strict whitelist
- [ ] Implement `insertBlock` with link handling (`target="_blank"`, `rel="noopener"`)
- [ ] Add unit tests for rendering
- [ ] Add security tests for XSS prevention

#### Acceptance Criteria
- CommonMark and GFM Markdown renders correctly to HTML
- All dangerous HTML is sanitized (script, event handlers, javascript: URLs)
- Code blocks have syntax highlighting via highlight.js
- Mermaid diagrams are rendered correctly
- Links open in new tab with `rel="noopener noreferrer"`
- Blocks can be inserted into and removed from DOM

---

### Phase 5: Terminal State Integration
**Goal**: Connect Markdown system to terminal action processing
**Estimated Effort**: Small

#### Components
1. **TerminalState Enhancement**
   - Location: `src/terminal/state.ts`
   - Responsibility: Delegate EmtermExtension actions to MarkdownSessionManager
   - Interface:
     ```typescript
     // Add to TerminalState class
     private markdownManager: MarkdownSessionManager;
     private _markdownBlocks: MarkdownBlock[];

     private handleEmtermExtension(verb: string, params: string[]): void;
     takePendingMarkdownBlocks(): MarkdownBlock[];
     ```
   - Dependencies: `MarkdownSessionManager`

#### Tasks
- [ ] Add `MarkdownSessionManager` instance to `TerminalState`
- [ ] Update `handleOsc` to delegate `EmtermExtension` to new handler
- [ ] Implement `handleEmtermExtension` to route markdown commands
- [ ] Implement `takePendingMarkdownBlocks` for renderer retrieval
- [ ] Add cleanup in `reset()` method
- [ ] Add integration tests

#### Acceptance Criteria
- `EmtermExtension` actions with `markdown` command are processed
- Completed blocks are queued for rendering
- Blocks include cursor position information
- State is properly cleaned up on terminal reset

---

### Phase 6: DOM Rendering Integration
**Goal**: Display rendered Markdown in terminal WebView
**Estimated Effort**: Medium

#### Components
1. **Markdown Container**
   - Location: `src/main.ts` or new `src/markdown/container.ts`
   - Responsibility: Create and manage DOM container for Markdown blocks
   - Interface:
     ```typescript
     function createMarkdownContainer(): HTMLElement;
     function insertMarkdownBlock(block: MarkdownBlock, container: HTMLElement): void;
     ```
   - Dependencies: DOM

2. **Render Loop Integration**
   - Location: `src/terminal/renderer.ts` (or rendering entry point)
   - Responsibility: Check for pending blocks and render them
   - Interface:
     ```typescript
     function renderPendingMarkdownBlocks(
       state: TerminalState,
       container: HTMLElement,
       renderer: MarkdownRenderer
     ): void;
     ```
   - Dependencies: `TerminalState`, `MarkdownRenderer`

#### Tasks
- [ ] Create Markdown container element in DOM structure
- [ ] Add CSS styles for Markdown blocks (`.markdown-block`)
- [ ] Integrate block rendering into main render loop
- [ ] Position blocks relative to terminal content
- [ ] Implement virtual scrolling (`updateVisibility`) for off-screen blocks
- [ ] Verify Markdown blocks cannot access Tauri privileged APIs (permission isolation)
- [ ] Manual testing for visual verification (no automated E2E tests)

#### Acceptance Criteria
- Markdown blocks appear in terminal at correct position
- Blocks are styled consistently with terminal theme
- Multiple blocks can be displayed simultaneously
- Blocks scroll with terminal content
- Off-screen blocks are detached from DOM (virtual scrolling)
- Markdown rendering code has no access to Tauri APIs

---

### Phase 7: Theme Integration
**Goal**: Synchronize Markdown styles with terminal theme
**Estimated Effort**: Small

#### Components
1. **Theme Generator**
   - Location: `src/markdown/theme.ts`
   - Responsibility: Generate Markdown CSS variables from terminal colors
   - Interface:
     ```typescript
     interface MarkdownTheme {
       "--md-bg": string;
       "--md-fg": string;
       "--md-link": string;
       "--md-code-bg": string;
       "--md-code-fg": string;
       "--md-border": string;
       "--md-heading": string;
       "--md-blockquote": string;
     }

     function generateMarkdownTheme(
       terminalBg: string,
       terminalFg: string,
       palette: string[]
     ): MarkdownTheme;

     function applyMarkdownTheme(
       container: HTMLElement,
       theme: MarkdownTheme
     ): void;
     ```
   - Dependencies: Terminal color palette

2. **Markdown CSS**
   - Location: `src/styles.css` (or new `src/markdown/styles.css`)
   - Responsibility: Style Markdown elements using CSS variables
   - Dependencies: None

#### Tasks
- [ ] Implement theme generation from terminal colors
- [ ] Create CSS styles using CSS custom properties
- [ ] Apply theme on terminal theme change
- [ ] Add visual tests for theme consistency

#### Acceptance Criteria
- Markdown colors match terminal theme
- Theme updates when terminal theme changes
- Code blocks use terminal-appropriate colors
- Links are visually distinct but theme-consistent

---

### Phase 8: Polish and Error Handling
**Goal**: Refine error handling, logging, and edge cases
**Estimated Effort**: Small

#### Components
1. **Error Handling Enhancement**
   - Location: All markdown modules
   - Responsibility: Consistent error logging and recovery
   - Interface: N/A (internal behavior)
   - Dependencies: None

2. **Performance Monitoring**
   - Location: `src/markdown/session.ts`, `src/markdown/renderer.ts`
   - Responsibility: Log timing for large documents
   - Dependencies: `console.time/timeEnd` or performance API

#### Tasks
- [ ] Add consistent warning logging for all error cases
- [ ] Implement graceful degradation for parse errors
- [ ] Add performance timing for render operations
- [ ] Document error handling behavior
- [ ] Manual testing with edge cases

#### Acceptance Criteria
- All errors from SPEC section 5.1 are handled appropriately
- Parse errors fall back to raw text display
- Performance meets requirements (<100ms for 1KB)
- No unhandled exceptions from malformed input

## Component Contracts

### Rust Parser -> TypeScript TerminalState
- **Communication**: Tauri IPC event (`terminal_actions`)
- **Data Format**: JSON-serialized `OscAction::EmtermExtension`
  ```json
  {
    "action": "EmtermExtension",
    "verb": "emterm",
    "params": ["markdown", "begin", "id=uuid", "format=gfm"]
  }
  ```
- **Error Handling**: Invalid sequences emit `OscAction::Unknown`, frontend ignores

### TerminalState -> MarkdownSessionManager
- **Communication**: Direct method call
- **Data Format**: `handleCommand(verb: string, params: string[])`
  - verb: `"emterm"` (from Rust parser)
  - params: `["markdown", "begin", "id=...", ...]` or `["markdown", "chunk", ...]` or `["markdown", "end", ...]`
- **Error Handling**: Manager logs warnings, returns `null` for invalid commands

### MarkdownSessionManager -> MarkdownRenderer
- **Communication**: Direct method call
- **Data Format**: `render(markdown: string, format: string)` returns sanitized HTML
- **Error Handling**: Parse errors return escaped raw text

### MarkdownRenderer -> DOM
- **Communication**: DOM manipulation
- **Data Format**: HTMLElement with `data-markdown-id` attribute
- **Error Handling**: Invalid HTML is sanitized, empty blocks are not inserted

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| XSS vulnerability in rendered HTML | High | Strict DOMPurify whitelist, security tests |
| Memory leak from undisposed sessions | Medium | Timeout cleanup, dispose methods, tests |
| Performance degradation with large docs | Medium | Size limits, async rendering if needed |
| Base64 decoding errors | Low | Try-catch with session cleanup |
| Conflicting OSC 777 usage | Low | Require `emterm` namespace prefix |

## Testing Strategy by Phase

| Phase | Test Type | Coverage Target |
|-------|-----------|-----------------|
| 1 | Unit | OSC 777 parsing with various inputs |
| 2 | Unit | Type validation (compile-time) |
| 3 | Unit | Session lifecycle, timeouts, limits |
| 4 | Unit | Markdown rendering, XSS prevention, syntax highlighting, Mermaid |
| 5 | Integration | TerminalState + SessionManager |
| 6 | Manual | Visual rendering verification (human testing) |
| 7 | Visual | Theme consistency |
| 8 | Manual | Edge cases, error recovery |

## Dependencies

### External Libraries (to be added)
- `marked` (^17.0.0): Markdown parsing - CommonMark and GFM support
- `dompurify` (^3.0.0): HTML sanitization for XSS protection
- `@types/dompurify` (^3.0.0): TypeScript definitions
- `highlight.js` (^11.0.0): Syntax highlighting for code blocks
- `mermaid` (^11.0.0): Diagram rendering (flowcharts, sequence diagrams, etc.)

### Internal Dependencies
- `src/terminal/state.ts`: Terminal state management
- `src/terminal/renderer.ts`: Terminal rendering
- `src/types/terminal.ts`: Action type definitions

## File Structure

```
src/
├── markdown/
│   ├── index.ts           # Module exports
│   ├── types.ts           # Type definitions
│   ├── session.ts         # Session management
│   ├── session.test.ts    # Session tests
│   ├── renderer.ts        # Markdown rendering
│   ├── renderer.test.ts   # Renderer tests
│   ├── theme.ts           # Theme integration
│   └── security.test.ts   # XSS security tests
├── terminal/
│   └── state.ts           # Updated with markdown handling
└── styles.css             # Updated with markdown styles

src-tauri/src/ansi/
└── parser.rs              # Updated OSC 777 parsing
```

## Verification Checklist

### SPEC.md Coverage
- [ ] OSC 777 protocol parsing (Section 3.1)
- [ ] Session management (Section 3.2.3)
- [ ] Markdown rendering (Section 4.2.3)
- [ ] Theme integration (Section 4.2.4)
- [ ] Error handling (Section 5)
- [ ] Security requirements (Section 7)
- [ ] Performance requirements (Section 8)

### Implementation Quality
- [ ] Each phase has clear acceptance criteria
- [ ] Component interfaces are well-defined
- [ ] Error handling is specified for all components
- [ ] Security considerations (XSS) are addressed
- [ ] Dependencies are documented

### Testing Coverage
- [ ] Unit tests for all core logic
- [ ] Integration tests for component interaction
- [ ] Security tests for XSS prevention
- [ ] E2E tests for visual verification
