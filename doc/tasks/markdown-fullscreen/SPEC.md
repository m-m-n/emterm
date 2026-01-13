# Technical Specification: Markdown Fullscreen Display

## 1. Overview

### 1.1 Purpose
Implement a fullscreen display mode for Markdown content in eMterm terminal emulator. This feature extends the existing OSC 777 Markdown display functionality to provide an immersive document viewing experience.

### 1.2 Scope
- Extension of OSC 777 protocol with new render mode (`fullscreen`)
- Fullscreen overlay UI component implementation
- Scroll and navigation functionality
- Code block copy functionality
- Link handling with confirmation dialog
- Integration with existing Markdown rendering infrastructure

### 1.3 Design Principles
- **Non-intrusive**: Fullscreen mode overlays the terminal without disrupting its state
- **Consistent**: Reuses existing Markdown rendering, theme, and security mechanisms
- **Accessible**: Supports both mouse and keyboard navigation
- **Secure**: Maintains XSS protection and requires confirmation for external links

## 2. Architecture

### 2.1 Component Design

```
┌────────────────────────────────────────────────────────────────────────┐
│                         eMterm Application                              │
├─────────────────────────────┬──────────────────────────────────────────┤
│      Rust Backend           │         TypeScript Frontend               │
├─────────────────────────────┼──────────────────────────────────────────┤
│                             │                                           │
│  ┌─────────────────────┐   │   ┌─────────────────────────────────┐    │
│  │    ANSI Parser      │   │   │    MarkdownSessionManager       │    │
│  │  (unchanged)        │   │   │  (src/markdown/session.ts)      │    │
│  └──────────┬──────────┘   │   └──────────────┬──────────────────┘    │
│             │              │                  │                        │
│             │ OSC 777      │                  │ render=fullscreen      │
│             │ EmtermExt    │                  ▼                        │
│             ▼              │   ┌─────────────────────────────────┐    │
│  ┌─────────────────────┐   │   │   FullscreenMarkdownView        │    │
│  │  OscAction::        │   │   │  (src/markdown/fullscreen.ts)   │    │
│  │  EmtermExtension    │───┼──►│  - Overlay management           │    │
│  └─────────────────────┘   │   │  - Scroll handling              │    │
│                             │   │  - Keyboard navigation          │    │
│                             │   │  - Link handling                │    │
│                             │   └──────────────┬──────────────────┘    │
│                             │                  │                        │
│                             │                  ▼                        │
│                             │   ┌─────────────────────────────────┐    │
│                             │   │   MarkdownRenderer              │    │
│                             │   │  (src/markdown/renderer.ts)     │    │
│                             │   │  - Extended with copy buttons   │    │
│                             │   └──────────────┬──────────────────┘    │
│                             │                  │                        │
│                             │                  ▼                        │
│                             │   ┌─────────────────────────────────┐    │
│                             │   │   LinkConfirmDialog             │    │
│                             │   │  (src/markdown/link-dialog.ts)  │    │
│                             │   └─────────────────────────────────┘    │
└─────────────────────────────┴──────────────────────────────────────────┘
```

### 2.2 Data Flow

```
1. PTY Output
   │
   ▼
2. ANSI Parser (Rust) - unchanged
   - Parses OSC 777 sequence
   - Emits OscAction::EmtermExtension
   │
   ▼
3. IPC Event (terminal_actions)
   │
   ▼
4. MarkdownSessionManager
   - Detects render=fullscreen
   - Triggers fullscreen display
   │
   ▼
5. FullscreenMarkdownView
   - Creates overlay element
   - Renders Markdown with copy buttons
   - Sets up event listeners
   │
   ▼
6. User Interaction
   - Scroll (mouse/keyboard)
   - Copy code
   - Click links
   - Close (Esc)
   │
   ▼
7. Cleanup
   - Remove overlay
   - Restore focus
   - Clear resources
```

## 3. Interface Design

### 3.1 OSC Protocol Extension

#### 3.1.1 New Render Mode

The existing `render` parameter is extended with a new value:

```
ESC ] 777 ; emterm ; markdown ; begin ; id=<uuid> ; render=fullscreen [; format=<fmt>] ST
```

**Render Mode Values:**

