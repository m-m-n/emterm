# Technical Specification: Markdown Display Feature

## 1. Overview

### 1.1 Purpose
Implement a Markdown rendering feature for eMterm terminal emulator using a custom OSC 777 extension. This enables rich document display directly within the terminal through control sequences.

### 1.2 Scope
- OSC 777 protocol parsing and handling for Markdown content
- Session management for chunked data transfer
- Secure Markdown-to-HTML rendering with XSS protection
- Integration with terminal theme system

### 1.3 Design Principles
- **Explicit commands only**: No automatic content detection
- **Stateless CLI design**: Works over SSH connections
- **Security first**: Complete XSS protection and resource isolation

## 2. Architecture

### 2.1 Component Design

```
┌────────────────────────────────────────────────────────────────────┐
│                         eMterm Application                          │
├─────────────────────────────┬──────────────────────────────────────┤
│      Rust Backend           │         TypeScript Frontend           │
├─────────────────────────────┼──────────────────────────────────────┤
│                             │                                       │
│  ┌─────────────────────┐   │   ┌─────────────────────────────┐    │
│  │    ANSI Parser      │   │   │    TerminalState            │    │
│  │  (src-tauri/src/    │   │   │  (src/terminal/state.ts)    │    │
│  │   ansi/parser.rs)   │   │   └──────────────┬──────────────┘    │
│  └──────────┬──────────┘   │                  │                    │
│             │              │                  ▼                    │
│             │ OSC 777      │   ┌─────────────────────────────┐    │
│             │ EmtermExt    │   │   MarkdownSessionManager    │    │
│             ▼              │   │  (src/markdown/session.ts)  │    │
│  ┌─────────────────────┐   │   └──────────────┬──────────────┘    │
│  │  OscAction::        │   │                  │                    │
│  │  EmtermExtension    │───┼─────────────────►│                    │
│  └─────────────────────┘   │   IPC Event      │                    │
│                             │                  ▼                    │
│                             │   ┌─────────────────────────────┐    │
│                             │   │   MarkdownRenderer          │    │
│                             │   │  (src/markdown/renderer.ts) │    │
│                             │   └──────────────┬──────────────┘    │
│                             │                  │                    │
│                             │                  ▼                    │
│                             │   ┌─────────────────────────────┐    │
│                             │   │   WebView (DOM)             │    │
│                             │   │  - DOMPurify sanitization   │    │
│                             │   │  - Theme integration        │    │
│                             │   └─────────────────────────────┘    │
└─────────────────────────────┴──────────────────────────────────────┘
```

### 2.2 Data Flow

```
1. PTY Output
   │
   ▼
2. ANSI Parser (Rust)
   - Parses OSC 777 sequence
   - Extracts verb and parameters
   - Emits OscAction::EmtermExtension
   │
   ▼
3. IPC Event (terminal_actions)
   - Serialized as JSON
   - Sent to frontend via Tauri event
   │
   ▼
4. TerminalState.handleOsc()
   - Delegates to MarkdownSessionManager
   │
   ▼
5. MarkdownSessionManager
   - begin: Creates new session
   - chunk: Appends Base64-decoded data
   - end: Triggers rendering
   │
   ▼
6. MarkdownRenderer
   - Parses Markdown to AST
   - Converts to HTML
   - Sanitizes with DOMPurify
   - Injects into DOM
```

## 3. Interface Design

### 3.1 OSC Protocol Specification

#### 3.1.1 General Format
```
ESC ] 777 ; emterm ; markdown ; <verb> ; <param>=<value> ; ... ST
```

Where:
- `ESC ]` = `\x1b]` (OSC introducer)
- `777` = OSC number (eMterm extension namespace)
- `emterm` = Application identifier (required)
- `markdown` = Command type
- `<verb>` = Action (begin, chunk, end)
- `<param>=<value>` = Key-value parameters separated by `;`
- `ST` = `\x1b\\` (String Terminator)

#### 3.1.2 begin Verb
```
ESC ] 777 ; emterm ; markdown ; begin ; id=<uuid> [; format=<fmt>] [; version=<ver>] [; render=<mode>] ST
```

**Parameters:**

