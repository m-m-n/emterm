/**
 * Markdown display module.
 *
 * Provides functionality for rendering Markdown content in the terminal
 * via OSC 777 extension sequences.
 *
 * @module markdown
 */

// Types
export type {
  MarkdownFormat,
  RenderMode,
  MarkdownSession,
  MarkdownVerb,
  MarkdownCommand,
  MarkdownBlock,
  BeginParams,
  ChunkParams,
  EndParams,
  MarkdownCommandResult,
} from "./types.ts";

// Session Manager
export { MarkdownSessionManager } from "./session.ts";

// Renderer
export { MarkdownRenderer } from "./renderer.ts";

// Theme
export type { MarkdownTheme } from "./theme.ts";
export {
  generateMarkdownTheme,
  applyMarkdownTheme,
  getDarkTheme,
  getLightTheme,
} from "./theme.ts";