| Value | Description |
|-------|-------------|
| `inline` | Inline display within terminal output (existing) |
| `block` | Block display within terminal output (existing, default) |
| `fullscreen` | Full-window overlay display (new) |

**Example:**
```
\x1b]777;emterm;markdown;begin;id=550e8400-e29b-41d4-a716-446655440000;render=fullscreen;format=gfm\x1b\\
```

### 3.2 Data Structures

#### 3.2.1 Extended TypeScript Types

```typescript
// src/markdown/types.ts - Extended

/**
 * Render mode for Markdown blocks.
 * Extended to include fullscreen mode.
 */
export type RenderMode = "inline" | "block" | "fullscreen";

/**
 * Fullscreen view configuration.
 * Note: These settings are managed by the viewer (eMterm application),
 * not controlled via OSC protocol from the sender.
 * Future versions may expose these as application preferences.
 */
export interface FullscreenConfig {
  /** Whether to show close button (X) */
  showCloseButton: boolean;
  /** Whether to show scrollbar always */
  alwaysShowScrollbar: boolean;
  /** Whether to show copy buttons on code blocks */
  showCopyButtons: boolean;
  /**
   * Link click behavior.
   * This is a viewer-side setting, not controlled via OSC protocol.
   * Default: "confirm" (show confirmation dialog before opening links)
   * Future: May be configurable via application settings.
   */
  linkBehavior: "confirm" | "direct" | "disabled";
}

/**
 * Fullscreen view state.
 */
export interface FullscreenState {
  /** Whether fullscreen is currently active */
  isActive: boolean;
}
```

#### 3.2.2 FullscreenMarkdownView Interface

```typescript
// src/markdown/fullscreen.ts

/**
 * Manages fullscreen Markdown display.
 */
export class FullscreenMarkdownView {
  /**
   * Show Markdown content in fullscreen mode.
   *
   * @param block - Rendered Markdown block
   * @param config - Display configuration
   */
  show(block: MarkdownBlock, config?: Partial<FullscreenConfig>): void;

  /**
   * Close fullscreen view and cleanup.
   */
  close(): void;

  /**
   * Check if fullscreen view is currently active.
   */
  isActive(): boolean;

  /**
   * Scroll to position.
   *
   * @param position - Scroll position or "top" | "bottom"
   */
  scrollTo(position: number | "top" | "bottom"): void;

  /**
   * Scroll by amount.
   *
   * @param delta - Scroll delta (positive = down, negative = up)
   */
  scrollBy(delta: number): void;

  /**
   * Dispose view and release resources.
   */
  dispose(): void;
}
```

#### 3.2.3 LinkConfirmDialog Interface

```typescript
// src/markdown/link-dialog.ts

/**
 * Confirmation dialog for external links.
 */
export class LinkConfirmDialog {
  /**
   * Show confirmation dialog for URL.
   *
   * @param url - URL to confirm
   * @returns Promise resolving to true if user confirms, false if cancelled
   */
  confirm(url: string): Promise<boolean>;

  /**
   * Close dialog without action.
   */
  close(): void;

  /**
   * Check if dialog is currently shown.
   */
  isShown(): boolean;

  /**
   * Dispose dialog.
   */
  dispose(): void;
}
```

## 4. Implementation Details

### 4.1 Backend (Rust)

No changes required to the Rust backend. The existing OSC 777 parser already handles the `render` parameter and passes it to the frontend.

### 4.2 Frontend (TypeScript)

#### 4.2.1 File Structure

```
src/
├── markdown/
│   ├── index.ts              # Module exports (updated)
│   ├── types.ts              # Type definitions (extended)
│   ├── session.ts            # Session management (updated)
│   ├── renderer.ts           # Markdown rendering (extended)
│   ├── fullscreen.ts         # NEW: Fullscreen view
│   ├── fullscreen.css        # NEW: Fullscreen styles
│   ├── link-dialog.ts        # NEW: Link confirmation dialog
│   ├── copy-button.ts        # NEW: Code copy button component
│   ├── sanitizer.ts          # DOMPurify wrapper (existing)
│   └── theme.ts              # Theme integration (existing)
```

#### 4.2.2 FullscreenMarkdownView Implementation

