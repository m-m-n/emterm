# Implementation Plan: Color Palette Layout Redesign

## Overview

Redesign the color palette editor layout to display all colors horizontally, unifying the component structure so that special colors, standard ANSI colors, and bright ANSI colors use the same compact layout with label + color picker + hex input.

## Objectives

- Change special colors from 2-column grid to 4-column grid (horizontal row)
- Unify color input component so all colors show label + picker + hex horizontally
- Maintain all existing color editing functionality unchanged

## Prerequisites

### Development Environment
- Bun (package manager and bundler)
- TypeScript (frontend)

### Dependencies
- No new dependencies required

### Knowledge Requirements
- Understanding of the existing `renderColorInput` function in `settings-sections.ts`
- CSS Grid layout properties

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend)
- **Styling**: CSS with MD3 design tokens
- **Build**: Bun

### Design Approach
Pure UI/CSS change. No logic changes. Two files modified:
1. TypeScript: Unify `renderColorInput` to always show labels
2. CSS: Update grid columns and compact layout direction

### Component Interaction
No changes to component interaction. The `renderColorInput` function continues to be called from `renderPalette` with the same parameters. Only the DOM structure and CSS styling change.

## Implementation Phases

### Phase 1: Unify Color Input Layout

**Goal**: All color items (special and ANSI) display label + picker + hex in a consistent layout.

**Files to Modify**:
- `src/settings/settings-sections.ts`:
  - Modify `renderColorInput` to always show label element
  - Change special colors to use compact mode
- `src/styles/settings-panel.css`:
  - Update `.color-palette-special` grid to 4 columns
  - Update `.color-input-compact` to show picker + hex horizontally
  - Add `.color-input-compact .color-input-label` styles
  - Add responsive breakpoint for narrow screens

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `renderColorInput` | Render a single color item with label, picker, hex | Valid color value and key | DOM element with label, picker, hex input appended to parent |
| `renderPalette` | Render all color sections | Scheme colors loaded | Special (4-col), standard (8-col), bright (8-col) grids displayed |
| `.color-palette-special` | CSS grid for special colors | 4 child items | Items displayed in 1 row of 4 |
| `.color-palette-grid` | CSS grid for ANSI colors | 8 child items | Items displayed in 1 row of 8 |

**Processing Flow**:
```
1. renderPalette called
2. Create special colors grid (4-column)
   └─ For each special color → renderColorInput(compact=true)
3. Create "標準色" label
4. Create standard grid (8-column)
   └─ For i=0..7 → renderColorInput(compact=true)
5. Create "高輝度色" label
6. Create bright grid (8-column)
   └─ For i=8..15 → renderColorInput(compact=true)
```

**Implementation Steps**:

1. **Update `renderColorInput` to always show label**
   - Remove the `if (!compact)` guard around label creation
   - Always create and append the label element
   - Keep `compact` parameter to control CSS class assignment
   - Since label is now always visible, clear `colorPicker.title` (set to `""`) to avoid redundancy with the displayed label

2. **Change special colors to compact mode**
   - In `renderPalette`, call `renderColorInput` with `compact=true` for special colors
   - This ensures all colors use the same component structure

3. **Update CSS `.color-palette-special` grid**
   - Change `grid-template-columns` from `repeat(2, 1fr)` to `repeat(4, 1fr)`

4. **Update CSS `.color-input-compact` layout**
   - Change `.color-input-compact .color-input-group` from `flex-direction: column` to `flex-direction: row`
   - Add label styling for compact mode
   - Adjust picker and hex input sizing for horizontal fit

5. **Add responsive CSS**
   - At narrow widths, reduce special grid to 2 columns and ANSI grid to 4 columns

6. **Remove unused CSS `.color-input-row`**
   - Delete `.color-input-row` and related rules since all colors now use `.color-input-compact`
   - Also remove the `compact` parameter from `renderColorInput` (all calls use compact mode)

**Dependencies**:
- Requires: None (standalone change)
- Blocks: None

**Testing Approach**:

*Unit Tests*:
- Existing color-scheme-editor tests should pass without modification
- Type check (`bun run typecheck`) must pass

*Manual Testing*:
- [ ] Open settings → Terminal Appearance → Color section
- [ ] Verify special colors display in 1 row of 4
- [ ] Verify standard colors display in 1 row of 8 with numbers 0-7
- [ ] Verify bright colors display in 1 row of 8 with numbers 8-15
- [ ] Verify color picker works for each color
- [ ] Verify hex input works for each color
- [ ] Verify auto-copy from preset works
- [ ] Verify duplicate/delete/rename works

**Acceptance Criteria**:
- [ ] Special colors (foreground, background, cursor, selection) in 1 horizontal row
- [ ] Standard colors (0-7) in 1 horizontal row with numeric labels
- [ ] Bright colors (8-15) in 1 horizontal row with numeric labels
- [ ] Each color shows: label, color picker, hex input
- [ ] All color editing functionality unchanged
- [ ] Type check passes
- [ ] Existing tests pass

**Estimated Effort**: 小 (1-2 days)

**Risks and Mitigation**:
- **Risk**: Horizontal hex inputs may be too narrow on small screens
  - **Mitigation**: Use responsive CSS to wrap on narrow screens

## Complete File Structure

No new files. Modified files:

```
src/
├── settings/
│   └── settings-sections.ts   # Update renderColorInput, renderPalette
└── styles/
    └── settings-panel.css     # Update grid layouts, compact input styles
```

**File Descriptions**:
- `settings-sections.ts`: Contains the `renderColorSchemeEditor` function with `renderPalette` and `renderColorInput` inner functions. Changes are to the label rendering logic and compact mode usage.
- `settings-panel.css`: Contains all color palette CSS classes. Changes are to `.color-palette-special` grid, `.color-input-compact` layout direction, and responsive breakpoints.

## Testing Strategy

### Unit Testing
- Existing `color-scheme-editor.test.ts` tests cover logic layer (CRUD, naming)
- No new unit tests needed (this is a pure layout change)

### Type Check
- `bun run typecheck` must pass

### Manual Testing
- Visual verification of layout in settings panel
- Color editing workflow verification

## Dependencies

### External Dependencies
None added.

### Internal Dependencies
- `src/settings/color-scheme-editor.ts` (unchanged, provides logic)
- `src/terminal/colors.ts` (unchanged, provides color data)

## Risk Assessment

### Technical Risks

1. **Hex input width in compact horizontal layout**
   - **Risk**: HEX input may be too narrow to show full `#RRGGBB`
   - **Likelihood**: Low
   - **Impact**: Low (cosmetic)
   - **Mitigation**: Set appropriate min-width on hex input

## Open Questions

None.

## Success Metrics

### Functional Completeness
- [ ] All layout requirements from SPEC.md implemented
- [ ] All existing functionality preserved

### Quality Metrics
- [ ] Type check passes
- [ ] Existing tests pass
- [ ] No visual regressions

## References

- **Specification**: `doc/tasks/color-palette-layout/SPEC.md`
- **Requirements**: `doc/tasks/color-palette-layout/要件定義書.md`
- **Current implementation**: `src/settings/settings-sections.ts`
- **Current CSS**: `src/styles/settings-panel.css`
