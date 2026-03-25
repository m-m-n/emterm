/**
 * Template Engine
 *
 * Parses template strings with {variable} placeholders and resolves them
 * using registered providers. Supports colored output for providers that
 * supply color information.
 */

import type { VariableProvider } from "./providers/types";

/** Regex to match {variable_name} patterns, including {cmd:name}. */
const VARIABLE_PATTERN = /\{([a-zA-Z_][a-zA-Z0-9_]*(?::[a-zA-Z0-9_]+)?)\}/g;

/**
 * TemplateEngine resolves template strings with variable placeholders.
 */
export class TemplateEngine {
  private providers: Map<string, VariableProvider> = new Map();

  /**
   * Extract variable names from a template string.
   * Returns all occurrences (including duplicates).
   */
  static extractVariables(template: string): string[] {
    const result: string[] = [];
    let match: RegExpExecArray | null;
    const re = new RegExp(VARIABLE_PATTERN.source, "g");
    while ((match = re.exec(template)) !== null) {
      result.push(match[1]!);
    }
    return result;
  }

  /**
   * Register a provider for a variable name.
   */
  registerProvider(name: string, provider: VariableProvider): void {
    this.providers.set(name, provider);
  }

  /**
   * Check if a provider is registered for a variable name.
   */
  hasProvider(name: string): boolean {
    return this.providers.has(name);
  }

  /**
   * Unregister a provider for a variable name.
   */
  unregisterProvider(name: string): void {
    const provider = this.providers.get(name);
    if (provider) {
      provider.dispose();
      this.providers.delete(name);
    }
  }

  /**
   * Resolve a template string, replacing variables with provider values.
   * Unknown variables are replaced with empty string.
   */
  resolve(template: string): string {
    if (!template) return "";
    return template.replace(VARIABLE_PATTERN, (_match, varName: string) => {
      const provider = this.providers.get(varName);
      return provider ? provider.getValue() : "";
    });
  }

  /**
   * Resolve a template string with color spans for providers that supply color.
   * Variables with colors are wrapped in <span style="color:...">value</span>.
   */
  resolveWithColors(template: string): string {
    if (!template) return "";
    return template.replace(VARIABLE_PATTERN, (_match, varName: string) => {
      const provider = this.providers.get(varName);
      if (!provider) return "";

      const value = provider.getValue();
      const color = provider.getColor?.();

      if (color) {
        return `<span style="color:${color}">${value}</span>`;
      }
      return value;
    });
  }

  /**
   * Dispose all registered providers.
   */
  dispose(): void {
    for (const provider of this.providers.values()) {
      provider.dispose();
    }
    this.providers.clear();
  }
}