```typescript
// src/markdown/fullscreen.ts

import { shell } from "@tauri-apps/plugin-shell";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { MarkdownRenderer } from "./renderer.ts";
import { LinkConfirmDialog } from "./link-dialog.ts";
import type { MarkdownBlock, FullscreenConfig, FullscreenState } from "./types.ts";
import "./fullscreen.css";

const DEFAULT_CONFIG: FullscreenConfig = {
  showCloseButton: false,
  alwaysShowScrollbar: true,
  showCopyButtons: true,
  linkBehavior: "confirm",
};

/**
 * Manages fullscreen Markdown display.
 */
export class FullscreenMarkdownView {
  private overlay: HTMLElement | null = null;
  private content: HTMLElement | null = null;
  private state: FullscreenState = {
    isActive: false,
  };
  private config: FullscreenConfig = DEFAULT_CONFIG;
  private linkDialog: LinkConfirmDialog;
  private boundHandleKeydown: (e: KeyboardEvent) => void;
  private boundHandleLinkClick: (e: MouseEvent) => void;
  private boundHandleCopyClick: (e: MouseEvent) => void;
  /** Element that had focus before fullscreen was opened */
  private previouslyFocusedElement: HTMLElement | null = null;

  constructor() {
    this.linkDialog = new LinkConfirmDialog();
    this.boundHandleKeydown = this.handleKeydown.bind(this);
    this.boundHandleLinkClick = this.handleLinkClick.bind(this);
    this.boundHandleCopyClick = this.handleCopyClick.bind(this);
  }

  /**
   * Show Markdown content in fullscreen mode.
   */
  show(block: MarkdownBlock, config?: Partial<FullscreenConfig>): void {
    // Close existing if any
    if (this.state.isActive) {
      this.close();
    }

    // Save currently focused element for restoration on close
    this.previouslyFocusedElement = document.activeElement as HTMLElement | null;

    this.config = { ...DEFAULT_CONFIG, ...config };

    // Create overlay
    this.overlay = document.createElement("div");
    this.overlay.className = "markdown-fullscreen-overlay";
    this.overlay.setAttribute("role", "dialog");
    this.overlay.setAttribute("aria-modal", "true");
    this.overlay.setAttribute("aria-label", "Markdown Document");

    // Create content container
    this.content = document.createElement("div");
    this.content.className = "markdown-fullscreen-content";
    this.content.innerHTML = block.html;

    // Add copy buttons to code blocks
    if (this.config.showCopyButtons) {
      this.addCopyButtons();
    }

    // Configure scrollbar
    if (this.config.alwaysShowScrollbar) {
      this.content.style.overflowY = "scroll";
    }

    // Assemble and insert
    this.overlay.appendChild(this.content);
    document.body.appendChild(this.overlay);

    // Set up event listeners
    document.addEventListener("keydown", this.boundHandleKeydown);
    this.content.addEventListener("click", this.boundHandleLinkClick);
    this.content.addEventListener("click", this.boundHandleCopyClick);

    // Update state
    this.state.isActive = true;

    // Focus for keyboard navigation
    this.content.setAttribute("tabindex", "-1");
    this.content.focus();

    console.log(`[LOG][FRONTEND] Fullscreen markdown view opened: ${block.id}`);
  }

  /**
   * Close fullscreen view and cleanup.
   */
  close(): void {
    if (!this.state.isActive) return;

    // Remove event listeners
    document.removeEventListener("keydown", this.boundHandleKeydown);
    if (this.content) {
      this.content.removeEventListener("click", this.boundHandleLinkClick);
      this.content.removeEventListener("click", this.boundHandleCopyClick);
    }

    // Close link dialog if open
    this.linkDialog.close();

    // Remove from DOM
    if (this.overlay) {
      this.overlay.remove();
      this.overlay = null;
      this.content = null;
    }

    // Restore focus to previously focused element
    if (this.previouslyFocusedElement && typeof this.previouslyFocusedElement.focus === 'function') {
      this.previouslyFocusedElement.focus();
    }
    this.previouslyFocusedElement = null;

    // Reset state
    this.state = {
      isActive: false,
    };

    console.log("[LOG][FRONTEND] Fullscreen markdown view closed");
  }

  /**
   * Check if fullscreen view is currently active.
   */
  isActive(): boolean {
    return this.state.isActive;
  }

  /**
   * Scroll to position.
   */
  scrollTo(position: number | "top" | "bottom"): void {
    if (!this.content) return;

    if (position === "top") {
      this.content.scrollTop = 0;
    } else if (position === "bottom") {
      this.content.scrollTop = this.content.scrollHeight;
    } else {
      this.content.scrollTop = position;
    }
  }

  /**
   * Scroll by amount.
   */
  scrollBy(delta: number): void {
    if (!this.content) return;
    this.content.scrollBy({ top: delta, behavior: "smooth" });
  }

  /**
   * Handle keyboard events.
   * Note: If link dialog is shown, it handles its own keyboard events.
   */
  private handleKeydown(e: KeyboardEvent): void {
    if (!this.state.isActive) return;

    // When link dialog is shown, let it handle keyboard events
    if (this.linkDialog.isShown()) {
      return;
    }

    switch (e.key) {
      case "Escape":
        e.preventDefault();
        e.stopPropagation();
        this.close();
        break;

      case "ArrowUp":
        e.preventDefault();
        this.scrollBy(-40); // ~1 line
        break;

      case "ArrowDown":
        e.preventDefault();
        this.scrollBy(40);
        break;

      case "PageUp":
        e.preventDefault();
        this.scrollBy(-(this.content?.clientHeight || 400));
        break;

      case "PageDown":
        e.preventDefault();
        this.scrollBy(this.content?.clientHeight || 400);
        break;

      case "Home":
        e.preventDefault();
        this.scrollTo("top");
        break;

      case "End":
        e.preventDefault();
        this.scrollTo("bottom");
        break;

      case "Tab":
        this.handleTabKey(e);
        break;
    }
  }

  /**
   * Handle Tab key for focus trap within fullscreen overlay.
   * Cycles focus among focusable elements (links, buttons).
   */
  private handleTabKey(e: KeyboardEvent): void {
    const focusableElements = this.content?.querySelectorAll(
      'a[href], button, [tabindex]:not([tabindex="-1"])'
    );
    if (!focusableElements?.length) return;

    const focusableArray = Array.from(focusableElements) as HTMLElement[];
    const first = focusableArray[0];
    const last = focusableArray[focusableArray.length - 1];

    if (e.shiftKey) {
      // Shift+Tab: move backward
      if (document.activeElement === first || document.activeElement === this.content) {
        e.preventDefault();
        last.focus();
      }
    } else {
      // Tab: move forward
      if (document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  }

  /**
   * Handle link clicks.
   */
  private async handleLinkClick(e: MouseEvent): Promise<void> {
    const target = e.target as HTMLElement;
    const link = target.closest("a");
    if (!link) return;

    e.preventDefault();

    const href = link.getAttribute("href");
    if (!href) return;

    // Skip non-http(s) links
    if (!href.startsWith("http://") && !href.startsWith("https://")) {
      return;
    }

    // Ctrl+Click or Cmd+Click bypasses confirmation
    const bypassConfirm = e.ctrlKey || e.metaKey;

    if (bypassConfirm || this.config.linkBehavior === "direct") {
      await this.openLink(href);
    } else if (this.config.linkBehavior === "confirm") {
      const confirmed = await this.linkDialog.confirm(href);
      if (confirmed) {
        await this.openLink(href);
      }
    }
    // linkBehavior === "disabled": do nothing
  }

  /**
   * Open link in external browser.
   */
  private async openLink(url: string): Promise<void> {
    try {
      await shell.open(url);
      console.log(`[LOG][FRONTEND] Opened external link: ${url}`);
    } catch (err) {
      console.error(`[ERROR][FRONTEND] Failed to open link: ${url}`, err);
    }
  }

  /**
   * Handle copy button clicks.
   */
  private async handleCopyClick(e: MouseEvent): Promise<void> {
    const target = e.target as HTMLElement;
    const button = target.closest(".copy-code-button");
    if (!button) return;

    e.preventDefault();
    e.stopPropagation();

    const pre = button.closest("pre");
    const code = pre?.querySelector("code");
    if (!code) return;

    const text = code.textContent || "";

    try {
      await writeText(text);
      this.showCopyFeedback(button as HTMLElement, true);
      console.log("[LOG][FRONTEND] Code copied to clipboard");
    } catch (err) {
      this.showCopyFeedback(button as HTMLElement, false);
      console.error("[ERROR][FRONTEND] Failed to copy code", err);
    }
  }

  /**
   * Add copy buttons to code blocks.
   */
  private addCopyButtons(): void {
    if (!this.content) return;

    const codeBlocks = this.content.querySelectorAll("pre > code");
    for (const code of codeBlocks) {
      const pre = code.parentElement;
      if (!pre) continue;

      // Wrap in container for positioning
      pre.style.position = "relative";

      const button = document.createElement("button");
      button.className = "copy-code-button";
      button.setAttribute("type", "button");
      button.setAttribute("aria-label", "Copy code");
      button.innerHTML = `<span class="copy-icon">Copy</span>`;

      pre.appendChild(button);
    }
  }

  /**
   * Show copy feedback on button.
   */
  private showCopyFeedback(button: HTMLElement, success: boolean): void {
    const originalText = button.innerHTML;
    button.innerHTML = success
      ? `<span class="copy-icon">Copied!</span>`
      : `<span class="copy-icon">Failed</span>`;
    button.classList.add(success ? "copy-success" : "copy-error");

    setTimeout(() => {
      button.innerHTML = originalText;
      button.classList.remove("copy-success", "copy-error");
    }, 2000);
  }

  /**
   * Dispose view and release resources.
   */
  dispose(): void {
    this.close();
    this.linkDialog.dispose();
  }
}
```