| Parameter | Required | Type | Default | Description |
|-----------|----------|------|---------|-------------|
| id | Yes | UUID v4 | - | Session identifier |
| format | No | string | `commonmark` | Markdown format (`commonmark`, `gfm`) |
| version | No | integer | `1` | Protocol version |
| render | No | string | `block` | Render mode (`inline`, `block`) |

**Example:**
```
\x1b]777;emterm;markdown;begin;id=550e8400-e29b-41d4-a716-446655440000;format=gfm\x1b\\
```

#### 3.1.3 chunk Verb
```
ESC ] 777 ; emterm ; markdown ; chunk ; id=<uuid> ; seq=<n> ; data=<base64> ST
```

**Parameters:**

| Parameter | Required | Type | Description |
|-----------|----------|------|-------------|
| id | Yes | UUID v4 | Session identifier |
| seq | Yes | integer | Sequence number (0-indexed) |
| data | Yes | Base64 | Base64-encoded Markdown content |

**Example:**
```
\x1b]777;emterm;markdown;chunk;id=550e8400-e29b-41d4-a716-446655440000;seq=0;data=IyBIZWxsbw==\x1b\\
```

#### 3.1.4 end Verb
```
ESC ] 777 ; emterm ; markdown ; end ; id=<uuid> ST
```

**Parameters:**

| Parameter | Required | Type | Description |
|-----------|----------|------|-------------|
| id | Yes | UUID v4 | Session identifier |

**Example:**
```
\x1b]777;emterm;markdown;end;id=550e8400-e29b-41d4-a716-446655440000\x1b\\
```

### 3.2 Data Structures

#### 3.2.1 Rust Types (Already Implemented)

```rust
// src-tauri/src/ansi/sequence.rs

/// OSC (Operating System Command) actions.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "action", content = "data")]
pub enum OscAction {
    // ... other variants ...

    /// OSC 777 - eMterm extension
    EmtermExtension { verb: String, params: Vec<String> },
}
```

#### 3.2.2 TypeScript Types

```typescript
// src/markdown/types.ts

/**
 * Markdown session state.
 */
interface MarkdownSession {
  /** Unique session identifier */
  id: string;
  /** Markdown format (commonmark, gfm) */
  format: "commonmark" | "gfm";
  /** Protocol version */
  version: number;
  /** Render mode */
  render: "inline" | "block";
  /** Accumulated chunks indexed by sequence number */
  chunks: Map<number, string>;
  /** Expected next sequence number */
  nextSeq: number;
  /** Session creation timestamp */
  createdAt: number;
  /** Total accumulated data size in bytes */
  dataSize: number;
}

/**
 * Parsed OSC 777 markdown command.
 */
interface MarkdownCommand {
  verb: "begin" | "chunk" | "end";
  params: Record<string, string>;
}

/**
 * Rendered Markdown block for display.
 */
interface MarkdownBlock {
  /** Block identifier (matches session id) */
  id: string;
  /** Sanitized HTML content */
  html: string;
  /** Terminal row where block starts */
  startRow: number;
  /** Number of rows occupied */
  rowCount: number;
  /** Whether block is currently visible */
  visible: boolean;
}
```

#### 3.2.3 Session Manager Interface

```typescript
// src/markdown/session.ts

/**
 * Manages Markdown rendering sessions.
 */
class MarkdownSessionManager {
  /** Maximum data size per session (2MB) */
  static readonly MAX_SESSION_SIZE = 2 * 1024 * 1024;

  /** Session timeout in milliseconds (30 seconds) */
  static readonly SESSION_TIMEOUT = 30 * 1000;

  /** Maximum concurrent sessions */
  static readonly MAX_SESSIONS = 10;

  /**
   * Handle an EmtermExtension OSC action for markdown.
   *
   * @param verb - The command verb (begin, chunk, end)
   * @param params - Command parameters as strings
   * @returns Rendered MarkdownBlock if end verb, null otherwise
   */
  handleCommand(verb: string, params: string[]): MarkdownBlock | null;

  /**
   * Get active session by ID.
   */
  getSession(id: string): MarkdownSession | undefined;

  /**
   * Clean up expired sessions.
   */
  cleanupExpiredSessions(): void;

  /**
   * Get count of active sessions.
   */
  get sessionCount(): number;
}
```

