# Implementation Plan: Settings Panel - Phase 1

## Overview

Extend the settings panel from font_size-only to support three categories (Appearance, Terminal, Keybinds) with new settings fields and UI controls. Phase 1 delivers the foundational type extensions, category navigation, and first batch of settings items.

## Objectives

- Extend Rust `AppSettings` and TypeScript `AppSettings` with all fields defined in SPEC.md (all phases' fields added now for backward compatibility)
- Add `KeybindSettings` struct/interface on both Rust and TypeScript sides
- Enable all three category tabs (Appearance, Terminal, Keybinds)
- Implement Phase 1 settings items: font_family, line_height, ui_theme, cursor_style, cursor_blink, and 9 keybinds
- Implement four new UI controls: text input, select dropdown, toggle switch, keybind capture input
- Add CSS styles for all new UI controls following MD3 design

## Prerequisites

### Development Environment
- Rust toolchain with Cargo
- Bun package manager
- Tauri development environment (`bun tauri dev`)

### Dependencies
- Existing `@tauri-apps/api` for Tauri invoke
- Existing `serde` / `serde_json` for Rust serialization

### Knowledge Requirements
- Current settings panel architecture (SettingsPanel, SettingsService, SettingsApplier pattern)
- Material Design 3 token system (CSS custom properties already defined in styles.css)
- ARIA tablist/tab/tabpanel pattern (already implemented for navigation)

## Architecture Overview

### Design Approach

All settings fields from all phases are added to the Rust and TypeScript type definitions in Phase 1 to ensure backward compatibility from day one. The `#[serde(default)]` pattern ensures that old settings files (with only `font_size`) deserialize correctly with defaults for all new fields.

The settings panel UI is extended incrementally -- only Phase 1 settings items are rendered in the panel, but the data structures support all future fields.

### Component Interaction

```
User Input
    |
    v
SettingsPanel (renders controls per category)
    |
    +---> SettingsApplier.apply*() --> CSS variables / renderer notification
    |
    +---> SettingsService.save() --> Tauri invoke --> config.rs --> settings.json
```

## Implementation Steps

### Step 1: Extend Rust Type Definitions

**Goal**: All settings fields exist in Rust with serde defaults; validation covers all fields.

**Files to Modify**:
- `src-tauri/src/commands/config.rs`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| AppSettings struct | Hold all settings fields with serde defaults and null-safe deserialization | None | All fields populated on deserialization, even from `{}` or null values |
| Enum types (UiTheme, CursorStyle, BellAction, ScrollbarMode) | Type-safe enum definitions with serde rename_all | None | Invalid values rejected at deserialization |
| KeybindSettings struct | Hold all keybind mappings with serde defaults | None | All keybind fields populated with default key combinations |
| Validation constants | Define valid ranges for numeric fields | None | Constants available for validation logic |
| default_* functions | Provide default values for each field | None | Each function returns the correct default per SPEC |
| impl Default for AppSettings | Manual Default implementation using default_* functions | default_* functions defined | AppSettings::default() returns correct custom defaults (not type zeros) |
| impl Default for KeybindSettings | Manual Default implementation using default_keybind_* functions | default_keybind_* functions defined | KeybindSettings::default() returns correct keybind defaults |
| deserialize_null_default | Custom deserializer that treats null as default value | impl Default returning correct values | JSON null values become custom defaults via unwrap_or_default() |
| save_settings validation | Reject invalid values for all fields | Settings passed from frontend | Returns error for out-of-range numeric values |
| load_settings | Deserialize with defaults for missing/null fields | Settings file may have any subset of fields | Returns fully populated AppSettings |

**Processing Flow**:
```
1. Define enum types: UiTheme, CursorStyle, BellAction, ScrollbarMode
   +-- Each uses #[derive(Default)] with #[default] on the default variant
   +-- Each uses #[serde(rename_all = "lowercase")] for JSON compatibility
2. Add deserialize_null_default helper function for null-safe deserialization
3. Add all fields to AppSettings struct with:
   +-- #[serde(default = "default_*")] for custom defaults
   +-- #[serde(deserialize_with = "deserialize_null_default")] for null handling
4. Add KeybindSettings struct with all keybind fields
   +-- All fields use #[serde(deserialize_with = "deserialize_null_default")] for null handling
5. Add validation constants (numeric ranges)
6. Add default value functions for numeric fields and keybind defaults
7. Implement manual Default for AppSettings and KeybindSettings
   +-- Uses default_* functions (not #[derive(Default)] which returns type zeros)
   +-- Ensures deserialize_null_default returns correct custom defaults
8. Extend save_settings validation
   +-- Numeric fields: check min/max range, return error if out of range
   +-- Enum fields: type-safe via Rust enum (invalid values rejected at deserialization)
   +-- String fields: no validation (accept any)
```

**Key Considerations**:
- All fields use `#[serde(default)]` + `deserialize_null_default` for backward compatibility and null handling
- Enum fields (ui_theme, cursor_style, bell_action, show_scrollbar) use Rust enums for type safety
- Unknown fields in JSON are ignored (serde default behavior)
- Validation in `save_settings` returns errors for numeric range violations; enum validation is handled by serde deserialization

**Testing Approach**:

*Unit Tests*:
- Deserialization of `{}` produces all defaults
- Deserialization of `{"font_size": 13}` (old format) produces defaults for new fields
- Deserialization of `{"font_size": null}` produces default font_size
- Deserialization of invalid enum value (e.g., `{"ui_theme": "invalid"}`) produces error
- `save_settings` rejects each invalid numeric range value
- `save_settings` accepts valid complete settings
- `KeybindSettings` defaults match SPEC values
- Round-trip: serialize then deserialize preserves all fields

**Acceptance Criteria**:
- [ ] `AppSettings` has all fields from SPEC (all phases)
- [ ] `UiTheme`, `CursorStyle`, `BellAction`, `ScrollbarMode` enums defined
- [ ] `KeybindSettings` has all 13 keybind fields
- [ ] `deserialize_null_default` helper implemented
- [ ] `cargo test` passes with all new tests
- [ ] Old settings file `{"font_size": 16}` loads without error
- [ ] Settings file with null values (`{"font_size": null}`) loads with defaults

---

### Step 2: Extend TypeScript Type Definitions

**Goal**: TypeScript types mirror Rust types exactly; validation constants available for frontend.

**Files to Modify**:
- `src/settings/types.ts`
- `src/settings/index.ts`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| AppSettings interface | Mirror Rust AppSettings for JSON deserialization | Rust struct defined | Interface fields match Rust struct exactly |
| KeybindSettings interface | Mirror Rust KeybindSettings | Rust struct defined | Interface fields match Rust struct |
| Type aliases | Define union types for enum fields | None | UiTheme, CursorStyle, BellAction, ScrollbarMode types defined |
| Validation constants | Match Rust validation ranges | Rust constants defined | All MIN/MAX/STEP constants exported |

**Key Considerations**:
- Field names use snake_case to match JSON serialization
- Type aliases provide compile-time safety for enum-like fields
- Constants must match Rust-side values exactly

**Acceptance Criteria**:
- [ ] `AppSettings` interface has all fields from SPEC
- [ ] `KeybindSettings` interface has all 13 keybind fields
- [ ] All type aliases defined (UiTheme, CursorStyle, BellAction, ScrollbarMode)
- [ ] All validation constants exported
- [ ] `bun run typecheck` passes

---

### Step 3: Extend Settings Applier

**Goal**: New apply functions update CSS variables and notify renderers for Phase 1 settings.

**Files to Modify**:
- `src/settings/settings-applier.ts`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| applySettings | Call all individual apply functions | Full AppSettings loaded | All visual settings applied |
| applyFontFamily | Set --terminal-font-family CSS variable | Valid font family string | CSS variable updated; renderers notified |
| applyLineHeight | Set --terminal-line-height CSS variable | Valid line height number | CSS variable updated; renderers notified |
| applyUiTheme | Set data-theme attribute on documentElement | Valid theme string | Theme attribute set; media query listener for "system" |
| applyCursorStyle | Notify renderers of cursor style change | Valid cursor style string | Renderers updated |
| applyCursorBlink | Notify renderers of cursor blink change | Valid boolean | Renderers updated |
| RendererSettings | Extend with new setting keys | None | Interface includes all renderer-relevant keys |

**Processing Flow**:
```
applySettings(settings)
    +-- applyFontSize(settings.font_size)         [existing]
    +-- applyFontFamily(settings.font_family)      [new]
    +-- applyLineHeight(settings.line_height)      [new]
    +-- applyUiTheme(settings.ui_theme)            [new]
    +-- applyCursorStyle(settings.cursor_style)    [new]
    +-- applyCursorBlink(settings.cursor_blink)    [new]
```

**applyUiTheme behavior**:
```
1. Receive theme value ("light" | "dark" | "system")
   +-- "light" --> set data-theme="light"
   +-- "dark"  --> set data-theme="dark"
   +-- "system" --> check prefers-color-scheme media query
       +-- matches dark --> set data-theme="dark"
       +-- matches light --> set data-theme="light"
       +-- register media query change listener
```

**Key Considerations**:
- applyFontFamily: empty string means system monospace (no CSS variable change needed, browser falls back)
- applyLineHeight: must update the existing line-height calculation (currently derived from font_size; Phase 1 makes it independent)
- applyUiTheme "system": must register a matchMedia listener and clean up previous listener if theme changes

**Testing Approach**:

*Unit Tests*:
- applyFontFamily sets --terminal-font-family CSS variable
- applyLineHeight sets --terminal-line-height CSS variable
- applyUiTheme("light") sets data-theme="light"
- applyUiTheme("dark") sets data-theme="dark"
- applyUiTheme("system") respects prefers-color-scheme
- applyCursorStyle notifies renderers
- applyCursorBlink notifies renderers

**Acceptance Criteria**:
- [ ] All Phase 1 apply functions implemented
- [ ] applySettings calls all individual functions
- [ ] Theme switching works for light/dark/system
- [ ] `bun test` passes

---

### Step 4: Enable Category Tabs and Render Phase 1 Settings UI

**Goal**: All three category tabs are navigable; each category renders its Phase 1 settings items with appropriate UI controls.

**Files to Modify**:
- `src/settings/settings-panel.ts`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| categories array | Enable all three categories | None | terminal and keybinds categories have `enabled: true` |
| renderContent switch | Route to category-specific render methods | Active category selected | Correct content panel rendered |
| renderAppearanceSection | Render Font subsection with font_size, font_family, line_height; Theme subsection with ui_theme | Settings loaded | All Phase 1 Appearance controls displayed |
| renderTerminalSection | Render Cursor subsection with cursor_style, cursor_blink | Settings loaded | All Phase 1 Terminal controls displayed |
| renderKeybindsSection | Render Basic, Display, Settings subsections with 9 keybinds | Settings loaded | All Phase 1 Keybind controls displayed |
| Subsection header | Render h3 elements for subsections within categories | None | Visual grouping of related settings |

**UI Controls to Create**:

| Control | Used By | Behavior |
|---------|---------|----------|
| Text input | font_family | Save on blur/Enter; no real-time preview needed for text |
| Number input | line_height | Real-time preview on input; save on blur/Enter; clamp to range |
| Select dropdown | ui_theme, cursor_style | Save and apply on change event |
| Toggle switch | cursor_blink | Save and apply on click; ARIA role="switch" |
| Keybind capture | All 9 keybinds | On click enter capture mode; next keydown records combination; save immediately |

**Processing Flow for Settings Row Creation**:
```
For each setting item:
1. Create settings-row container
2. Create label element with for attribute
3. Create appropriate input element based on control type
   +-- text input: input[type=text] with placeholder
   +-- number input: input[type=number] with min/max/step + unit span
   +-- select: select element with option elements
   +-- toggle: button[role=switch] with track/thumb spans
   +-- keybind: button with current value text
4. Create hint span (if applicable)
5. Attach event listeners for save and apply behavior
```

**Keybind Capture Flow**:
```
1. User clicks keybind button
   +-- Button enters "capture" state (visual indicator)
   +-- Focus is on the button
2. User presses key combination
   +-- keydown handler captures: modifiers + key
   +-- Format as "Modifier+Key" string
   +-- Update button text
   +-- Save setting
   +-- Exit capture state
3. User presses Escape
   +-- Cancel capture
   +-- Restore original value
   +-- Exit capture state
```

**Event Listener Management**:
- Extend the existing `eventListeners` array pattern for cleanup
- On category switch, detach content listeners and reattach for new content
- Each control type has its own event attachment logic

**Key Considerations**:
- Reuse the existing font_size number input pattern for line_height
- Select dropdown changes trigger save immediately (no blur needed)
- Toggle switches are accessible buttons with role="switch" and aria-checked
- Keybind capture must prevent default browser shortcuts during capture

**Acceptance Criteria**:
- [ ] All three category tabs are enabled and clickable
- [ ] Appearance shows: Font Size (existing), Font Family (new), Line Height (new), UI Theme (new)
- [ ] Terminal shows: Cursor Style, Cursor Blink
- [ ] Keybinds shows: 9 keybinds in 3 subsections (Basic, Display, Settings)
- [ ] Category switching preserves settings state
- [ ] All controls save and apply correctly

---

### Step 5: Add CSS Styles for New UI Controls

**Goal**: All new UI controls styled consistently with MD3 design tokens.

**Files to Modify**:
- `src/styles/settings-panel.css`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| .settings-text-input | Style text input fields (MD3 outlined text field) | None | Consistent with existing number input style |
| .settings-select | Style select dropdowns (MD3 outlined style) | None | Custom appearance with proper focus/hover states |
| .settings-toggle / .settings-toggle-track / .settings-toggle-thumb | Style toggle switch (MD3 switch) | None | Animated toggle with proper on/off colors |
| .settings-keybind-input | Style keybind capture button | None | Distinct button style with capture-mode indicator |
| .settings-subsection-header | Style h3 subsection headers | None | Visual hierarchy within categories |
| .settings-row-toggle | Horizontal layout variant for toggle rows | None | Label and toggle side-by-side |
| .settings-row-keybind | Layout variant for keybind rows | None | Label and keybind button side-by-side |

**Key Considerations**:
- Text input style mirrors the existing number input (MD3 outlined text field)
- Toggle switch uses MD3 color tokens: primary for "on", outline for "off"
- Keybind capture button has a distinct "recording" state with primary-container background
- All controls must have focus-visible styles for keyboard accessibility
- Subsection headers use MD3 Title Medium typography

**Acceptance Criteria**:
- [ ] Text input styled consistently with number input
- [ ] Select dropdown has proper MD3 styling
- [ ] Toggle switch animates between on/off states
- [ ] Keybind button shows visual capture-mode indicator
- [ ] Subsection headers visually separate groups
- [ ] All controls have focus-visible indicators

---

## Dependencies Between Steps

```
Step 1 (Rust types) ──────────> Step 3 (Applier) ──> Step 4 (UI)
                                                        ^
Step 2 (TS types) ─────────────────────────────────────-+
                                                        ^
Step 5 (CSS) ──────────────────────────────────────────-+
```

- Steps 1 and 2 can proceed in parallel
- Step 3 depends on Step 2 (TypeScript types)
- Step 4 depends on Steps 2 and 3
- Step 5 can proceed in parallel with Steps 1-3 (CSS is independent)
- Step 4 requires Step 5 to be complete for visual correctness

## Complete File Changes

**Files to Modify**:
- `src-tauri/src/commands/config.rs` - Extended struct, validation, defaults, tests
- `src/settings/types.ts` - Extended interface, type aliases, constants
- `src/settings/settings-applier.ts` - New apply functions, extended RendererSettings
- `src/settings/settings-panel.ts` - Category enabling, new sections, new controls
- `src/styles/settings-panel.css` - New control styles
- `src/settings/index.ts` - Updated exports

## Testing Strategy

### Unit Tests (Rust)

| Test | Description |
|------|-------------|
| Default values | `AppSettings::default()` returns correct defaults for all fields |
| Empty JSON | Deserialization of `{}` produces all defaults |
| Old format | Deserialization of `{"font_size": 13}` produces defaults for new fields |
| Unknown fields | Deserialization ignores unknown fields |
| Validation rejects | `save_settings` rejects out-of-range and invalid enum values |
| Validation accepts | `save_settings` accepts valid complete settings |
| KeybindSettings defaults | `KeybindSettings::default()` returns correct defaults |
| Round-trip | Serialize then deserialize preserves all fields |

### Unit Tests (TypeScript)

| Test | Description |
|------|-------------|
| applyFontFamily | Sets --terminal-font-family CSS variable |
| applyLineHeight | Sets --terminal-line-height CSS variable |
| applyUiTheme light | Sets data-theme="light" |
| applyUiTheme dark | Sets data-theme="dark" |
| applyUiTheme system | Respects prefers-color-scheme |
| applyCursorStyle | Notifies renderers |
| applyCursorBlink | Notifies renderers |

### Manual Testing

- [ ] Open settings panel -- all three category tabs visible and clickable
- [ ] Appearance: change font_family -- terminal font updates
- [ ] Appearance: change line_height -- terminal line spacing updates
- [ ] Appearance: switch UI theme to Light -- MD3 colors change
- [ ] Appearance: switch UI theme to Dark -- MD3 colors change
- [ ] Appearance: switch UI theme to System -- follows OS preference
- [ ] Terminal: change cursor style -- cursor appearance updates
- [ ] Terminal: toggle cursor blink -- cursor blink behavior changes
- [ ] Keybinds: click keybind button -- enters capture mode
- [ ] Keybinds: press key combination -- keybind recorded and saved
- [ ] Keybinds: press Escape during capture -- capture cancelled
- [ ] Restart app -- all settings preserved
- [ ] Delete settings.json -- app starts with defaults
- [ ] Old settings file `{"font_size": 16}` -- loads correctly, new fields use defaults

## Estimated Effort

Medium (3-5 days)

## Risks and Mitigation

- **Risk**: Line height independence from font_size breaks existing rendering
  - **Mitigation**: The current `applyFontSize` calculates line height as `fontSize + 2`. Phase 1 changes this to use the explicit `line_height` setting. Ensure the default line_height (1.2) produces visually similar results.

- **Risk**: UI Theme "system" media query listener leaks on theme change
  - **Mitigation**: Store listener reference; remove previous listener before registering new one.

- **Risk**: Keybind capture interferes with browser/Tauri shortcuts
  - **Mitigation**: Use `preventDefault()` and `stopPropagation()` during capture mode. Only capture when the keybind button is in capture state.