#### 4.2.3 Fullscreen CSS Styles

```css
/* src/markdown/fullscreen.css */

.markdown-fullscreen-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  z-index: 9999;
  background-color: var(--md-bg, #1e1e1e);
  display: flex;
  justify-content: center;
  overflow: hidden;
}

.markdown-fullscreen-content {
  width: 100%;
  max-width: 900px;
  height: 100%;
  padding: 2rem 3rem;
  overflow-y: scroll;
  color: var(--md-fg, #d4d4d4);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  font-size: 16px;
  line-height: 1.6;
  box-sizing: border-box;
}

/* Scrollbar styling */
.markdown-fullscreen-content::-webkit-scrollbar {
  width: 12px;
}

.markdown-fullscreen-content::-webkit-scrollbar-track {
  background: var(--md-bg, #1e1e1e);
}

.markdown-fullscreen-content::-webkit-scrollbar-thumb {
  background-color: var(--md-border, #404040);
  border-radius: 6px;
  border: 3px solid var(--md-bg, #1e1e1e);
}

.markdown-fullscreen-content::-webkit-scrollbar-thumb:hover {
  background-color: var(--md-fg, #606060);
}

/* Focus outline */
.markdown-fullscreen-content:focus {
  outline: none;
}

/* Code block container */
.markdown-fullscreen-content pre {
  position: relative;
  padding: 1rem;
  padding-right: 3.5rem; /* Space for copy button */
  background: var(--md-code-bg, #2d2d2d);
  border-radius: 6px;
  overflow-x: auto;
}

/* Copy button */
.copy-code-button {
  position: absolute;
  top: 0.5rem;
  right: 0.5rem;
  padding: 0.25rem 0.5rem;
  background: var(--md-border, #404040);
  border: none;
  border-radius: 4px;
  color: var(--md-fg, #d4d4d4);
  font-size: 12px;
  cursor: pointer;
  opacity: 0.7;
  transition: opacity 0.2s, background-color 0.2s;
}

.copy-code-button:hover {
  opacity: 1;
  background: var(--md-link, #569cd6);
}

.copy-code-button.copy-success {
  background: #4caf50;
  opacity: 1;
}

.copy-code-button.copy-error {
  background: #f44336;
  opacity: 1;
}

/* Link styling */
.markdown-fullscreen-content a {
  color: var(--md-link, #569cd6);
  text-decoration: none;
}

.markdown-fullscreen-content a:hover {
  text-decoration: underline;
}

/* Selection styling */
.markdown-fullscreen-content ::selection {
  background: var(--md-link, #569cd6);
  color: var(--md-bg, #1e1e1e);
}

/* Headings */
.markdown-fullscreen-content h1,
.markdown-fullscreen-content h2,
.markdown-fullscreen-content h3,
.markdown-fullscreen-content h4,
.markdown-fullscreen-content h5,
.markdown-fullscreen-content h6 {
  color: var(--md-heading, #ffffff);
  margin-top: 1.5em;
  margin-bottom: 0.5em;
}

.markdown-fullscreen-content h1 {
  font-size: 2em;
  border-bottom: 1px solid var(--md-border, #404040);
  padding-bottom: 0.3em;
}

.markdown-fullscreen-content h2 {
  font-size: 1.5em;
  border-bottom: 1px solid var(--md-border, #404040);
  padding-bottom: 0.3em;
}

/* Blockquotes */
.markdown-fullscreen-content blockquote {
  border-left: 4px solid var(--md-border, #404040);
  margin: 1em 0;
  padding: 0.5em 1em;
  color: var(--md-blockquote, #808080);
}

/* Tables */
.markdown-fullscreen-content table {
  width: 100%;
  border-collapse: collapse;
  margin: 1em 0;
}

.markdown-fullscreen-content th,
.markdown-fullscreen-content td {
  border: 1px solid var(--md-border, #404040);
  padding: 0.5em 1em;
}

.markdown-fullscreen-content th {
  background: var(--md-code-bg, #2d2d2d);
}

/* Task lists */
.markdown-fullscreen-content input[type="checkbox"] {
  margin-right: 0.5em;
}
```

