/**
 * Status Bar Module
 *
 * Main class for status bar lifecycle management.
 * Handles initialization, settings application, template engine wiring,
 * and cleanup.
 */

import { StatusBarRenderer } from "./renderer";
import { TemplateEngine } from "./template-engine";
import { TimeProvider } from "./providers/time-provider";
import { CwdProvider } from "./providers/cwd-provider";
import { GitBranchProvider } from "./providers/git-provider";
import { CommandProvider } from "./providers/command-provider";
import type { StatusBarConfig } from "./types";
import type { AppSettings } from "../settings/types";

/** Default refresh rates in milliseconds. */
const DEFAULT_REFRESH_RATES: Record<string, number> = {
  time: 1000,
  cwd: 5000,
  git_branch: 5000,
};

/**
 * StatusBarUI manages the status bar component lifecycle.
 */
export class StatusBarUI {
  private container: HTMLElement;
  private renderer: StatusBarRenderer | null = null;
  private templateEngine: TemplateEngine | null = null;
  private cwdProvider: CwdProvider | null = null;
  private timeProvider: TimeProvider | null = null;
  private commandProviders: Map<string, CommandProvider> = new Map();
  private enabled = false;
  private refreshTimerId: ReturnType<typeof setInterval> | null = null;
  private lastConfig: StatusBarConfig | null = null;

  /** External function to get current working directory from active terminal. */
  private getCwdFn: (() => string) | null = null;

  /** External function to execute a command (for git and custom commands). */
  private executeCommandFn:
    | ((cmd: string, args: string[], cwd: string) => Promise<string>)
    | null = null;

  constructor(container: HTMLElement) {
    this.container = container;
  }

  /**
   * Initialize the status bar (creates renderer and template engine).
   */
  init(): void {
    this.renderer = new StatusBarRenderer(this.container);
    this.templateEngine = new TemplateEngine();
    this.updateVisibility();
  }

  /**
   * Wire the CWD source function (called from main.ts after terminal is ready).
   */
  setCwdSource(fn: () => string): void {
    this.getCwdFn = fn;
  }

  /**
   * Wire the command execution function (for git provider).
   */
  setCommandExecutor(
    fn: (cmd: string, args: string[], cwd: string) => Promise<string>,
  ): void {
    this.executeCommandFn = fn;
  }

  /**
   * Apply settings to the status bar.
   * Creates/updates providers based on template variables used.
   */
  applySettings(settings: AppSettings): void {
    this.enabled = settings.statusbar_enabled;
    this.updateVisibility();

    if (!this.renderer || !this.templateEngine) return;

    const config: StatusBarConfig = {
      enabled: settings.statusbar_enabled,
      appLine1Left: settings.statusbar_app_line1_left,
      appLine1Right: settings.statusbar_app_line1_right,
      appLine2Left: settings.statusbar_app_line2_left,
      appLine2Right: settings.statusbar_app_line2_right,
      timeFormat: settings.statusbar_time_format,
      fontSize: settings.statusbar_font_size,
    };

    this.lastConfig = config;
    this.renderer.applyConfig(config);

    // Update providers based on current templates
    this.setupProviders(settings);

    // Render immediately and start refresh cycle
    this.renderTemplates();
    this.startRefreshCycle(settings);
  }

  /**
   * Update CWD from OSC 7 event (called directly, no polling needed).
   */
  updateCwd(fullPath: string): void {
    if (this.cwdProvider) {
      this.cwdProvider.setCwd(fullPath);
      // Trigger immediate re-render
      this.renderTemplates();
    }
  }

  /**
   * Get the renderer instance (for OSC controller wiring).
   */
  getRenderer(): StatusBarRenderer | null {
    return this.renderer;
  }

  /**
   * Check if the status bar is enabled.
   */
  isEnabled(): boolean {
    return this.enabled;
  }

