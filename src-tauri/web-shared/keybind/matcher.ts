/**
 * Keybind matcher - parses keybind strings and matches against keyboard events.
 */

/** Parsed keybind representation. */
export interface ParsedKeybind {
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  key: string;
}

/**
 * Key name normalization map.
 * Maps setting-format key names to KeyboardEvent.key values.
 */
const KEY_MAP: Record<string, string> = {
  plus: "+",
  minus: "-",
  comma: ",",
  period: ".",
  slash: "/",
  backslash: "\\",
  space: " ",
  enter: "Enter",
  escape: "Escape",
  tab: "Tab",
  backspace: "Backspace",
  delete: "Delete",
  arrowup: "ArrowUp",
  arrowdown: "ArrowDown",
  arrowleft: "ArrowLeft",
  arrowright: "ArrowRight",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  pagedown: "PageDown",
  insert: "Insert",
};

/**
 * Parse a keybind string into its components.
 *
 * @param keybind - Keybind string (e.g., "Ctrl+Shift+T", "F11", "Ctrl+Plus")
 * @returns Parsed keybind object
 */
export function parseKeybind(keybind: string): ParsedKeybind {
  const parts = keybind.split("+").map((p) => p.trim());
  const result: ParsedKeybind = {
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    key: "",
  };

  for (const part of parts) {
    const lower = part.toLowerCase();
    switch (lower) {
      case "ctrl":
      case "control":
        result.ctrlKey = true;
        break;
      case "shift":
        result.shiftKey = true;
        break;
      case "alt":
        result.altKey = true;
        break;
      case "meta":
      case "cmd":
      case "command":
        result.metaKey = true;
        break;
      default:
        // This is the main key
        result.key = KEY_MAP[lower] || part;
        break;
    }
  }

  return result;
}

/**
 * Check if a keyboard event matches a parsed keybind.
 *
 * @param event - The keyboard event to check
 * @param keybind - Parsed keybind to match against
 * @returns true if the event matches the keybind
 */
export function matchKeybind(
  event: KeyboardEvent,
  keybind: ParsedKeybind,
): boolean {
  if (event.ctrlKey !== keybind.ctrlKey) return false;
  if (event.shiftKey !== keybind.shiftKey) return false;
  if (event.altKey !== keybind.altKey) return false;
  if (event.metaKey !== keybind.metaKey) return false;

  // Case-insensitive key comparison
  return event.key.toLowerCase() === keybind.key.toLowerCase();
}

/**
 * Check if a keyboard event matches a keybind string.
 *
 * @param event - The keyboard event to check
 * @param keybindStr - Keybind string (e.g., "Ctrl+Shift+T")
 * @returns true if the event matches
 */
export function matchKeybindStr(
  event: KeyboardEvent,
  keybindStr: string,
): boolean {
  return matchKeybind(event, parseKeybind(keybindStr));
}
