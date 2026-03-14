/**
 * OSC color handlers for terminal color management.
 *
 * Handles OSC 4 (palette set/query), OSC 10/11/12 (default fg/bg/cursor),
 * OSC 104 (palette reset), and OSC 110/111/112 (default color reset).
 *
 * Color spec parsing mirrors the WASM implementation in color_spec.rs.
 */

import type { Rgb } from "./colors.ts";

// ── Color spec types ────────────────────────────────────

export type ColorSpecResult =
  | { type: "color"; r: number; g: number; b: number }
  | { type: "query" };

// ── Color spec parsing (mirrors WASM color_spec.rs) ─────

/**
 * Parse a color specification string.
 * Supports: rgb:r/g/b, #RGB, #RRGGBB, #RRRRGGGGBBBB, ?
 */
export function parseColorSpec(spec: string): ColorSpecResult | null {
  spec = spec.trim();
  if (spec === "?") return { type: "query" };

  if (spec.startsWith("rgb:")) {
    return parseRgbColon(spec.slice(4));
  }
  if (spec.startsWith("#")) {
    return parseHash(spec.slice(1));
  }
  return null;
}

function parseComponent(s: string): number | null {
  const val = parseInt(s, 16);
  if (isNaN(val)) return null;
  switch (s.length) {
    case 1: return val * 17;       // 0xF -> 0xFF
    case 2: return val;
    case 4: return val >> 8;       // Downscale 16-bit to 8-bit
    default: return null;
  }
}

function parseRgbColon(s: string): ColorSpecResult | null {
  const parts = s.split("/");
  if (parts.length !== 3) return null;
  const r = parseComponent(parts[0]!);
  const g = parseComponent(parts[1]!);
  const b = parseComponent(parts[2]!);
  if (r === null || g === null || b === null) return null;
  return { type: "color", r, g, b };
}

function parseHash(s: string): ColorSpecResult | null {
  switch (s.length) {
    case 3: {
      const r = parseInt(s[0]!, 16);
      const g = parseInt(s[1]!, 16);
      const b = parseInt(s[2]!, 16);
      if (isNaN(r) || isNaN(g) || isNaN(b)) return null;
      return { type: "color", r: r * 17, g: g * 17, b: b * 17 };
    }
    case 6: {
      const r = parseInt(s.slice(0, 2), 16);
      const g = parseInt(s.slice(2, 4), 16);
      const b = parseInt(s.slice(4, 6), 16);
      if (isNaN(r) || isNaN(g) || isNaN(b)) return null;
      return { type: "color", r, g, b };
    }
    case 12: {
      const r = parseInt(s.slice(0, 4), 16);
      const g = parseInt(s.slice(4, 8), 16);
      const b = parseInt(s.slice(8, 12), 16);
      if (isNaN(r) || isNaN(g) || isNaN(b)) return null;
      return { type: "color", r: r >> 8, g: g >> 8, b: b >> 8 };
    }
    default:
      return null;
  }
}

// ── Color response formatting ───────────────────────────

/**
 * Format an 8-bit RGB color as a 16-bit xterm query response.
 * Returns "rgb:rrrr/gggg/bbbb".
 */
export function formatColorResponse(r: number, g: number, b: number): string {
  const r16 = (r << 8) | r;
  const g16 = (g << 8) | g;
  const b16 = (b << 8) | b;
  return `rgb:${r16.toString(16).padStart(4, "0")}/${g16.toString(16).padStart(4, "0")}/${b16.toString(16).padStart(4, "0")}`;
}

// ── OSC Color Handler ───────────────────────────────────

/**
 * Manages runtime palette overlay and default color overrides.
 * All state is per-terminal-session.
 */
export class OscColorHandler {
  /** 256-entry nullable palette overlay. null = use theme default. */
  private paletteOverlay: (Rgb | null)[] = new Array(256).fill(null);

  /** Default foreground color override. null = use theme default. */
  private fgOverride: Rgb | null = null;

  /** Default background color override. null = use theme default. */
  private bgOverride: Rgb | null = null;

  /** Cursor color override. null = use theme default. */
  private cursorOverride: Rgb | null = null;

  // ── Palette overlay API ─────────────────────────────

  setPaletteEntry(index: number, r: number, g: number, b: number): void {
    if (index >= 0 && index < 256) {
      this.paletteOverlay[index] = { r, g, b };
    }
  }

  getPaletteEntry(index: number): Rgb | null {
    if (index >= 0 && index < 256) {
      return this.paletteOverlay[index] ?? null;
    }
    return null;
  }