#### 3.2.4 Renderer Interface

```typescript
// src/markdown/renderer.ts

/**
 * Renders Markdown content to sanitized HTML.
 */
class MarkdownRenderer {
  /**
   * Render Markdown text to sanitized HTML.
   *
   * @param markdown - Raw Markdown text
   * @param format - Markdown format to use
   * @returns Sanitized HTML string
   */
  render(markdown: string, format: "commonmark" | "gfm"): string;

  /**
   * Insert rendered HTML into terminal display.
   *
   * @param block - Rendered Markdown block
   * @param container - Target DOM container
   */
  insertBlock(block: MarkdownBlock, container: HTMLElement): void;

  /**
   * Remove a Markdown block from display.
   *
   * @param id - Block identifier
   */
  removeBlock(id: string): void;

  /**
   * Update block visibility based on scroll position.
   *
   * @param visibleRange - Currently visible row range
   */
  updateVisibility(visibleRange: { start: number; end: number }): void;
}
```

## 4. Implementation Details

### 4.1 Backend (Rust)

The ANSI parser already handles OSC 777 and emits `EmtermExtension` actions. No additional Rust implementation is required for the basic feature.

#### 4.1.1 Existing Implementation (parser.rs L645-655)

```rust
777 => {
    // eMterm extension format: verb;param1;param2;...
    let parts: Vec<&str> = data.split(';').collect();
    if !parts.is_empty() {
        let verb = parts[0].to_string();
        let params = parts[1..].iter().map(|s| s.to_string()).collect();
        OscAction::EmtermExtension { verb, params }
    } else {
        OscAction::Unknown { ps: 777, data }
    }
}
```

#### 4.1.2 Required Enhancement

Update the OSC 777 parser to handle the nested namespace structure:

```rust
// Enhanced parsing for emterm;markdown;verb format
777 => {
    let parts: Vec<&str> = data.split(';').collect();
    if parts.len() >= 2 && parts[0] == "emterm" {
        let command = parts[1].to_string();
        let verb = parts.get(2).map(|s| s.to_string()).unwrap_or_default();
        let params = parts[3..].iter().map(|s| s.to_string()).collect();
        OscAction::EmtermExtension {
            namespace: "emterm".to_string(),
            command,
            verb,
            params
        }
    } else {
        OscAction::Unknown { ps: 777, data }
    }
}
```

### 4.2 Frontend (TypeScript)

#### 4.2.1 File Structure

```
src/
├── markdown/
│   ├── index.ts           # Module exports
│   ├── types.ts           # Type definitions
│   ├── session.ts         # Session management
│   ├── renderer.ts        # Markdown rendering
│   ├── sanitizer.ts       # DOMPurify wrapper
│   └── theme.ts           # Theme integration
└── terminal/
    └── state.ts           # Updated to delegate markdown handling
```

#### 4.2.2 Session Manager Implementation

