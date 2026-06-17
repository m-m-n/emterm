/**
 * Git Branch Provider
 *
 * Provides {git_branch} variable with branch name and dirty/clean state color.
 * Executes git commands asynchronously via Tauri shell command infrastructure.
 */

import type { VariableProvider } from "./types";

export type GitState = "" | "clean" | "dirty" | "untracked";

/**
 * Parse branch name from `git rev-parse --abbrev-ref HEAD` output.
 * Returns empty string if output indicates error or non-git repo.
 */
export function parseGitBranch(output: string): string {
  const trimmed = output.trim();
  if (!trimmed || trimmed.startsWith("fatal:")) return "";
  return trimmed;
}

/**
 * Parse git status from `git status --porcelain` output.
 * Returns state: "clean", "dirty", "untracked".
 */
export function parseGitStatus(output: string): GitState {
  if (!output.trim()) return "clean";

  const lines = output.trim().split("\n");
  let hasTracked = false;
  let hasUntracked = false;

  for (const line of lines) {
    if (line.startsWith("??")) {
      hasUntracked = true;
    } else if (line.length > 0) {
      hasTracked = true;
    }
  }

  if (hasTracked) return "dirty";
  if (hasUntracked) return "untracked";
  return "clean";
}

/**
 * Get CSS color for a git state.
 * Returns null for empty state (not a git repo).
 */
export function getGitStateColor(state: string): string | null {
  switch (state) {
    case "clean":
      return "var(--md-sys-color-primary, #4caf50)";
    case "dirty":
      return "var(--md-sys-color-error, #f9a825)";
    case "untracked":
      return "var(--md-sys-color-on-surface-variant, #9e9e9e)";
    default:
      return null;
  }
}

/**
 * GitBranchProvider implements VariableProvider for the {git_branch} variable.
 * It polls git commands at a configurable interval.
 */
export class GitBranchProvider implements VariableProvider {
  private branch = "";
  private state: GitState = "";
  private intervalId: ReturnType<typeof setInterval> | null = null;
  private getCwd: () => string;
  private executeCommand: (cmd: string, args: string[], cwd: string) => Promise<string>;

  constructor(
    getCwd: () => string,
    executeCommand: (cmd: string, args: string[], cwd: string) => Promise<string>,
    intervalMs: number = 5000,
  ) {
    this.getCwd = getCwd;
    this.executeCommand = executeCommand;
    this.startPolling(intervalMs);
  }

  getValue(): string {
    return this.branch;
  }

  getColor(): string | null {
    return getGitStateColor(this.state);
  }

  private startPolling(intervalMs: number): void {
    // Initial fetch
    this.refresh();
    this.intervalId = setInterval(() => this.refresh(), intervalMs);
  }

  async refresh(): Promise<void> {
    const cwd = this.getCwd();
    if (!cwd) {
      this.branch = "";
      this.state = "";
      return;
    }

    try {
      const branchOutput = await this.executeCommand(
        "git",
        ["rev-parse", "--abbrev-ref", "HEAD"],
        cwd,
      );
      this.branch = parseGitBranch(branchOutput);

      if (this.branch) {
        const statusOutput = await this.executeCommand(
          "git",
          ["status", "--porcelain"],
          cwd,
        );
        this.state = parseGitStatus(statusOutput);
      } else {
        this.state = "";
      }
    } catch {
      this.branch = "";
      this.state = "";
    }
  }

  dispose(): void {
    if (this.intervalId) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }
  }
}
