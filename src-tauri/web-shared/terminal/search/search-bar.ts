/**
 * Search bar DOM component for in-terminal text search.
 *
 * Renders a floating search bar with input, toggle buttons, navigation,
 * and hit count display. Emits events for search operations.
 */

import { SearchStateManager, type SearchOptions } from "./search-state";

export interface SearchBarCallbacks {
  /** Called when search query or options change. */
  onSearch: (query: string, options: SearchOptions) => void;
  /** Called when user navigates to next match (Enter). */
  onNextMatch: () => void;
  /** Called when user navigates to previous match (Shift+Enter). */
  onPrevMatch: () => void;
  /** Called when search bar is closed (Esc or close button). */
  onClose: () => void;
}

/**
 * Floating search bar component for terminal text search.
 */
export class SearchBar {
  private container: HTMLElement;
  private element: HTMLElement | null = null;
  private inputEl: HTMLInputElement | null = null;
  private countEl: HTMLElement | null = null;
  private regexBtn: HTMLButtonElement | null = null;
  private caseBtn: HTMLButtonElement | null = null;
  private callbacks: SearchBarCallbacks;
  private options: SearchOptions = { isRegex: false, caseSensitive: false };
  private visible: boolean = false;
  private abortController: AbortController | null = null;

  constructor(container: HTMLElement, callbacks: SearchBarCallbacks) {
    this.container = container;
    this.callbacks = callbacks;
  }

  /**
   * Show the search bar. Creates DOM elements on first call.
   */
  show(): void {
    if (!this.element) {
      this.createElement();
    }
    this.element!.style.display = "flex";
    this.visible = true;
    this.inputEl?.focus();
    this.inputEl?.select();
  }

  /**
   * Hide the search bar.
   */
  hide(): void {
    if (this.element) {
      this.element.style.display = "none";
    }
    this.visible = false;
  }

  /**
   * Check if the search bar is currently visible.
   */
  isVisible(): boolean {
    return this.visible;
  }

  /**
   * Check if the search bar input has focus.
   */
  hasFocus(): boolean {
    return this.visible && document.activeElement === this.inputEl;
  }

  /**
   * Update the hit count display.
   */
  updateCount(current: number, total: number): void {
    if (this.countEl) {
      if (total === 0) {
        this.countEl.textContent = "No results";
      } else {
        this.countEl.textContent = `${current + 1}/${total}`;
      }
    }
  }

  /**
   * Set error state on the input field.
   */
  setError(hasError: boolean): void {
    if (this.inputEl) {
      if (hasError) {
        this.inputEl.classList.add("search-bar-error");
      } else {
        this.inputEl.classList.remove("search-bar-error");
      }
    }
  }

  /**
   * Get the current query text.
   */
  getQuery(): string {
    return this.inputEl?.value ?? "";
  }

  /**
   * Get current search options.
   */
  getOptions(): SearchOptions {
    return { ...this.options };
  }

  /**
   * Restore state from a SearchStateManager (for tab switching).
   */
  restoreState(manager: SearchStateManager): void {
    if (this.inputEl) {
      this.inputEl.value = manager.query;
    }
    this.options = { ...manager.options };
    this.updateToggleButtons();
    this.updateCount(manager.currentMatchIndex, manager.matches.length);
    this.setError(manager.error !== null);
  }

  /**
   * Dispose of DOM elements.
   */
  dispose(): void {
    // Abort all event listeners registered via the AbortController
    if (this.abortController) {
      this.abortController.abort();
      this.abortController = null;
    }
    if (this.element) {
      this.element.remove();
      this.element = null;
    }
    this.inputEl = null;
    this.countEl = null;
    this.regexBtn = null;
    this.caseBtn = null;
    this.visible = false;
  }

  /**
   * Create the search bar DOM element tree.
   */
  private createElement(): void {
    this.abortController = new AbortController();
    const { signal } = this.abortController;

    const bar = document.createElement("div");
    bar.className = "search-bar";
    bar.style.display = "none";

    // Stop keydown events from propagating to terminal when search bar is focused
    bar.addEventListener("keydown", (e) => {
      // Allow Escape to propagate so the close handler works
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        this.callbacks.onClose();
        return;
      }

      // Handle Enter/Shift+Enter for match navigation
      if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        if (e.shiftKey) {
          this.callbacks.onPrevMatch();
        } else {
          this.callbacks.onNextMatch();
        }
        return;
      }

      // Stop all other key events from reaching terminal
      e.stopPropagation();
    }, { signal });

    // Input field
    const input = document.createElement("input");
    input.type = "text";
    input.className = "search-bar-input";
    input.placeholder = "Search...";
    input.addEventListener("input", () => {
      this.callbacks.onSearch(input.value, this.options);
    }, { signal });
    this.inputEl = input;
    bar.appendChild(input);

    // Hit count
    const count = document.createElement("span");
    count.className = "search-bar-count";
    count.textContent = "No results";
    this.countEl = count;
    bar.appendChild(count);

    // Separator
    bar.appendChild(this.createSeparator());

    // Regex toggle button
    const regexBtn = this.createButton(".*", "Regular expression", () => {
      this.options.isRegex = !this.options.isRegex;
      this.updateToggleButtons();
      this.callbacks.onSearch(this.getQuery(), this.options);
    }, signal);
    this.regexBtn = regexBtn;
    bar.appendChild(regexBtn);

    // Case sensitive toggle button
    const caseBtn = this.createButton("Aa", "Match case", () => {
      this.options.caseSensitive = !this.options.caseSensitive;
      this.updateToggleButtons();
      this.callbacks.onSearch(this.getQuery(), this.options);
    }, signal);
    this.caseBtn = caseBtn;
    bar.appendChild(caseBtn);

    // Separator
    bar.appendChild(this.createSeparator());

    // Prev match button
    const prevBtn = this.createButton("\u2191", "Previous match (Shift+Enter)", () => {
      this.callbacks.onPrevMatch();
    }, signal);
    bar.appendChild(prevBtn);

    // Next match button
    const nextBtn = this.createButton("\u2193", "Next match (Enter)", () => {
      this.callbacks.onNextMatch();
    }, signal);
    bar.appendChild(nextBtn);

    // Separator
    bar.appendChild(this.createSeparator());

    // Close button
    const closeBtn = this.createButton("\u2715", "Close (Esc)", () => {
      this.callbacks.onClose();
    }, signal);
    bar.appendChild(closeBtn);

    this.element = bar;

    // Ensure container has relative positioning
    const computedPosition = window.getComputedStyle(this.container).position;
    if (computedPosition === "static") {
      this.container.style.position = "relative";
    }
    this.container.appendChild(bar);
  }

  private createButton(label: string, title: string, onClick: () => void, signal?: AbortSignal): HTMLButtonElement {
    const btn = document.createElement("button");
    btn.className = "search-bar-btn";
    btn.textContent = label;
    btn.title = title;
    btn.addEventListener("click", onClick, signal ? { signal } : undefined);
    return btn;
  }

  private createSeparator(): HTMLElement {
    const sep = document.createElement("div");
    sep.className = "search-bar-separator";
    return sep;
  }

  private updateToggleButtons(): void {
    if (this.regexBtn) {
      this.regexBtn.classList.toggle("active", this.options.isRegex);
    }
    if (this.caseBtn) {
      this.caseBtn.classList.toggle("active", this.options.caseSensitive);
    }
  }
}