```typescript
// src/markdown/session.ts

import { MarkdownRenderer } from "./renderer.ts";
import type { MarkdownSession, MarkdownBlock, MarkdownCommand } from "./types.ts";

export class MarkdownSessionManager {
  private sessions = new Map<string, MarkdownSession>();
  private renderer: MarkdownRenderer;
  private cleanupTimer: number | null = null;

  constructor() {
    this.renderer = new MarkdownRenderer();
    this.startCleanupTimer();
  }

  handleCommand(verb: string, params: string[]): MarkdownBlock | null {
    const parsed = this.parseParams(params);

    switch (verb) {
      case "begin":
        return this.handleBegin(parsed);
      case "chunk":
        return this.handleChunk(parsed);
      case "end":
        return this.handleEnd(parsed);
      default:
        console.warn(`Unknown markdown verb: ${verb}`);
        return null;
    }
  }

  private handleBegin(params: Record<string, string>): null {
    const id = params.id;
    if (!id) {
      console.warn("Markdown begin: missing id");
      return null;
    }

    if (this.sessions.size >= MarkdownSessionManager.MAX_SESSIONS) {
      console.warn("Markdown begin: max sessions reached");
      return null;
    }

    const session: MarkdownSession = {
      id,
      format: (params.format as "commonmark" | "gfm") || "commonmark",
      version: parseInt(params.version || "1", 10),
      render: (params.render as "inline" | "block") || "block",
      chunks: new Map(),
      nextSeq: 0,
      createdAt: Date.now(),
      dataSize: 0,
    };

    this.sessions.set(id, session);
    return null;
  }

  private handleChunk(params: Record<string, string>): null {
    const { id, seq, data } = params;

    const session = this.sessions.get(id);
    if (!session) {
      console.warn(`Markdown chunk: unknown session ${id}`);
      return null;
    }

    const seqNum = parseInt(seq, 10);
    if (isNaN(seqNum)) {
      console.warn("Markdown chunk: invalid seq");
      return null;
    }

    // Decode Base64
    let decoded: string;
    try {
      decoded = atob(data);
    } catch (e) {
      console.warn("Markdown chunk: invalid base64");
      return null;
    }

    // Check size limit
    if (session.dataSize + decoded.length > MarkdownSessionManager.MAX_SESSION_SIZE) {
      console.warn("Markdown chunk: session size limit exceeded");
      this.sessions.delete(id);
      return null;
    }

    session.chunks.set(seqNum, decoded);
    session.dataSize += decoded.length;

    return null;
  }

  private handleEnd(params: Record<string, string>): MarkdownBlock | null {
    const { id } = params;

    const session = this.sessions.get(id);
    if (!session) {
      console.warn(`Markdown end: unknown session ${id}`);
      return null;
    }

    // Assemble chunks in order
    const markdown = this.assembleChunks(session);

    // Render
    const html = this.renderer.render(markdown, session.format);

    // Cleanup session
    this.sessions.delete(id);

    return {
      id,
      html,
      startRow: 0, // To be set by caller
      rowCount: 0, // To be calculated after insertion
      visible: true,
    };
  }

  private assembleChunks(session: MarkdownSession): string {
    const sortedSeqs = Array.from(session.chunks.keys()).sort((a, b) => a - b);
    return sortedSeqs.map(seq => session.chunks.get(seq)!).join("");
  }

  private parseParams(params: string[]): Record<string, string> {
    const result: Record<string, string> = {};
    for (const param of params) {
      const eqIndex = param.indexOf("=");
      if (eqIndex > 0) {
        const key = param.substring(0, eqIndex);
        const value = param.substring(eqIndex + 1);
        result[key] = value;
      }
    }
    return result;
  }

  private startCleanupTimer(): void {
    this.cleanupTimer = window.setInterval(() => {
      this.cleanupExpiredSessions();
    }, 5000);
  }

  cleanupExpiredSessions(): void {
    const now = Date.now();
    for (const [id, session] of this.sessions) {
      if (now - session.createdAt > MarkdownSessionManager.SESSION_TIMEOUT) {
        console.warn(`Markdown session ${id} timed out`);
        this.sessions.delete(id);
      }
    }
  }

  get sessionCount(): number {
    return this.sessions.size;
  }

  dispose(): void {
    if (this.cleanupTimer !== null) {
      clearInterval(this.cleanupTimer);
    }
    this.sessions.clear();
  }
}
```

#### 4.2.3 Markdown Renderer Implementation

