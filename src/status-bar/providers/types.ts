/**
 * Variable Provider Interface
 *
 * Contract for status bar template variable providers.
 * Each provider resolves a specific variable type (time, cwd, git, cmd).
 */

/**
 * A provider that resolves a template variable value.
 */
export interface VariableProvider {
  /** Get the current value of the variable. */
  getValue(): string;

  /** Get an optional color for the variable text (CSS color string). */
  getColor?(): string | null;

  /** Clean up timers and resources. */
  dispose(): void;
}