#### 4.2.4 LinkConfirmDialog Implementation

```typescript
// src/markdown/link-dialog.ts

/**
 * Confirmation dialog for external links.
 */
export class LinkConfirmDialog {
  private dialog: HTMLElement | null = null;
  private resolvePromise: ((value: boolean) => void) | null = null;
  private boundHandleKeydown: (e: KeyboardEvent) => void;

  constructor() {
    this.boundHandleKeydown = this.handleKeydown.bind(this);
  }

  /**
   * Show confirmation dialog for URL.
   */
  confirm(url: string): Promise<boolean> {
    return new Promise((resolve) => {
      this.resolvePromise = resolve;

      // Create dialog
      this.dialog = document.createElement("div");
      this.dialog.className = "link-confirm-dialog-overlay";
      this.dialog.innerHTML = `
        <div class="link-confirm-dialog" role="alertdialog" aria-modal="true">
          <h3 class="link-confirm-title">外部リンクを開きますか？</h3>
          <p class="link-confirm-url">${this.escapeHtml(url)}</p>
          <div class="link-confirm-buttons">
            <button class="link-confirm-cancel" type="button">キャンセル</button>
            <button class="link-confirm-open" type="button">開く</button>
          </div>
        </div>
      `;

      // Event listeners
      const openBtn = this.dialog.querySelector(".link-confirm-open");
      const cancelBtn = this.dialog.querySelector(".link-confirm-cancel");

      openBtn?.addEventListener("click", () => this.handleConfirm(true));
      cancelBtn?.addEventListener("click", () => this.handleConfirm(false));
      this.dialog.addEventListener("click", (e) => {
        if (e.target === this.dialog) {
          this.handleConfirm(false);
        }
      });

      document.addEventListener("keydown", this.boundHandleKeydown);
      document.body.appendChild(this.dialog);

      // Focus open button
      (openBtn as HTMLElement)?.focus();
    });
  }

  /**
   * Handle dialog confirmation.
   */
  private handleConfirm(confirmed: boolean): void {
    if (this.resolvePromise) {
      this.resolvePromise(confirmed);
      this.resolvePromise = null;
    }
    this.close();
  }

  /**
   * Handle keyboard events.
   * Note: stopPropagation prevents parent (FullscreenMarkdownView) from handling these events.
   */
  private handleKeydown(e: KeyboardEvent): void {
    // Always stop propagation to prevent parent overlay from handling
    e.stopPropagation();

    if (e.key === "Escape") {
      e.preventDefault();
      this.handleConfirm(false);
    } else if (e.key === "Enter") {
      e.preventDefault();
      this.handleConfirm(true);
    }
  }

  /**
   * Close dialog without action.
   */
  close(): void {
    document.removeEventListener("keydown", this.boundHandleKeydown);
    if (this.dialog) {
      this.dialog.remove();
      this.dialog = null;
    }
  }

  /**
   * Check if dialog is currently shown.
   */
  isShown(): boolean {
    return this.dialog !== null;
  }

  /**
   * Escape HTML for safe display.
   */
  private escapeHtml(text: string): string {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  /**
   * Dispose dialog.
   */
  dispose(): void {
    this.close();
  }
}
```