```typescript
// src/markdown/renderer.ts

import DOMPurify from "dompurify";
import { marked } from "marked";
import type { MarkdownBlock } from "./types.ts";

export class MarkdownRenderer {
  private purifyConfig: DOMPurify.Config;
  private blocks = new Map<string, HTMLElement>();

  constructor() {
    this.purifyConfig = {
      ALLOWED_TAGS: [
        "h1", "h2", "h3", "h4", "h5", "h6",
        "p", "br", "hr",
        "ul", "ol", "li",
        "blockquote", "pre", "code",
        "table", "thead", "tbody", "tr", "th", "td",
        "a", "strong", "em", "del", "mark",
        "img", "span", "div",
      ],
      ALLOWED_ATTR: [
        "href", "src", "alt", "title", "class",
        "id", "name", "target", "rel",
      ],
      ALLOW_DATA_ATTR: false,
      ADD_ATTR: ["target"],
      FORBID_TAGS: ["script", "style", "iframe", "object", "embed", "form"],
      FORBID_ATTR: ["onerror", "onclick", "onload", "onmouseover"],
    };

    // Configure marked
    marked.setOptions({
      gfm: true,
      breaks: true,
    });
  }

  render(markdown: string, format: "commonmark" | "gfm"): string {
    // Configure marked based on format
    marked.setOptions({
      gfm: format === "gfm",
    });

    // Parse Markdown to HTML
    const rawHtml = marked.parse(markdown) as string;

    // Sanitize HTML
    const cleanHtml = DOMPurify.sanitize(rawHtml, this.purifyConfig);

    return cleanHtml;
  }

  insertBlock(block: MarkdownBlock, container: HTMLElement): HTMLElement {
    const element = document.createElement("div");
    element.className = "markdown-block";
    element.dataset.markdownId = block.id;
    element.innerHTML = block.html;

    // Add target="_blank" and rel="noopener" to all links
    element.querySelectorAll("a").forEach(link => {
      link.setAttribute("target", "_blank");
      link.setAttribute("rel", "noopener noreferrer");
    });

    container.appendChild(element);
    this.blocks.set(block.id, element);

    return element;
  }

  removeBlock(id: string): void {
    const element = this.blocks.get(id);
    if (element) {
      element.remove();
      this.blocks.delete(id);
    }
  }

  updateVisibility(visibleRange: { start: number; end: number }): void {
    // Virtual scrolling implementation
    // Detach off-screen blocks, reattach when visible
    for (const [id, element] of this.blocks) {
      const row = parseInt(element.dataset.startRow || "0", 10);
      const height = parseInt(element.dataset.rowCount || "1", 10);

      const isVisible = row + height >= visibleRange.start && row <= visibleRange.end;

      if (isVisible && !element.parentElement) {
        // Reattach
        // Note: Need reference to container
      } else if (!isVisible && element.parentElement) {
        // Detach but keep reference
        element.remove();
      }
    }
  }

  getBlock(id: string): HTMLElement | undefined {
    return this.blocks.get(id);
  }

  dispose(): void {
    for (const element of this.blocks.values()) {
      element.remove();
    }
    this.blocks.clear();
  }
}
```

#### 4.2.4 Theme Integration

```typescript
// src/markdown/theme.ts

/**
 * CSS custom properties for Markdown theme synchronization.
 */
export interface MarkdownTheme {
  "--md-bg": string;
  "--md-fg": string;
  "--md-link": string;
  "--md-code-bg": string;
  "--md-code-fg": string;
  "--md-border": string;
  "--md-heading": string;
  "--md-blockquote": string;
}

/**
 * Generate Markdown theme from terminal colors.
 */
export function generateMarkdownTheme(
  terminalBg: string,
  terminalFg: string,
  palette: string[]
): MarkdownTheme {
  return {
    "--md-bg": terminalBg,
    "--md-fg": terminalFg,
    "--md-link": palette[4] || "#5555ff",      // Blue
    "--md-code-bg": adjustBrightness(terminalBg, 0.1),
    "--md-code-fg": palette[2] || "#55ff55",   // Green
    "--md-border": adjustBrightness(terminalFg, -0.5),
    "--md-heading": terminalFg,
    "--md-blockquote": adjustBrightness(terminalFg, -0.3),
  };
}

function adjustBrightness(color: string, factor: number): string {
  // Simple brightness adjustment implementation
  // ...
  return color;
}

/**
 * Apply theme to Markdown container.
 */
export function applyMarkdownTheme(
  container: HTMLElement,
  theme: MarkdownTheme
): void {
  for (const [prop, value] of Object.entries(theme)) {
    container.style.setProperty(prop, value);
  }
}
```

#### 4.2.5 Integration with TerminalState

