# Feature: Color Palette Layout Redesign

## Overview

Redesign the color palette editor layout to display colors horizontally in rows, making it more compact and easier to view all colors at once.

## Objectives

- Display special colors (foreground, background, cursor, selection) in a single horizontal row
- Display standard ANSI colors (0-7) in a single horizontal row with numbered labels
- Display bright ANSI colors (8-15) in a single horizontal row with numbered labels
- Maintain existing functionality (color picking, hex input, auto-copy, save)

## User Stories

### US1: View Color Palette
As a user, I want to see all terminal colors in a compact horizontal layout, so that I can easily compare and adjust colors.

**Acceptance Criteria:**
- [ ] Special colors (4) displayed in one row
- [ ] Standard colors (8) displayed in one row with numbers 0-7
- [ ] Bright colors (8) displayed in one row with numbers 8-15
- [ ] Each color shows: label, color picker, hex input

### US2: Edit Colors
As a user, I want to edit colors using either the color picker or hex input, so that I can customize my terminal appearance.

**Acceptance Criteria:**
- [ ] Color picker changes update hex input and terminal preview
- [ ] Hex input changes update color picker and terminal preview
- [ ] Auto-copy from preset works correctly

## Technical Requirements

### Functional Requirements
- **FR1:** Special colors row displays 4 items horizontally: foreground, background, cursor, selection
- **FR2:** Standard colors row displays 8 items with numeric labels 0-7
- **FR3:** Bright colors row displays 8 items with numeric labels 8-15
- **FR4:** Each color item contains: label, color picker, hex input
- **FR5:** Section labels "標準色" and "高輝度色" displayed above respective rows

### Non-Functional Requirements
- **NFR1 - Responsive:** Grid wraps appropriately on narrow screens
- **NFR2 - Compatibility:** No changes to color saving/loading logic
- **NFR3 - Accessibility:** Maintain existing accessibility features

## Implementation Approach

### Architecture

Changes are limited to UI layer:

```
┌─────────────────────────────────────────────┐
│     settings-sections.ts (TypeScript)       │
│  - renderColorSchemeEditor (DOM structure)  │
│  - renderColorInput (component)             │
├─────────────────────────────────────────────┤
│     settings-panel.css (CSS)                │
│  - .color-palette-special (4-column grid)   │
│  - .color-palette-grid (8-column grid)      │
│  - .color-input-compact (item layout)       │
└─────────────────────────────────────────────┘
```

### Layout Specification

**Special Colors Row:**
```
┌────────────┬────────────┬────────────┬────────────┐
│ 前景色     │ 背景色     │ カーソル   │ 選択       │
│ ■ #RRGGBB │ ■ #RRGGBB │ ■ #RRGGBB │ ■ #RRGGBB │
└────────────┴────────────┴────────────┴────────────┘
```

**Standard Colors Row (with label):**
```
標準色
┌───────┬───────┬───────┬───────┬───────┬───────┬───────┬───────┐
│   0   │   1   │   2   │   3   │   4   │   5   │   6   │   7   │
│ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │
└───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┘
```

**Bright Colors Row (with label):**
```
高輝度色
┌───────┬───────┬───────┬───────┬───────┬───────┬───────┬───────┐
│   8   │   9   │  10   │  11   │  12   │  13   │  14   │  15   │
│ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │ ■ HEX │
└───────┴───────┴───────┴───────┴───────┴───────┴───────┴───────┘
```

### CSS Changes

#### settings-panel.css

**Update `.color-palette-special`:**
```css
.color-palette-special {
  display: grid;
  grid-template-columns: repeat(4, 1fr);  /* Changed from repeat(2, 1fr) */
  gap: 8px;
}
```

**Update `.color-input-compact`:**
```css
.color-input-compact {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
}

.color-input-compact .color-input-label {
  font-size: 11px;
  font-weight: 500;
  color: var(--md-sys-color-on-surface-variant);
}

.color-input-compact .color-input-group {
  display: flex;
  flex-direction: row;  /* Changed: horizontal layout */
  align-items: center;
  gap: 4px;
}

.color-input-compact .color-picker {
  width: 28px;
  height: 28px;
}

.color-input-compact .color-hex-input {
  width: 72px;
  padding: 4px 6px;
  font-size: 12px;
}
```

**Responsive adjustment:**
```css
@container settings (max-width: 599px) {
  .color-palette-special {
    grid-template-columns: repeat(2, 1fr);
  }
  .color-palette-grid {
    grid-template-columns: repeat(4, 1fr);
  }
}
```

### TypeScript Changes

#### settings-sections.ts

**Update `renderColorInput` function:**
- Add label display for compact mode (currently only shows label in non-compact mode)
- Modify compact mode to show: label (top), then color picker + hex input (horizontal)

```typescript
const renderColorInput = (
  parent: HTMLElement,
  label: string,
  value: string,
  colorKey: ColorKey,
  compact = false,
) => {
  const row = document.createElement("div");
  row.className = compact ? "color-input-compact" : "color-input-row";

  // Always show label
  const labelEl = document.createElement("span");
  labelEl.className = "color-input-label";
  labelEl.textContent = label;
  row.appendChild(labelEl);

  const inputGroup = document.createElement("div");
  inputGroup.className = "color-input-group";
  row.appendChild(inputGroup);

  // ... rest of existing logic
};
```

**Update `renderPalette` function:**
- Use `renderColorInput` with compact=true for special colors too
- This ensures consistent label+picker+hex layout for all colors

### Cleanup

- Remove `compact` parameter from `renderColorInput` (all colors use same layout)
- Remove unused `.color-input-row` CSS class and related rules
- Clear `colorPicker.title` since labels are always visible

### File Structure

No new files. Modified files:

```
src/
├── settings/
│   └── settings-sections.ts   # Update renderColorInput, renderPalette
└── styles/
    └── settings-panel.css     # Update grid layouts, remove unused rules
```

## Test Scenarios

### Visual Tests
- [ ] Special colors (4) displayed horizontally in one row
- [ ] Standard colors (8) displayed horizontally with numbers 0-7
- [ ] Bright colors (8) displayed horizontally with numbers 8-15
- [ ] Each color shows label above picker+hex group
- [ ] Layout wraps appropriately on narrow screens

### Functional Tests
- [ ] Color picker changes update hex input
- [ ] Hex input changes update color picker
- [ ] Color changes are saved correctly
- [ ] Auto-copy from preset creates new user scheme
- [ ] Existing color schemes display correctly

### Edge Cases
- [ ] Very long custom scheme names don't break layout
- [ ] Invalid hex values are handled (revert to picker value)

## Success Criteria

- [ ] All 4 special colors visible in one horizontal row
- [ ] All 8 standard colors visible in one horizontal row with 0-7 labels
- [ ] All 8 bright colors visible in one horizontal row with 8-15 labels
- [ ] Color editing functionality unchanged
- [ ] Type check passes (`bun run typecheck`)
- [ ] All tests pass

## References

- Current implementation: `src/settings/settings-sections.ts`
- CSS styles: `src/styles/settings-panel.css`
- Requirements: `doc/tasks/color-palette-layout/要件定義書.md`