  /**
   * Set up providers based on template variable usage.
   */
  private setupProviders(settings: AppSettings): void {
    if (!this.templateEngine) return;

    // Gather all template strings
    const allTemplates = [
      settings.statusbar_app_line1_left,
      settings.statusbar_app_line1_right,
      settings.statusbar_app_line2_left,
      settings.statusbar_app_line2_right,
    ].join(" ");

    const vars = TemplateEngine.extractVariables(allTemplates);
    const varSet = new Set(vars);

    // Time provider
    if (varSet.has("time")) {
      if (!this.timeProvider) {
        this.timeProvider = new TimeProvider(settings.statusbar_time_format);
        this.templateEngine.registerProvider("time", this.timeProvider);
      } else {
        this.timeProvider.setFormat(settings.statusbar_time_format);
      }
    }

    // CWD provider
    if (varSet.has("cwd")) {
      if (!this.cwdProvider) {
        this.cwdProvider = new CwdProvider();
        this.templateEngine.registerProvider("cwd", this.cwdProvider);
      }
      // Refresh CWD from source
      if (this.getCwdFn) {
        this.cwdProvider.setCwd(this.getCwdFn());
      }
    }

    // Git branch provider
    if (varSet.has("git_branch") && this.executeCommandFn) {
      const getCwd = this.getCwdFn ?? (() => "");
      const rate =
        settings.statusbar_refresh_rates?.["git_branch"] ??
        DEFAULT_REFRESH_RATES["git_branch"]!;

      // Check if already registered, if not create
      if (!this.templateEngine.hasProvider("git_branch")) {
        const gitProvider = new GitBranchProvider(
          getCwd,
          this.executeCommandFn,
          rate,
        );
        this.templateEngine.registerProvider("git_branch", gitProvider);
      }
    }

    // Custom command providers ({cmd:name})
    this.setupCommandProviders(settings, vars);
  }

  /**
   * Set up or update CommandProvider instances for {cmd:name} variables.
   */
  private setupCommandProviders(settings: AppSettings, vars: string[]): void {
    if (!this.templateEngine || !this.executeCommandFn) return;

    const customCommands = settings.statusbar_custom_commands ?? {};

    // Find all cmd:* variables used in templates
    const cmdVars = vars.filter((v) => v.startsWith("cmd:"));
    const activeNames = new Set(cmdVars.map((v) => v.slice(4))); // "cmd:foo" -> "foo"

    // Remove providers for commands no longer referenced
    for (const [name, provider] of this.commandProviders) {
      if (!activeNames.has(name)) {
        provider.dispose();
        this.commandProviders.delete(name);
        this.templateEngine.unregisterProvider(`cmd:${name}`);
      }
    }

    // Create providers for new cmd:* variables that have matching settings
    const executeFn = this.executeCommandFn;
    for (const name of activeNames) {
      const cmdConfig = customCommands[name];
      if (!cmdConfig?.executable) continue;

      // Skip if already registered
      if (this.commandProviders.has(name)) continue;

      const intervalMs = cmdConfig.interval_ms ?? 1000;
      const provider = new CommandProvider(
        cmdConfig.executable,
        async (executable: string) => {
          return await executeFn(executable, [], "");
        },
        intervalMs,
      );
      this.commandProviders.set(name, provider);
      this.templateEngine.registerProvider(`cmd:${name}`, provider);
    }
  }

  /**
   * Render template strings to the renderer.
   */
  private renderTemplates(): void {
    if (!this.renderer || !this.templateEngine || !this.lastConfig) return;

    const engine = this.templateEngine;
    const config = this.lastConfig;

    // Resolve templates with color support
    this.renderer.setContent(
      "app-line1",
      "left",
      engine.resolveWithColors(config.appLine1Left),
    );
    this.renderer.setContent(
      "app-line1",
      "right",
      engine.resolveWithColors(config.appLine1Right),
    );
    this.renderer.setContent(
      "app-line2",
      "left",
      engine.resolveWithColors(config.appLine2Left),
    );
    this.renderer.setContent(
      "app-line2",
      "right",
      engine.resolveWithColors(config.appLine2Right),
    );
  }

  /**
   * Start the periodic refresh cycle for template variables.
   */
  private startRefreshCycle(settings: AppSettings): void {
    this.stopRefreshCycle();

    if (!this.enabled) return;

    // Use the fastest refresh rate among used variables
    const timeRate =
      settings.statusbar_refresh_rates?.["time"] ??
      DEFAULT_REFRESH_RATES["time"]!;

    this.refreshTimerId = setInterval(() => {
      if (this.enabled) {
        // Update CWD from source if available
        if (this.cwdProvider && this.getCwdFn) {
          this.cwdProvider.setCwd(this.getCwdFn());
        }
        this.renderTemplates();
      }
    }, timeRate);
  }

  /**
   * Stop the refresh cycle.
   */
  private stopRefreshCycle(): void {
    if (this.refreshTimerId) {
      clearInterval(this.refreshTimerId);
      this.refreshTimerId = null;
    }
  }

  /**
   * Update container visibility based on enabled state.
   */
  private updateVisibility(): void {
    this.container.classList.toggle("hidden", !this.enabled);
  }

  /**
   * Clean up resources.
   */
  dispose(): void {
    this.stopRefreshCycle();
    // Dispose command providers (templateEngine.dispose() also disposes, but clear our map)
    for (const provider of this.commandProviders.values()) {
      provider.dispose();
    }
    this.commandProviders.clear();
    this.templateEngine?.dispose();
    this.templateEngine = null;
    this.cwdProvider = null;
    this.timeProvider = null;
    this.renderer?.dispose();
    this.renderer = null;
  }
}