```typescript
// Update src/terminal/state.ts

import { MarkdownSessionManager } from "../markdown/session.ts";

export class TerminalState {
  // ... existing code ...

  /** Markdown session manager */
  private markdownManager: MarkdownSessionManager;

  /** Rendered Markdown blocks */
  private _markdownBlocks: MarkdownBlock[] = [];

  constructor(cols: number, rows: number) {
    // ... existing initialization ...
    this.markdownManager = new MarkdownSessionManager();
  }

  /**
   * Handle OSC sequence.
   */
  private handleOsc(action: OscAction): void {
    switch (action.action) {
      // ... existing cases ...

      case "EmtermExtension":
        this.handleEmtermExtension(action.verb, action.params);
        break;
    }
  }

  /**
   * Handle eMterm extension commands.
   */
  private handleEmtermExtension(verb: string, params: string[]): void {
    // Check if this is a markdown command
    // params[0] should be "emterm", params[1] should be "markdown"
    // Already parsed by backend as: verb = "emterm", params = ["markdown", <actual_verb>, ...]

    if (params[0] === "markdown" && params.length >= 2) {
      const mdVerb = params[1];
      const mdParams = params.slice(2);

      const block = this.markdownManager.handleCommand(mdVerb, mdParams);

      if (block) {
        // Set block position based on current cursor
        block.startRow = this.cursor.row;
        this._markdownBlocks.push(block);

        // Emit event for renderer to pick up
        // Or store for later retrieval
      }
    }
  }

  /**
   * Get pending Markdown blocks for rendering.
   */
  takePendingMarkdownBlocks(): MarkdownBlock[] {
    const blocks = this._markdownBlocks;
    this._markdownBlocks = [];
    return blocks;
  }
}
```

## 5. Error Handling

### 5.1 Error Categories

| Category | Handling | User Feedback |
|----------|----------|---------------|
| Invalid verb | Log warning, ignore | None |
| Missing required param | Log warning, ignore | None |
| Invalid Base64 | Log warning, drop session | None |
| Session size exceeded | Log warning, drop session | None |
| Session timeout | Log warning, cleanup | None |
| Max sessions exceeded | Log warning, reject new | None |
| Markdown parse error | Use raw text as fallback | Display raw text |
| Sanitization error | Log error, display nothing | None |

### 5.2 Recovery Strategies

1. **Partial data**: If chunks are missing, attempt to render available content
2. **Invalid sequences**: Ignore and continue processing other sequences
3. **Resource exhaustion**: Gracefully drop oldest sessions when limits reached

## 6. Testing Strategy

### 6.1 Unit Tests

#### 6.1.1 Session Manager Tests
```typescript
// src/markdown/session.test.ts

describe("MarkdownSessionManager", () => {
  describe("handleBegin", () => {
    it("should create new session with valid params");
    it("should reject session without id");
    it("should reject when max sessions reached");
    it("should use default values for optional params");
  });

  describe("handleChunk", () => {
    it("should append decoded data to session");
    it("should reject chunk for unknown session");
    it("should reject invalid Base64 data");
    it("should enforce size limit");
  });

  describe("handleEnd", () => {
    it("should assemble chunks in order");
    it("should return rendered block");
    it("should cleanup session after end");
  });

  describe("timeout", () => {
    it("should cleanup expired sessions");
  });
});
```

#### 6.1.2 Renderer Tests
```typescript
// src/markdown/renderer.test.ts

describe("MarkdownRenderer", () => {
  describe("render", () => {
    it("should render CommonMark to HTML");
    it("should render GFM to HTML");
    it("should sanitize dangerous HTML");
    it("should remove script tags");
    it("should remove onclick attributes");
    it("should preserve safe tags");
  });

  describe("insertBlock", () => {
    it("should insert block into container");
    it("should add target=_blank to links");
  });
});
```

### 6.2 Integration Tests

```typescript
// src/markdown/integration.test.ts

describe("Markdown Display Integration", () => {
  it("should render markdown from OSC sequence");
  it("should handle chunked transfer");
  it("should handle multiple concurrent sessions");
  it("should timeout stale sessions");
  it("should respect size limits");
});
```

### 6.3 E2E Tests

```typescript
// e2e/markdown.test.ts

describe("Markdown Display E2E", () => {
  it("should display markdown from emterm markdown command");
  it("should display markdown over SSH");
  it("should handle large documents");
  it("should sync with terminal theme");
});
```

### 6.4 Security Tests

```typescript
// src/markdown/security.test.ts

describe("Markdown Security", () => {
  it("should block XSS via script tag");
  it("should block XSS via event handlers");
  it("should block XSS via javascript: URLs");
  it("should block XSS via data: URLs with scripts");
  it("should allow safe content");
});
```

