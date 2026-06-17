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
  /** Profile-specific spawn overrides (shell, args, env, cwd) */
  spawnOverrides?: {
    shell_path?: string;
    shell_args?: string[];
    env_vars?: Record<string, string>;
    working_directory?: string;
  };
  /** SSH connection name (non-empty means SSH tab) */
  sshConnectionName?: string;
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
