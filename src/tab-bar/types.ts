/**
 * Tab Bar Type Definitions
 *
 * Type definitions for the tab bar feature.
 */

import type { TerminalApp } from "../terminal-app";
import type { UnlistenFn } from "@tauri-apps/api/event";

/**
 * Base tab interface with common properties (data only, no resource ownership)
 */
interface BaseTab {
  /** Unique tab identifier */
  id: string;
  /** Display title */
  title: string;
}

/**
 * Terminal tab with PTY session (data only)
 */
export interface TerminalTab extends BaseTab {
  type: "terminal";
  /** Associated PTY session ID */
  sessionId: string;
}

/**
 * Settings tab (singleton, no PTY)
 */
export interface SettingsTab extends BaseTab {
  type: "settings";
}

/**
 * Tab type - discriminated union for type-safe handling
 * Note: Tab is data-only; TerminalApp instances are managed by TabManager
 */
export type Tab = TerminalTab | SettingsTab;

/**
 * Type guard for terminal tabs
 */
export function isTerminalTab(tab: Tab): tab is TerminalTab {
  return tab.type === "terminal";
}

/**
 * Type guard for settings tab
 */
export function isSettingsTab(tab: Tab): tab is SettingsTab {
  return tab.type === "settings";
}

/**
 * Operation state for preventing race conditions
 */
export type TabOperationState =
  | { status: "idle" }
  | { status: "creating" }
  | { status: "closing"; tabId: string };

/**
 * Tab bar state (centralized resource management)
 */
export interface TabBarState {
  /** All tabs (data only) */
  tabs: Tab[];
  /** Currently active tab ID */
  activeTabId: string | null;
  /** Current operation state (prevents concurrent modifications) */
  operationState: TabOperationState;
  /** TerminalApp instances keyed by tab ID (centralized ownership) */
  terminalApps: Map<string, TerminalApp>;
  /** Event unlisten functions keyed by tab ID (centralized cleanup) */
  eventUnlistens: Map<string, UnlistenFn>;
}

/**
 * Tab creation options
 */
export interface CreateTabOptions {
  /** Tab type (default: 'terminal') */
  type?: "terminal" | "settings";
  /** Initial title */
  title?: string;
}

/**
 * Tab event types for EventEmitter pattern
 */
export type TabEventType =
  | "tab:created"
  | "tab:closed"
  | "tab:activated"
  | "tab:deactivated"
  | "tab:reordered"
  | "tab:titleChanged";

/**
 * Tab event payloads (typed event data)
 */
export interface TabEventPayloads {
  "tab:created": { tab: Tab };
  "tab:closed": { tabId: string; wasActive: boolean };
  "tab:activated": { tab: Tab; previousTabId: string | null };
  "tab:deactivated": { tab: Tab };
  "tab:reordered": { tabs: Tab[] };
  "tab:titleChanged": { tabId: string; title: string };
}

/**
 * Event handler type for tab events
 */
export type TabEventHandler<T extends TabEventType> = (
  payload: TabEventPayloads[T],
) => void;

/**
 * Unsubscribe function returned by on()
 */
export type UnsubscribeFn = () => void;

/**
 * Activity type for tab activity tracking
 */
export type ActivityType = "process_exit" | "output" | "bell";

/**
 * Tab event emitter interface
 */
export interface TabEventEmitter {
  on<T extends TabEventType>(
    event: T,
    handler: TabEventHandler<T>,
  ): UnsubscribeFn;
  off<T extends TabEventType>(event: T, handler: TabEventHandler<T>): void;
  emit<T extends TabEventType>(event: T, payload: TabEventPayloads[T]): void;
}