## 7. Security Considerations

### 7.1 XSS Prevention

1. **DOMPurify Configuration**:
   - Whitelist-based tag filtering
   - Whitelist-based attribute filtering
   - Explicit forbid list for dangerous elements
   - No inline event handlers allowed

2. **Content Security Policy**:
   - Recommend CSP headers for WebView if supported
   - `script-src 'none'` for Markdown content area

3. **Link Handling**:
   - All links open in external browser
   - `rel="noopener noreferrer"` on all links
   - Optional: URL whitelist for allowed domains

### 7.2 Resource Protection

1. **Memory Limits**:
   - 2MB per session
   - 10 concurrent sessions max
   - Automatic cleanup of expired sessions

2. **CPU Protection**:
   - Async rendering for large documents
   - Throttled DOM updates
   - Virtual scrolling for long content

3. **Isolation**:
   - Markdown content in sandboxed container
   - No access to Tauri APIs
   - No access to terminal state

### 7.3 Input Validation

1. **UUID Validation**: Validate session ID format
2. **Base64 Validation**: Strict Base64 decoding
3. **Sequence Validation**: Integer sequence numbers only
4. **Parameter Validation**: Known parameters only

## 8. Performance Considerations

### 8.1 Rendering Performance

- Use `requestAnimationFrame` for DOM updates
- Batch multiple block insertions
- Lazy syntax highlighting for code blocks
- Virtual scrolling for long documents

### 8.2 Memory Management

- Reuse DOM elements when possible
- Detach off-screen blocks
- Cleanup sessions promptly
- Monitor memory usage in tests

### 8.3 Network Efficiency

- Efficient Base64 encoding (chunked)
- Minimal protocol overhead
- Support for streaming large documents

## 9. Dependencies

### 9.1 Required Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| marked | ^17.0.0 | Markdown parsing |
| dompurify | ^3.0.0 | HTML sanitization |
| highlight.js | ^11.0.0 | Syntax highlighting |
| mermaid | ^11.0.0 | Diagram rendering |

### 9.2 Optional Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| katex | ^0.16.0 | Math rendering |

## 10. Acceptance Criteria

### 10.1 Required

- [ ] OSC 777 `emterm;markdown` sequences are correctly parsed
- [ ] Session management handles begin/chunk/end lifecycle
- [ ] Markdown is rendered to HTML correctly
- [ ] All rendered content is XSS-safe (DOMPurify)
- [ ] Session timeout works (30 seconds)
- [ ] Session size limit works (2MB)
- [ ] Theme colors are synchronized
- [ ] Code blocks have syntax highlighting (highlight.js)
- [ ] Mermaid diagrams are rendered correctly

### 10.2 Recommended

- [ ] GFM format is supported
- [ ] Links open in external browser
- [ ] Virtual scrolling for long content
- [ ] Works over SSH connection (manual verification)

### 10.3 Performance

- [ ] Render < 100ms for 1KB Markdown
- [ ] No main thread blocking > 16ms
- [ ] Memory usage stays bounded

## 11. Implementation Phases

### Phase 1: Core Protocol (2-3 days)
- Update Rust OSC parser for namespace structure
- Implement TypeScript session manager
- Basic begin/chunk/end handling
- Unit tests

### Phase 2: Rendering (2-3 days)
- Implement MarkdownRenderer with marked
- Add DOMPurify sanitization
- Basic CSS styling
- Integration with terminal display

### Phase 3: Theme & Polish (1-2 days)
- Theme synchronization
- Link handling
- Error handling refinement
- Documentation

### Phase 4: Optional Features (ongoing)
- Syntax highlighting
- GFM extensions
- Virtual scrolling
- Math/diagrams

## 12. References

- [CommonMark Specification](https://spec.commonmark.org/)
- [GitHub Flavored Markdown Spec](https://github.github.com/gfm/)
- [DOMPurify Documentation](https://github.com/cure53/DOMPurify)
- [marked Documentation](https://marked.js.org/)
- [XTerm Control Sequences](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html)
- [OSC 777 Usage Examples](https://gitlab.gnome.org/GNOME/vte/-/issues/471)
