/**
 * OSC sequence handlers.
 *
 * Handles Operating System Command sequences.
 */

import type { TerminalStateAccessor } from "./types.ts";
import type { OscAction } from "../../types/terminal.ts";

/**
 * Dispatch OSC action to specific handler.
 *
 * @param state - Terminal state accessor
 * @param action - OSC action to dispatch
 */
export function handleOscDispatch(
  state: TerminalStateAccessor,
  action: OscAction
): void {
  switch (action.action) {
    case "SetTitle":
      handleSetTitle(state, action.data);
      break;
    case "SetIconName":
      handleSetIconName(state, action.data);
      break;
    case "SetTitleAndIcon":
      handleSetTitleAndIcon(state, action.data);
      break;
    case "SetColorPalette":
      // Color palette customization - could update colors.ts palette
      // For now, just log
      // console.debug(`Set color ${action.index} to ${action.color}`);
      break;
    case "SetWorkingDirectory":
      handleSetWorkingDirectory(state, action.data);
      break;
    case "Hyperlink":
      handleHyperlink(state, action.params, action.uri);
      break;
    case "SetForegroundColor":
      // Could update default foreground color
      // console.debug(`Set foreground color to ${action.data}`);
      break;
    case "SetBackgroundColor":
      // Could update default background color
      // console.debug(`Set background color to ${action.data}`);
      break;
    case "EmtermExtension":
      handleEmtermExtension(state, action.data.verb, action.data.params);
      break;
    case "SemanticPrompt":
      handleSemanticPrompt(
        state,
        action.data.zone_type,
        action.data.exit_code,
      );
      break;
    case "Unknown":
      // Unknown OSC sequences are ignored
      break;
  }
}

/**
 * Handle SetTitle (OSC 2).
 *
 * Set window title.
 *
 * @param state - Terminal state accessor
 * @param title - Window title
 */
export function handleSetTitle(
  state: TerminalStateAccessor,
  title: string
): void {
  state._title = title;
}

/**
 * Handle SetIconName (OSC 1).
 *
 * Set icon name.
 *
 * @param state - Terminal state accessor
 * @param name - Icon name
 */
export function handleSetIconName(
  state: TerminalStateAccessor,
  name: string
): void {
  state._iconName = name;
}

/**
 * Handle SetTitleAndIcon (OSC 0).
 *
 * Set both window title and icon name.
 *
 * @param state - Terminal state accessor
 * @param value - Title and icon name
 */
export function handleSetTitleAndIcon(
  state: TerminalStateAccessor,
  value: string
): void {
  state._title = value;
  state._iconName = value;
}

/**
 * Handle SetWorkingDirectory (OSC 7).
 *
 * Set current working directory.
 *
 * @param state - Terminal state accessor
 * @param path - Working directory path
 */
export function handleSetWorkingDirectory(
  state: TerminalStateAccessor,
  path: string
): void {
  state._workingDirectory = path;
}

/**
 * Handle Hyperlink (OSC 8).
 *
 * Set or clear active hyperlink.
 *
 * @param state - Terminal state accessor
 * @param params - Hyperlink parameters
 * @param uri - Hyperlink URI (empty to clear)
 */
export function handleHyperlink(
  state: TerminalStateAccessor,
  params: string,
  uri: string
): void {
  if (uri) {
    // Start hyperlink
    state._activeHyperlink = { params, uri };
  } else {
    // End hyperlink (empty URI)
    state._activeHyperlink = null;
  }
}

/**
 * Handle SemanticPrompt (OSC 133).
 *
 * Records semantic zone markers for prompt/command/output identification.
 * Only records markers when primary buffer is active (not alternate).
 *
 * @param state - Terminal state accessor
 * @param zoneType - Zone type (A/B/C/D)
 * @param exitCode - Exit code (only for type D)
 */
export function handleSemanticPrompt(
  state: TerminalStateAccessor,
  zoneType: string,
  exitCode: number | null,
): void {
  // Don't record markers in alternate buffer
  if (state.isAlternateBuffer) {
    return;
  }

  const tracker = state.getSemanticZoneTracker();
  const scrollbackLength = state.getScrollbackLength();
  const lineIndex = scrollbackLength + state.cursor.row;

  tracker.addMarker(
    zoneType,
    lineIndex,
    exitCode !== null ? exitCode : undefined,
  );

  // On D marker: detect C→D pair and register fold region
  if (zoneType === "D") {
    registerOsc133FoldRegion(state, lineIndex, exitCode);
  }
}

/**
 * Register an OSC 133 fold region when a D marker is received.
 * Looks back in markers to find matching C, and B for command text.
 */