#### 4.2.5 Session Manager Update

```typescript
// src/markdown/session.ts - Updated handleEnd method

private handleEnd(params: Record<string, string>): MarkdownBlock | null {
  const id = params.id;

  if (!id) {
    console.warn("Markdown end: missing id");
    return null;
  }

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

  const block: MarkdownBlock = {
    id,
    html,
    startRow: 0,
    rowCount: 0,
    visible: true,
  };

  // Handle fullscreen mode
  if (session.render === "fullscreen") {
    this.handleFullscreenDisplay(block);
    return null; // Fullscreen handles its own display
  }

  return block;
}

/**
 * Handle fullscreen display mode.
 */
private handleFullscreenDisplay(block: MarkdownBlock): void {
  if (!this.fullscreenView) {
    this.fullscreenView = new FullscreenMarkdownView();
  }
  this.fullscreenView.show(block);
}
```

## 5. Error Handling

### 5.1 Error Categories

| Category | Handling | User Feedback |
|----------|----------|---------------|
| Invalid render mode | Fall back to "block" | None |
| Clipboard write failure | Log error | "Failed" feedback on button |
| External link open failure | Log error | None |
| Fullscreen already active | Close existing, open new | None |

### 5.2 Recovery Strategies

1. **Escape key always works**: Esc closes fullscreen regardless of other state
2. **Dialog state**: If link dialog is open, Esc closes dialog first
3. **Memory cleanup**: Close always cleans up resources

