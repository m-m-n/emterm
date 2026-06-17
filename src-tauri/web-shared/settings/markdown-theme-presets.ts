/**
 * Markdown Theme Presets
 *
 * Defines color palettes for the Markdown viewer.
 * Each preset (Purple, Blue, Green, Orange) has both dark and light variants.
 * Colors are derived from UI_THEME_PRESETS for visual harmony.
 */

import type { UiThemePreset } from "./types";

// ============================================================
// Types
// ============================================================

export interface MarkdownThemeColors {
  /** Background color */
  bg: string;
  /** Foreground/text color */
  fg: string;
  /** Heading color */
  heading: string;
  /** Link color */
  link: string;
  /** Border color */
  border: string;
  /** Blockquote text color */
  blockquote: string;
  /** Inline code background */
  codeBg: string;
  /** Code text color */
  codeFg: string;
  /** Code block (pre) background */
  preBg: string;
  /** Table background */
  tableBg: string;
  /** Table stripe (alternating row) background */
  tableStripe: string;
}

export interface MarkdownPresetDefinition {
  dark: MarkdownThemeColors;
  light: MarkdownThemeColors;
}

// ============================================================
// Preset Definitions
// ============================================================

export const MARKDOWN_THEME_PRESETS: Record<
  UiThemePreset,
  MarkdownPresetDefinition
> = {
  purple: {
    dark: {
      bg: "#141218",
      fg: "#E6E0E9",
      heading: "#FFFFFF",
      link: "#D0BCFF",
      border: "#49454F",
      blockquote: "#CAC4D0",
      codeBg: "#2B2930",
      codeFg: "#E6E0E9",
      preBg: "#1D1B20",
      tableBg: "#211F26",
      tableStripe: "#2B2930",
    },
    light: {
      bg: "#FEF7FF",
      fg: "#1D1B20",
      heading: "#21005D",
      link: "#6750A4",
      border: "#CAC4D0",
      blockquote: "#49454F",
      codeBg: "#ECE6F0",
      codeFg: "#1D1B20",
      preBg: "#F3EDF7",
      tableBg: "#F7F2FA",
      tableStripe: "#ECE6F0",
    },
  },
  blue: {
    dark: {
      bg: "#111318",
      fg: "#E2E2E9",
      heading: "#FFFFFF",
      link: "#A8C7FA",
      border: "#44464F",
      blockquote: "#C4C6D0",
      codeBg: "#292B30",
      codeFg: "#E2E2E9",
      preBg: "#1A1C20",
      tableBg: "#1F2126",
      tableStripe: "#292B30",
    },
    light: {
      bg: "#F9F9FF",
      fg: "#1A1C20",
      heading: "#041E49",
      link: "#0B57D0",
      border: "#C4C6D0",
      blockquote: "#44464F",
      codeBg: "#E8E9EF",
      codeFg: "#1A1C20",
      preBg: "#EFF0F6",
      tableBg: "#F3F3FA",
      tableStripe: "#E8E9EF",
    },
  },
  green: {
    dark: {
      bg: "#101412",
      fg: "#DEE4DF",
      heading: "#FFFFFF",
      link: "#7DD3A8",
      border: "#404943",
      blockquote: "#BFC9C1",
      codeBg: "#262B28",
      codeFg: "#DEE4DF",
      preBg: "#181C1A",
      tableBg: "#1C201E",
      tableStripe: "#262B28",
    },
    light: {
      bg: "#F5FBF5",
      fg: "#181C1A",
      heading: "#002110",
      link: "#006D3E",
      border: "#BFC9C1",
      blockquote: "#404943",
      codeBg: "#E5EBE5",
      codeFg: "#181C1A",
      preBg: "#EBF1EB",
      tableBg: "#EFF5EF",
      tableStripe: "#E5EBE5",
    },
  },
  orange: {
    dark: {
      bg: "#18120B",
      fg: "#EFE0CF",
      heading: "#FFFFFF",
      link: "#FFB877",
      border: "#524436",
      blockquote: "#D4C4B1",
      codeBg: "#302922",
      codeFg: "#EFE0CF",
      preBg: "#211A13",
      tableBg: "#261F18",
      tableStripe: "#302922",
    },
    light: {
      bg: "#FFF8F4",
      fg: "#211A13",
      heading: "#2D1600",
      link: "#8B5000",
      border: "#D4C4B1",
      blockquote: "#524436",
      codeBg: "#EEE6E3",
      codeFg: "#211A13",
      preBg: "#F5EDEA",
      tableBg: "#FAF2EE",
      tableStripe: "#EEE6E3",
    },
  },
  pink: {
    dark: {
      bg: "#1A1114",
      fg: "#F0DEE2",
      heading: "#FFFFFF",
      link: "#FFB1C8",
      border: "#514349",
      blockquote: "#D4BFC5",
      codeBg: "#322830",
      codeFg: "#F0DEE2",
      preBg: "#221820",
      tableBg: "#271D21",
      tableStripe: "#322830",
    },
    light: {
      bg: "#FFF8F8",
      fg: "#22191C",
      heading: "#3E001D",
      link: "#984061",
      border: "#D4BFC5",
      blockquote: "#514349",
      codeBg: "#F2E4E8",
      codeFg: "#22191C",
      preBg: "#FAECEF",
      tableBg: "#FDF0F2",
      tableStripe: "#F2E4E8",
    },
  },
};

// ============================================================
// CSS Variable Mapping
// ============================================================

/** Maps MarkdownThemeColors property names to CSS variable names */
export const MARKDOWN_COLOR_TO_CSS_VAR: Record<
  keyof MarkdownThemeColors,
  string
> = {
  bg: "--markdown-bg",
  fg: "--markdown-fg",
  heading: "--markdown-heading",
  link: "--markdown-link",
  border: "--markdown-border",
  blockquote: "--markdown-blockquote",
  codeBg: "--markdown-code-bg",
  codeFg: "--markdown-code-fg",
  preBg: "--markdown-pre-bg",
  tableBg: "--markdown-table-bg",
  tableStripe: "--markdown-table-stripe",
};