  resetPaletteEntry(index: number): void {
    if (index >= 0 && index < 256) {
      this.paletteOverlay[index] = null;
    }
  }

  resetAllPaletteEntries(): void {
    this.paletteOverlay.fill(null);
  }

  // ── Default color overrides API ─────────────────────

  setForeground(r: number, g: number, b: number): void {
    this.fgOverride = { r, g, b };
  }

  getForeground(): Rgb | null {
    return this.fgOverride;
  }

  resetForeground(): void {
    this.fgOverride = null;
  }

  setBackground(r: number, g: number, b: number): void {
    this.bgOverride = { r, g, b };
  }

  getBackground(): Rgb | null {
    return this.bgOverride;
  }

  resetBackground(): void {
    this.bgOverride = null;
  }

  setCursorColor(r: number, g: number, b: number): void {
    this.cursorOverride = { r, g, b };
  }

  getCursorColor(): Rgb | null {
    return this.cursorOverride;
  }

  resetCursorColor(): void {
    this.cursorOverride = null;
  }

  // ── OSC 4 handler ───────────────────────────────────

  /**
   * Handle OSC 4 data: "index;spec[;index;spec...]"
   * Chained pairs of (index, spec).
   */
  handleOsc4(
    data: string,
    respondFn: (response: string) => void,
    lookupDefault?: (index: number) => Rgb,
  ): void {
    const parts = data.split(";");
    let i = 0;
    while (i + 1 < parts.length) {
      const indexStr = parts[i]!;
      const spec = parts[i + 1]!;
      const index = parseInt(indexStr, 10);
      if (isNaN(index) || index < 0 || index > 255) {
        i += 2;
        continue;
      }

      const result = parseColorSpec(spec);
      if (!result) {
        i += 2;
        continue;
      }

      if (result.type === "query") {
        const entry = this.paletteOverlay[index];
        if (entry) {
          const resp = formatColorResponse(entry.r, entry.g, entry.b);
          respondFn(`\x1b]4;${index};${resp}\x1b\\`);
        } else if (lookupDefault) {
          const def = lookupDefault(index);
          const resp = formatColorResponse(def.r, def.g, def.b);
          respondFn(`\x1b]4;${index};${resp}\x1b\\`);
        }
      } else {
        this.setPaletteEntry(index, result.r, result.g, result.b);
      }

      i += 2;
    }
  }

  // ── OSC 10/11/12 handler ────────────────────────────

  /**
   * Handle OSC 10, 11, or 12 data with chaining support.
   * Data may contain multiple specs separated by `;`.
   * Index 0 = self (oscNum), index 1 = oscNum+1, index 2 = oscNum+2.
   */
  handleOscDefaultColor(
    oscNum: number,
    data: string,
    respondFn: (response: string) => void,
    lookupThemeDefault?: (oscNum: number) => Rgb | null,
  ): void {
    const specs = data.split(";");
    for (let i = 0; i < specs.length; i++) {
      const currentOsc = oscNum + i;
      if (currentOsc > 12) break; // Only 10, 11, 12

      const result = parseColorSpec(specs[i]!);
      if (!result) continue;

      if (result.type === "query") {
        const color = this.getDefaultColor(currentOsc)
          ?? (lookupThemeDefault ? lookupThemeDefault(currentOsc) : null);
        if (color) {
          const resp = formatColorResponse(color.r, color.g, color.b);
          respondFn(`\x1b]${currentOsc};${resp}\x1b\\`);
        }
      } else {
        this.setDefaultColor(currentOsc, result.r, result.g, result.b);
      }
    }
  }

  // ── OSC 104 handler ─────────────────────────────────

  /**
   * Handle OSC 104 data.
   * Empty = reset all, or semicolon-separated indices to reset.
   */
  handleOsc104(data: string): void {
    if (!data || data.trim() === "") {
      this.resetAllPaletteEntries();
      return;
    }
    for (const part of data.split(";")) {
      const index = parseInt(part.trim(), 10);
      if (!isNaN(index) && index >= 0 && index < 256) {
        this.resetPaletteEntry(index);
      }
    }
  }

  // ── Internal helpers ────────────────────────────────

  private getDefaultColor(oscNum: number): Rgb | null {
    switch (oscNum) {
      case 10: return this.fgOverride;
      case 11: return this.bgOverride;
      case 12: return this.cursorOverride;
      default: return null;
    }
  }

  private setDefaultColor(oscNum: number, r: number, g: number, b: number): void {
    switch (oscNum) {
      case 10: this.setForeground(r, g, b); break;
      case 11: this.setBackground(r, g, b); break;
      case 12: this.setCursorColor(r, g, b); break;
    }
  }
}
