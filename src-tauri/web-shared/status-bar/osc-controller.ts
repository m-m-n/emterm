/**
 * OSC Layer Controller
 *
 * Manages the OSC layer of the status bar. Handles set/clear/show/hide
 * commands from OSC 777;statusbar protocol. All content is HTML-stripped
 * for security (XSS prevention).
 */

import type { StatusBarRenderer } from "./renderer";
import type { StatusBarSection } from "./types";

/**
 * Strip all HTML tags from a string for security.
 * Removes script/style tags and their content first, then strips remaining tags.
 * Preserves non-HTML angle brackets (e.g., "1 < 2 > 0").
 */
export function stripHtmlTags(input: string): string {
  if (!input) return "";

  let result = input;

  // Remove script tags and content
  result = result.replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, "");

  // Remove style tags and content
  result = result.replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, "");

  // Remove all remaining HTML tags (but not non-HTML angle brackets)
  result = result.replace(/<\/?[a-zA-Z][^>]*\/?>/g, "");

  return result;
}

/**
 * OscLayerController manages the OSC layer content.
 * Commands: set, clear, show, hide.
 */
export class OscLayerController {
  private renderer: StatusBarRenderer;

  constructor(renderer: StatusBarRenderer) {
    this.renderer = renderer;
  }

  /**
   * Handle an OSC statusbar command.
   * @param command - "set", "clear", "show", "hide"
   * @param param1 - For set: "left"|"right". For clear: optional "left"|"right"
   * @param param2 - For set: content string
   */
  handleCommand(command: string, param1?: string, param2?: string): void {
    switch (command) {
      case "set": {
        if (!param1 || (param1 !== "left" && param1 !== "right")) return;
        const section: StatusBarSection = param1;
        const content = stripHtmlTags(param2 ?? "");
        this.renderer.setContent("osc", section, content);
        // Auto-show OSC layer when content is set
        if (content) {
          this.renderer.setLayerVisible("osc", true);
        }
        break;
      }
      case "clear": {
        if (param1 === "left" || param1 === "right") {
          this.renderer.clearContent("osc", param1);
        } else {
          this.renderer.clearContent("osc");
        }
        break;
      }
      case "show":
        this.renderer.setLayerVisible("osc", true);
        break;
      case "hide":
        this.renderer.setLayerVisible("osc", false);
        break;
      default:
        // Unknown command - ignore, log at debug level
        console.debug(`[DEBUG][FRONTEND] Unknown statusbar OSC command: ${command}`);
        break;
    }
  }
}
