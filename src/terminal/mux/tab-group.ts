/**
 * Mux tab group UI — manages tab group display in the tab bar.
 *
 * When a mux session is active, the originating tab transforms into
 * a "tab group" that can expand (showing window sub-tabs) or compact
 * (showing "mux (N)").
 */

import type { MuxTab } from "../../tab-bar/types";

/** Visual state of a mux tab group. */
export type MuxTabGroupState = "compact" | "expanded";

/** Mux tab group UI controller. */
export class MuxTabGroup {
  private tab: MuxTab;
  private _state: MuxTabGroupState;
  private onStateChange: ((state: MuxTabGroupState) => void) | null = null;

  constructor(tab: MuxTab) {
    this.tab = tab;
    this._state = tab.expanded ? "expanded" : "compact";
  }

  get state(): MuxTabGroupState {
    return this._state;
  }

  /** Set state change callback. */
  setOnStateChange(callback: (state: MuxTabGroupState) => void): void {
    this.onStateChange = callback;
  }

  /** Expand the tab group to show window sub-tabs. */
  expand(): void {
    if (this._state === "expanded") return;
    this._state = "expanded";
    this.tab.expanded = true;
    this.onStateChange?.("expanded");
  }

  /** Compact the tab group to show "mux (N)". */
  compact(): void {
    if (this._state === "compact") return;
    this._state = "compact";
    this.tab.expanded = false;
    this.onStateChange?.("compact");
  }

  /** Toggle between expanded and compact. */
  toggle(): void {
    if (this._state === "expanded") {
      this.compact();
    } else {
      this.expand();
    }
  }

  /** Get the compact display label. */
  getCompactLabel(): string {
    return `mux (${this.tab.windowNames.length})`;
  }

  /** Get window names for sub-tab rendering. */
  getWindowNames(): string[] {
    return this.tab.windowNames;
  }

  /** Get active window index. */
  getActiveWindowIndex(): number {
    return this.tab.activeWindowIndex;
  }

  /** Set active window. */
  setActiveWindow(index: number): void {
    if (index >= 0 && index < this.tab.windowNames.length) {
      this.tab.activeWindowIndex = index;
    }
  }

  /** Update window names (called when daemon pushes state updates). */
  updateWindowNames(names: string[]): void {
    this.tab.windowNames = names;
    if (this.tab.activeWindowIndex >= names.length) {
      this.tab.activeWindowIndex = Math.max(0, names.length - 1);
    }
  }
}
