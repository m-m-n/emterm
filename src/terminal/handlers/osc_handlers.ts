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
  // Route to markdown manager
  const manager = state.getMarkdownManager();
  manager.handleCommand(verb, params);
}
