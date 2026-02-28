# Feature: Settings Navigation Collapsible Menu

## Overview

Widen the settings panel left navigation column from 200px to 300px, add SVG icons to each category, and add a collapse/expand toggle. When collapsed, the navigation shrinks to an 80px icon-only rail, keeping layout stable without affecting the content area.

## Objectives

- Improve readability of navigation labels by widening the nav column
- Add recognizable SVG icons to each settings category
- Allow users to collapse the navigation to an icon-only rail for more content space
- Maintain keyboard accessibility for all controls

## User Stories

### US1: Wider Navigation
As a user, I want a wider settings navigation, so that category labels are easier to read.

**Acceptance Criteria:**
- [ ] Navigation column width is 300px (was 200px)

### US2: Category Icons
As a user, I want icons on each settings category, so that I can quickly identify categories visually.

**Acceptance Criteria:**
- [ ] Each navigation item displays an SVG icon to the left of the label
- [ ] Icons are 24px and use `currentColor` to match text color

### US3: Collapse to Icon Rail
As a user, I want to collapse the navigation to an icon-only rail, so that I have more space for settings content.

**Acceptance Criteria:**
- [ ] A hamburger toggle button exists at the top of the navigation column
- [ ] Clicking the toggle shrinks the navigation to an 80px icon-only rail
- [ ] Only icons are visible in collapsed state (labels hidden)
- [ ] Clicking the toggle again restores the full navigation
- [ ] Clicking an icon in collapsed state switches category without expanding
- [ ] Content area layout remains stable (no shift from toggle buttons)
- [ ] Icons remain at the same horizontal position in both expanded and collapsed states

## Technical Requirements

### Functional Requirements
- **FR1:** Navigation column width changes from 200px to 300px
- **FR2:** Each category has an inline SVG icon (24px, MD3 style, `currentColor` fill)
- **FR3:** A hamburger toggle button (`≡`) is displayed at the top-left of the navigation column, left-aligned with category icons
- **FR4:** When collapsed, navigation shrinks to 80px wide (MD3 Navigation Rail standard), showing only icons (labels hidden)
- **FR5:** Clicking an icon in collapsed state switches category while staying collapsed
- **FR6:** Slide animation on collapse/expand using MD3 motion tokens (`duration-medium2` 300ms, `easing-emphasized`)
- **FR7:** Collapsed state is maintained while the settings panel instance exists. Not persisted to disk; defaults to expanded on fresh panel creation

### Non-Functional Requirements
- **NFR1 - Accessibility:** Toggle button and nav items must be keyboard-accessible with appropriate ARIA labels. Nav items must have `title` attribute in collapsed state for tooltip
- **NFR2 - Consistency:** All icons and buttons follow existing MD3 design token conventions
- **NFR3 - Visual stability:** Icon horizontal position must not shift between expanded and collapsed states
- **NFR4 - Nav item full width:** Nav items must fill the full width of the navigation column so that highlight/active backgrounds span correctly

## Implementation Approach

### Category Icons

SVG icons for each category (24px viewBox, `currentColor` fill):

| Category | Icon | Description |
|----------|------|-------------|
| UI Settings | palette | Color palette / theme |
| Keybinds | keyboard | Keyboard |
| Terminal Appearance | text_format | Typography / text formatting |
| Terminal Behavior | terminal | Terminal / console |
| Notifications | notifications | Bell |
| Markdown Viewer | article | Document / article |
| Profiles | person | Person / user |

Icons are defined as a map in `settings-panel.ts` returning SVG strings.

### Layout Changes

**Expanded state (default):**
```
grid-template-columns: 300px 1fr
```

**Collapsed state (icon rail):**
```
grid-template-columns: 80px 1fr
```

### Icon Position Alignment

To prevent icon shift between states, the icon's left offset from the nav edge must be identical:

**Expanded:** nav-padding-left(12px) + item-padding-left(16px) = **28px**
**Collapsed:** nav-padding-left(12px) + item-padding-left(16px) = **28px**

Nav items keep `padding-left: 16px` in collapsed state — only the label is hidden, icon position unchanged. Toggle button is also left-aligned at the same 28px offset to align with icons.

### Animation

CSS transitions on the grid container:

```css
.settings-panel {
  transition: grid-template-columns var(--md-motion-duration-medium2) var(--md-motion-easing-emphasized);
}
```

- Collapse: grid column slides from 300px to 80px
- Expand: grid column slides from 80px to 300px
- Duration: 300ms (`--md-motion-duration-medium2`)
- Easing: `--md-motion-easing-emphasized` (cubic-bezier(0.2, 0, 0, 1))

### Toggle Button

- Position: Top of `.settings-nav`, before the category list
- Left-aligned in both states (same horizontal position as category icons)
- Icon: Hamburger `≡` (same in both states — acts as a generic toggle)
- Style: MD3 icon button (40px, transparent background, state layer on hover)

### Nav Item Layout

**Expanded:**
```
[icon 24px] [gap 12px] [label text]
```

**Collapsed:**
```
[icon 24px] (label hidden, same padding-left)
```

Nav items use `display: flex; align-items: center; gap: 12px; width: 100%`. Items must fill the full nav width so that background highlights (active/hover states) span correctly. In collapsed state, the label is hidden via `display: none` but the icon keeps its original left padding position.

### State Management

- `SettingsPanel` class has `private navCollapsed: boolean = false`
- `toggleNavCollapsed()` toggles the CSS class only (no re-render needed)
- Clicking a nav item in collapsed state calls `switchCategory()` without changing `navCollapsed`
- The state persists as long as the SettingsPanel instance exists
- No Rust/backend changes required
- No content area DOM changes needed (toggle stays in nav)

### Dependencies

**Internal Dependencies:**
- `src/settings/settings-panel.ts`: Toggle logic, icon rendering, DOM updates
- `src/styles/settings-panel.css`: Width, collapse styles, icon layout
- `doc/UI-DESIGN-GUIDELINES.yaml`: Updated nav width, icon, and collapse specs

**External Dependencies:**
- None

### File Structure

```
src/
├── settings/
│   └── settings-panel.ts       # Toggle logic, SVG icons, DOM updates
├── styles/
│   └── settings-panel.css      # Width change, collapse styles, icon styles
doc/
└── UI-DESIGN-GUIDELINES.yaml   # Updated design specs
```

## Test Scenarios

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/settings-panel.e2e.js`, `settings-phases.e2e.js`
**Run command**: `./scripts/run-e2e-docker.sh test settings-panel.e2e.js`
- [ ] Existing settings panel E2E tests pass without regression
- [ ] Navigation displays at 300px width when expanded
- [ ] Each nav item has an SVG icon
- [ ] Collapse toggle shrinks navigation to 80px icon rail
- [ ] Only icons visible in collapsed state
- [ ] Clicking icon in collapsed state switches category without expanding
- [ ] Clicking toggle restores full navigation
- [ ] Icons do not shift position between expanded and collapsed

### Edge Cases
- [ ] Keyboard navigation (Tab, Enter/Space, Arrow keys) works in both states
- [ ] Settings panel opens in expanded state on fresh creation
- [ ] Collapse state maintained across category switches
- [ ] Content area layout remains stable across collapse/expand transitions
- [ ] Active category indicator works correctly in icon-only rail

## References

- Settings panel CSS: `src/styles/settings-panel.css`
- Settings panel TS: `src/settings/settings-panel.ts`
- UI Design Guidelines: `doc/UI-DESIGN-GUIDELINES.yaml`
- Material Design 3 Navigation Rail: reference for icon-only collapsed pattern