## 6. Testing Strategy

### 6.1 Unit Tests

```typescript
// src/markdown/fullscreen.test.ts

describe("FullscreenMarkdownView", () => {
  describe("show", () => {
    it("should create overlay element");
    it("should render markdown content");
    it("should add copy buttons to code blocks");
    it("should set up keyboard listeners");
    it("should close existing view before opening new one");
  });

  describe("close", () => {
    it("should remove overlay from DOM");
    it("should clean up event listeners");
    it("should reset state");
  });

  describe("keyboard navigation", () => {
    it("should close on Escape key");
    it("should scroll down on ArrowDown");
    it("should scroll up on ArrowUp");
    it("should scroll page on PageUp/PageDown");
    it("should scroll to top on Home");
    it("should scroll to bottom on End");
  });

  describe("link handling", () => {
    it("should show confirmation dialog on link click");
    it("should bypass confirmation on Ctrl+click");
    it("should bypass confirmation on Meta+click (macOS)");
    it("should open external browser on confirmation");
    it("should not open link on cancel");
  });

  describe("copy functionality", () => {
    it("should copy code text on button click");
    it("should show success feedback on copy");
    it("should show error feedback on copy failure");
  });
});
```

```typescript
// src/markdown/link-dialog.test.ts

describe("LinkConfirmDialog", () => {
  describe("confirm", () => {
    it("should show dialog with URL");
    it("should escape HTML in URL");
    it("should resolve true on Open click");
    it("should resolve false on Cancel click");
    it("should resolve false on overlay click");
    it("should resolve false on Escape key");
    it("should resolve true on Enter key");
  });

  describe("close", () => {
    it("should remove dialog from DOM");
    it("should clean up event listeners");
  });
});
```

### 6.2 Integration Tests

```typescript
// src/markdown/fullscreen-integration.test.ts

describe("Fullscreen Markdown Integration", () => {
  it("should display fullscreen when render=fullscreen");
  it("should handle chunked markdown in fullscreen mode");
  it("should preserve theme colors in fullscreen");
  it("should handle multiple fullscreen requests");
});
```

### 6.3 E2E Tests

```typescript
// e2e/markdown-fullscreen.test.ts

describe("Markdown Fullscreen E2E", () => {
  it("should display fullscreen from emterm markdown command");
  it("should close with Escape and restore terminal");
  it("should scroll with keyboard and mouse");
  it("should copy code to clipboard");
  it("should open external links with confirmation");
});
```

## 7. Security Considerations

### 7.1 XSS Prevention

- Reuses existing DOMPurify configuration from `renderer.ts`
- No additional HTML injection points
- Link URLs are escaped in confirmation dialog