function registerOsc133FoldRegion(
  state: TerminalStateAccessor,
  dLineIndex: number,
  exitCode: number | null,
): void {
  const tracker = state.getSemanticZoneTracker();
  const markers = tracker.getMarkers();

  // Find the most recent C marker before this D
  let cMarker: { lineIndex: number } | null = null;
  let bMarker: { lineIndex: number } | null = null;

  for (let i = markers.length - 1; i >= 0; i--) {
    const m = markers[i]!;
    // Skip the D marker we just added (last one)
    if (m.type === "D" && m.lineIndex === dLineIndex) continue;
    if (!cMarker && m.type === "C") {
      cMarker = m;
    }
    if (cMarker && m.type === "B") {
      bMarker = m;
      break;
    }
    // If we hit another D before finding C, no matching C exists
    if (m.type === "D") break;
  }

  if (!cMarker) return;

  // Extract command text from B marker line
  let commandText = "";
  if (bMarker) {
    commandText = extractLineText(state, bMarker.lineIndex);
  }

  const foldManager = state.getFoldManager();
  foldManager.registerOsc133Region(
    cMarker.lineIndex,
    dLineIndex,
    commandText,
    exitCode !== null ? exitCode : undefined,
  );
}

/**
 * Extract plain text from a line for command text display.
 */
function extractLineText(
  state: TerminalStateAccessor,
  lineIndex: number,
): string {
  const scrollbackLength = state.getScrollbackLength();
  const buffer = state.getActiveBuffer();

  let text = "";
  if (lineIndex < scrollbackLength) {
    // Line is in scrollback - not directly accessible via buffer
    // Will be populated when we have scrollback access
    return "";
  }

  const screenRow = lineIndex - scrollbackLength;
  if (screenRow < 0 || screenRow >= buffer.rows) return "";

  const line = buffer.getLine(screenRow);
  for (let col = 0; col < line.length; col++) {
    const cell = line.getCell(col);
    text += cell.char;
  }
  return text.trim();
}

/**
 * Pending fold begin marker for custom OSC fold sequences.
 */
interface PendingFoldBegin {
  lineIndex: number;
  label: string;
}

/** Pending fold begin state per terminal. Stored at module level. */
const pendingFoldBegins = new WeakMap<TerminalStateAccessor, PendingFoldBegin>();

/**
 * Handle EmtermExtension (OSC 777;emterm;...).
 *
 * Handle emterm-specific commands.
 *
 * @param state - Terminal state accessor
 * @param verb - Command verb
 * @param params - Command parameters
 */
export function handleEmtermExtension(
  state: TerminalStateAccessor,
  verb: string,
  params: string[]
): void {
  // OSC 777;emterm;... → verb="emterm", params=["subcommand", ...]
  if (verb === "emterm" && params.length > 0 && params[0] === "fold") {
    // params = ["fold", "begin", label] or ["fold", "end"]
    // Pass params from index 1 onward as fold command params
    handleFoldCommand(state, params.slice(1));
    return;
  }

  // Route to data viewer manager for json/yaml
  if (verb === "emterm" && params.length > 0 && (params[0] === "json" || params[0] === "yaml")) {
    state.getDataViewerManager().handleCommand(verb, params);
    return;
  }

  // Route to markdown manager (handles verb="emterm", params=["markdown", ...])
  const manager = state.getMarkdownManager();
  manager.handleCommand(verb, params);
}

/**
 * Handle fold command from OSC 777;emterm;fold;...
 *
 * @param state - Terminal state accessor
 * @param params - Fold parameters: ["begin", label] or ["end"]
 */
export function handleFoldCommand(
  state: TerminalStateAccessor,
  params: string[],
): void {
  if (state.isAlternateBuffer) return;

  const subCommand = params[0];
  if (subCommand === "begin") {
    const label = params[1] || "...";
    const scrollbackLength = state.getScrollbackLength();
    const lineIndex = scrollbackLength + state.cursor.row;
    // Overwrite any previous pending begin
    pendingFoldBegins.set(state, { lineIndex, label });
  } else if (subCommand === "end") {
    const pending = pendingFoldBegins.get(state);
    if (!pending) return; // Orphaned end: silently ignored

    const scrollbackLength = state.getScrollbackLength();
    const lineIndex = scrollbackLength + state.cursor.row;
    const foldManager = state.getFoldManager();
    foldManager.registerCustomRegion(pending.lineIndex, lineIndex, pending.label);
    pendingFoldBegins.delete(state);
  }
}

/**
 * Get pending fold begins map (for testing).
 */
export function _getPendingFoldBegins(): WeakMap<TerminalStateAccessor, PendingFoldBegin> {
  return pendingFoldBegins;
}
