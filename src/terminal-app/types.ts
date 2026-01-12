/**
 * Terminal application type definitions
 */

/**
 * Options for initializing the TerminalApp
 */
export interface TerminalAppOptions {
  /** Whether to use the new terminal implementation */
  useNewTerminal?: boolean;
  /** Enable IME debug mode */
  imeDebug?: boolean;
}

/**
 * Options for the KeyboardHandler
 */
export interface KeyboardHandlerOptions {
  /** Callback for copy operation */
  onCopy?: () => Promise<void>;
  /** Callback for paste operation */
  onPaste?: () => Promise<void>;
}

/**
 * Character cell dimensions in pixels
 */
export interface CharSize {
  /** Width of a single character cell in pixels */
  width: number;
  /** Height of a single character cell in pixels */
  height: number;
}