### 7.2 Link Security

- All external links require confirmation (default behavior)
- Only http:// and https:// links are processed
- Links open via Tauri `shell.open` (sandboxed)
- `rel="noopener noreferrer"` applied to all links

### 7.3 Clipboard Access

- Uses Tauri clipboard plugin (sandboxed)
- Only writes text (no images or rich content)
- User-initiated only (click event required)

## 8. Performance Considerations

### 8.1 Rendering Performance

- Fullscreen overlay uses CSS containment
- Smooth scrolling via CSS `scroll-behavior`
- Copy buttons added once on initial render

### 8.2 Memory Management

- Single fullscreen instance (closes previous before opening new)
- Event listeners properly removed on close
- DOM elements removed from document

### 8.3 Animation Performance

- CSS transitions for button feedback
- `transform` and `opacity` for animations (GPU accelerated)

## 9. Accessibility

### 9.1 Keyboard Navigation

- Full keyboard support for all operations
- Focus trapped within fullscreen overlay
- Logical tab order

### 9.2 Screen Reader Support

- `role="dialog"` on overlay
- `aria-modal="true"` to indicate modal
- `aria-label` on buttons
- `role="alertdialog"` on confirmation dialog

### 9.3 Visual Accessibility

- High contrast scrollbar
- Clear focus indicators
- Sufficient color contrast for text

## 10. Dependencies

### 10.1 Existing Dependencies (No Changes)

| Package | Version | Purpose |
|---------|---------|---------|
| marked | ^17.0.0 | Markdown parsing |
| dompurify | ^3.0.0 | HTML sanitization |
| highlight.js | ^11.0.0 | Syntax highlighting |

### 10.2 Tauri Plugins (Existing)

| Plugin | Purpose |
|--------|---------|
| @tauri-apps/plugin-shell | Open external links |
| @tauri-apps/plugin-clipboard-manager | Copy to clipboard |

## 11. Acceptance Criteria

### 11.1 Required

- [ ] `render=fullscreen` in OSC 777 triggers fullscreen overlay
- [ ] Fullscreen covers entire terminal window
- [ ] Esc key closes fullscreen and restores terminal
- [ ] Mouse wheel scrolls document
- [ ] Arrow keys scroll 1 line
- [ ] Page Up/Down scrolls 1 page
- [ ] Home/End scrolls to top/bottom
- [ ] Scrollbar is always visible
- [ ] Code blocks have copy button
- [ ] Copy button works and shows feedback
- [ ] Text selection and Ctrl+C works
- [ ] Link click shows confirmation dialog
- [ ] Ctrl+click bypasses confirmation
- [ ] External links open in browser
- [ ] Existing inline/block modes unaffected

### 11.2 Performance

- [ ] Fullscreen opens in < 100ms for 1KB Markdown
- [ ] Scrolling maintains 60fps
- [ ] No memory leaks on repeated open/close

### 11.3 Accessibility

- [ ] Keyboard navigation works without mouse
- [ ] Screen reader announces dialog
- [ ] Focus management is correct

## 12. Implementation Phases

### Phase 1: Core Fullscreen View (2-3 days)
- Implement `FullscreenMarkdownView` class
- Create overlay and content containers
- Implement Esc key close
- Add CSS styles
- Unit tests

### Phase 2: Scroll and Navigation (1-2 days)
- Implement keyboard navigation (arrows, Page Up/Down, Home/End)
- Configure scrollbar styling
- Smooth scroll behavior
- Tests

### Phase 3: Code Copy Functionality (1 day)
- Add copy buttons to code blocks
- Implement clipboard write via Tauri
- Visual feedback
- Tests

### Phase 4: Link Handling (1-2 days)
- Implement `LinkConfirmDialog`
- Ctrl+click bypass
- External browser open via Tauri
- Tests

### Phase 5: Integration and Polish (1 day)
- Integrate with session manager
- Theme synchronization
- Accessibility improvements
- E2E tests
- Documentation

## 13. References

- Existing spec: doc/tasks/markdown-display/SPEC.md
- Tauri Shell Plugin: https://v2.tauri.app/plugin/shell/
- Tauri Clipboard Plugin: https://v2.tauri.app/plugin/clipboard-manager/
- WAI-ARIA Dialog Pattern: https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/
