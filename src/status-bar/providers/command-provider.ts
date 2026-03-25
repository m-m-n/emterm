/**
 * Custom Command Provider
 *
 * Provides {cmd:name} variables by executing user-defined commands
 * at configurable intervals. Only accepts a single executable path
 * (no arguments, no shell expansion).
 */

import type { VariableProvider } from "./types";

/**
 * CommandProvider executes a single executable and captures stdout.
 */
export class CommandProvider implements VariableProvider {
  private value = "";
  private intervalId: ReturnType<typeof setInterval> | null = null;
  private executable: string;
  private executeFn: (executable: string) => Promise<string>;

  constructor(
    executable: string,
    executeFn: (executable: string) => Promise<string>,
    intervalMs: number = 1000,
  ) {
    this.executable = executable;
    this.executeFn = executeFn;
    this.startPolling(intervalMs);
  }

  getValue(): string {
    return this.value;
  }

  getColor(): string | null {
    return null;
  }

  private startPolling(intervalMs: number): void {
    // Initial execution
    this.execute();
    this.intervalId = setInterval(() => this.execute(), intervalMs);
  }

  private async execute(): Promise<void> {
    try {
      const output = await this.executeFn(this.executable);
      this.value = output.trim();
    } catch {
      this.value = "";
    }
  }

  dispose(): void {
    if (this.intervalId) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }
  }
}
