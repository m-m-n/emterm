/**
 * Type definitions for terminal handlers.
 *
 * Defines the interface that handlers use to access terminal state.
 */

import type { CellAttributes } from "../attributes.ts";
import type { ScreenBuffer } from "../buffer.ts";
import type { CursorState } from "../cursor.ts";
import type { TerminalModes } from "../modes.ts";
import type { CharSet } from "../../types/terminal.ts";
import type { MarkdownSessionManager } from "../../markdown/session.ts";
import type { SemanticZoneTracker } from "../semantic-zone.ts";
import type { FoldManager } from "../fold-manager.ts";

/**
 * Active character set (G0 or G1).
 */
export type ActiveCharSet = "G0" | "G1";

/**
 * Interface exposing terminal state properties to handlers.
 *
 * Handlers receive this interface to access and modify terminal state.
 * All mutations are done in-place (handlers return void).
 */
export interface TerminalStateAccessor {
  // Screen dimensions
  readonly cols: number;
  readonly rows: number;

  // Cursor access
  cursor: CursorState;

  // Mode access
  modes: TerminalModes;

  // Wrap pending flag
  wrapPending: boolean;

  // Character set state
  g0CharSet: CharSet;
  g1CharSet: CharSet;
  activeCharSet: ActiveCharSet;

  // Tab stops
  tabStops: Set<number>;

  // OSC state
  _title: string;
  _iconName: string;
  _workingDirectory: string;
  _activeHyperlink: { params: string; uri: string } | null;

  // Callbacks
  onBell?: () => void;

  // Grapheme cluster buffer for emoji sequences
  graphemeBuffer: number[];
  flushGraphemeBuffer(): void;

  // Methods for handlers to use
  getActiveBuffer(): ScreenBuffer;
  addPendingResponse(response: Uint8Array): void;
  switchToAlternateBuffer(saveCursor: boolean): void;
  switchToPrimaryBuffer(restoreCursor: boolean): void;
  getMarkdownManager(): MarkdownSessionManager;
  getSemanticZoneTracker(): SemanticZoneTracker;
  getFoldManager(): FoldManager;
  getScrollbackLength(): number;
  readonly isAlternateBuffer: boolean;
  reset(): void;
}
