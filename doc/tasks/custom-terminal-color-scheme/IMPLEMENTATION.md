# Implementation Plan: Custom Terminal Color Scheme

## Overview

Add user-customizable terminal color schemes to eMterm. Users can edit preset color palettes inline in the settings panel, with automatic copy-on-edit for presets, real-time terminal preview, and persistent storage in settings.json.

## Objectives

- Extend the data model (Rust + TypeScript) to store user-defined color schemes
- Build an inline color palette editor with native color pickers and HEX input
- Implement auto-copy on preset edit, duplicate, delete, and rename operations
- Integrate real-time preview via existing CSS variable and renderer notification pipeline

## Prerequisites

### Development Environment
- Rust toolchain (for Tauri backend changes)
- Bun (package manager and test runner)
- Existing eMterm development setup (`bun install`)

### Dependencies
- No new external dependencies required
- All functionality uses existing browser APIs (`input type="color"`) and project infrastructure

### Knowledge Requirements
- Existing settings panel architecture (settings-components.ts, settings-sections.ts pattern)
- Tauri invoke command pattern (SettingsService, config.rs)
- CSS variable-based theming pipeline (settings-applier.ts → CSS vars → canvas renderer)

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend), Rust (backend)
- **Framework**: Tauri v2, Vanilla DOM
- **Key Libraries**: None new — uses existing project infrastructure

### Design Approach
The feature follows the existing settings panel pattern: a new module (`color-scheme-editor.ts`) renders inline UI below the existing Terminal Color Scheme select, and manages user scheme CRUD operations. The data flows through the same `SettingsService.save()` → `settings.json` path.

### Component Interaction

```
settings-sections.ts (renders select + integrates editor)
  ↓ delegates to
color-scheme-editor.ts (palette UI + scheme management logic)
  ↓ calls
settings-applier.ts (applyTerminalColorScheme — extended for user schemes)
  ↓ sets
CSS variables + notifyRenderers
  ↓ applied by
canvas-renderer.ts (terminal display)
```

## Implementation Phases

### Phase 1: Data Model and Backend

**Goal**: Add `custom_color_schemes` field to AppSettings on both Rust and TypeScript sides, with backward-compatible deserialization and hex color conversion utilities.

**Files to Create**:
- (none)

**Files to Modify**:
- `src-tauri/src/commands/config.rs`:
  - Add `UserColorScheme` struct with name, foreground, background, cursor, selection, ansi_colors fields (all String)
  - Add `custom_color_schemes: Vec<UserColorScheme>` field to `AppSettings` with `serde(default)` + `deserialize_null_default`
  - Add to `Default for AppSettings`: `custom_color_schemes: Vec::new()`
- `src/settings/types.ts`:
  - Add `UserColorScheme` interface matching Rust struct
  - Add `custom_color_schemes` field to `AppSettings` interface
- `src/terminal/colors.ts`:
  - Add `hexToRgb(hex: string) -> Rgb` conversion utility
  - Add `rgbToHex(rgb: Rgb) -> string` conversion utility

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| UserColorScheme (Rust) | Serialize/deserialize user color data | Valid JSON | Struct with all color fields populated |
| UserColorScheme (TS) | TypeScript representation of user color data | — | Interface matches Rust struct exactly |
| hexToRgb | Convert `#RRGGBB` string to Rgb struct | Valid hex string | Rgb with correct r, g, b values |
| rgbToHex | Convert Rgb struct to `#RRGGBB` string | Valid Rgb | Lowercase hex string with `#` prefix |

**Processing Flow**:
```
1. settings.json loaded by Rust backend
   ├─ custom_color_schemes field exists → deserialize Vec<UserColorScheme>
   └─ custom_color_schemes field missing → default to empty Vec
2. Frontend receives AppSettings via Tauri invoke
3. Hex conversion utilities available for palette editor to bridge Rgb ↔ HEX
```

**Implementation Steps**:

1. **Add Rust UserColorScheme struct and AppSettings field**
   - Define struct with serde derives matching existing patterns
   - Key consideration: Use same `deserialize_null_default` pattern as other fields

2. **Add TypeScript types**
   - Mirror Rust struct as TypeScript interface
   - Add field to AppSettings interface

3. **Add hex color conversion utilities**
   - `hexToRgb`: parse `#RRGGBB` → `{ r, g, b }`
   - `rgbToHex`: format `{ r, g, b }` → `#rrggbb`
   - Key consideration: Handle edge cases (invalid input, case-insensitive parsing)

**Dependencies**:
- Requires: Nothing (foundation phase)
- Blocks: All subsequent phases

**Testing Approach**:

*Unit Tests (TypeScript)*:
- hexToRgb parses valid hex strings correctly
- hexToRgb handles lowercase and uppercase
- rgbToHex formats Rgb to lowercase `#rrggbb`
- Round-trip: rgbToHex(hexToRgb(hex)) === hex

*Unit Tests (Rust)*:
- AppSettings deserializes with missing `custom_color_schemes` → empty vec
- AppSettings deserializes with null `custom_color_schemes` → empty vec
- UserColorScheme round-trip serialization
- AppSettings with custom_color_schemes round-trip

**Acceptance Criteria**:
- [ ] Rust AppSettings has `custom_color_schemes: Vec<UserColorScheme>` field
- [ ] Existing settings.json without the field loads without error
- [ ] TypeScript `AppSettings` interface includes `custom_color_schemes`
- [ ] `hexToRgb` and `rgbToHex` pass all unit tests
- [ ] All existing tests still pass

**Estimated Effort**: 小

---

### Phase 2: Color Scheme Manager Logic

**Goal**: Implement the CRUD operations for user color schemes as a standalone module with pure logic (no DOM), enabling auto-copy, duplicate, delete, and rename.

**Files to Create**:
- `src/settings/color-scheme-editor.ts`: Scheme management functions (CRUD logic portion)
- `src/settings/color-scheme-editor.test.ts`: Tests for scheme management

**Files to Modify**:
- (none)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| generateCopyName | Generate unique `{base}_copy_N` name | Base name + list of existing names | Unique name with incrementing N |
| createUserSchemeFromPreset | Clone a preset to a new user scheme | Valid preset name | New UserColorScheme with auto-generated name |
| updateUserSchemeColor | Update a specific color in a user scheme | Existing user scheme + color key + new value | Scheme updated in place |
| deleteUserScheme | Remove a user scheme by name | Existing scheme name | Scheme removed from array |
| duplicateScheme | Copy any scheme (preset or user) as new user scheme | Source scheme name | New UserColorScheme added |
| renameUserScheme | Change a user scheme's name | Old name + new name (non-empty, unique) | Name updated |
| isUserScheme | Determine if a scheme name belongs to a user scheme | Scheme name + user schemes array | Boolean result |
| buildSelectOptions | Build option list for the select box | Presets + user schemes | Ordered array with `[User]` suffix on user items |
| validateHexColor | Check if a string is valid `#RRGGBB` format | Input string | Boolean result |

**Processing Flow**:
```
1. Auto-copy on preset edit
   ├─ Detect that current selection is a preset
   ├─ Generate copy name from preset name + existing user scheme names
   ├─ Clone preset colors to new UserColorScheme
   ├─ Apply the edited color to the clone
   └─ Return new scheme (caller switches select + saves)

2. Duplicate operation
   ├─ Resolve source scheme (preset lookup or user scheme lookup)
   ├─ Generate copy name
   ├─ Clone all colors to new UserColorScheme
   └─ Return new scheme

3. Delete operation
   ├─ Verify scheme is a user scheme (not preset)
   ├─ Remove from custom_color_schemes array
   └─ Signal caller to revert to "emterm"

4. Rename operation
   ├─ Validate new name (non-empty, unique among all scheme names)
   ├─ Update name field in user scheme
   └─ If scheme is currently active, update terminal_color_scheme reference
```

**Implementation Steps**:

1. **Implement naming utilities**
   - `generateCopyName` with incrementing `_copy_N` logic
   - `validateHexColor` for input validation
   - `isUserScheme` helper

2. **Implement CRUD operations**
   - Each operates on an `AppSettings` object (or its `custom_color_schemes` array)
   - Pure functions where possible, returning updated data

3. **Implement select box option builder**
   - Presets first (fixed order), user schemes second (creation/array order)
   - User scheme labels formatted as `{name} [User]`

**Dependencies**:
- Requires: Phase 1 (types and hex utilities)
- Blocks: Phase 3 (UI needs these operations)

**Testing Approach**:

*Unit Tests*:

| Scenario | Expected Result |
|----------|-----------------|
| generateCopyName with no existing copies | `{name}_copy_1` |
| generateCopyName with _copy_1 existing | `{name}_copy_2` |
| generateCopyName with _copy_1 and _copy_3 existing | `{name}_copy_2` (fills gap) |
| createUserSchemeFromPreset("dracula") | UserColorScheme with dracula colors and auto name |
| deleteUserScheme removes scheme from array | Array length decremented |
| duplicateScheme from preset | New user scheme with preset colors |
| duplicateScheme from user scheme | New user scheme with user colors |
| renameUserScheme to valid name | Name updated |
| renameUserScheme to empty string | Rejected |
| renameUserScheme to existing name | Rejected |
| buildSelectOptions orders presets first | Presets before user schemes |
| buildSelectOptions adds [User] suffix | User scheme labels end with " [User]" |
| validateHexColor accepts valid hex | Returns true for "#aabbcc" |
| validateHexColor rejects invalid | Returns false for "red", "#xyz" |

**Acceptance Criteria**:
- [ ] All CRUD functions pass unit tests
- [ ] Auto-copy naming follows `_copy_N` increment rule
- [ ] Select option builder produces correct order and format
- [ ] Rename validation prevents empty and duplicate names
- [ ] HEX validation correctly accepts/rejects inputs

**Estimated Effort**: 小

---

### Phase 3: Settings Applier Extension

**Goal**: Extend `applyTerminalColorScheme()` to support user-defined color schemes by looking up user schemes and applying their colors as CSS variables.

**Files to Modify**:
- `src/settings/settings-applier.ts`:
  - Extend `applyTerminalColorScheme` to accept user schemes data
  - When scheme name matches a user scheme, apply all 20 colors as CSS variables
  - Fall back to existing preset lookup when not a user scheme

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| applyTerminalColorScheme (extended) | Apply color scheme from either presets or user schemes | Scheme name + optional user schemes array | CSS variables set + renderers notified |
| applyUserColorScheme (internal) | Set all 20 CSS vars from a UserColorScheme | Valid UserColorScheme | All terminal color CSS variables populated |

**Processing Flow**:
```
1. applyTerminalColorScheme(schemeName, userSchemes?) called
   ├─ Empty/default/emterm → remove all custom vars (existing behavior)
   ├─ Match in userSchemes array → apply user scheme colors as CSS vars
   └─ No match in userSchemes → look up preset (existing behavior)
2. For user scheme application
   ├─ Set --terminal-foreground, --terminal-background, --terminal-cursor-color, --terminal-selection-bg
   ├─ Set --terminal-color-0 through --terminal-color-15
   ├─ Set data-terminal-color-scheme attribute
   └─ Notify renderers with scheme name
```

**Implementation Steps**:

1. **Extend applyTerminalColorScheme signature**
   - Add optional parameter for user schemes lookup
   - Key consideration: Maintain backward compatibility with existing callers

2. **Implement user scheme color application**
   - Map UserColorScheme HEX strings to CSS variables
   - Use same CSS variable names as existing preset application

3. **Update applySettings to pass user schemes**
   - Pass `settings.custom_color_schemes` to `applyTerminalColorScheme`

**Dependencies**:
- Requires: Phase 1 (types), Phase 2 (isUserScheme helper)
- Blocks: Phase 4 (editor needs real-time preview)

**Testing Approach**:

*Unit Tests*:

| Scenario | Expected Result |
|----------|-----------------|
| Apply user scheme sets all 20 CSS variables | Variables match scheme values |
| Apply user scheme sets data attribute | `data-terminal-color-scheme` set to name |
| Apply user scheme notifies renderers | colorScheme notification sent |
| Apply unknown name falls back to preset lookup | Existing behavior preserved |
| Apply emterm/default still clears CSS vars | Existing behavior preserved |
| Passing empty userSchemes array works | Falls back to preset lookup |

**Acceptance Criteria**:
- [ ] User scheme colors applied as CSS variables when selected
- [ ] Existing preset behavior unchanged
- [ ] All existing applier tests still pass
- [ ] Real-time color updates work through the pipeline

**Estimated Effort**: 小

---

### Phase 4: Color Palette Editor UI

**Goal**: Build the inline color palette editor component that renders below the Terminal Color Scheme select box, enabling visual color editing with native color pickers and HEX text input.

**Files to Modify**:
- `src/settings/color-scheme-editor.ts`:
  - Add palette editor rendering functions (DOM creation)
  - Add action buttons (Duplicate, Delete) rendering
  - Add rename field rendering
- `src/settings/settings-sections.ts`:
  - Replace static `renderSelect` for terminal-color-scheme with new integrated component
  - Pass SectionContext to color scheme editor for save/apply operations
- `src/i18n/locales/en.json`:
  - Add labels for color editor UI elements
- `src/i18n/locales/ja.json`:
  - Add Japanese labels for color editor UI elements
- `src-tauri/locales/en.json`:
  - Add any backend-side labels if needed
- `src-tauri/locales/ja.json`:
  - Add any backend-side labels if needed

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| renderColorSchemeEditor | Render the full color scheme editor (select + palette + buttons) | Parent element + SectionContext | Editor DOM appended with all controls |
| renderColorPalette | Render the 20-color palette grid | Scheme data + change callback | Color pickers + HEX inputs rendered |
| renderColorInput | Render a single color picker + HEX text pair | Color value + label + change handler | Two synchronized inputs |
| renderActionButtons | Render Duplicate/Delete buttons (context-dependent) | isUserScheme flag | Buttons with correct visibility |
| renderRenameField | Render inline rename text field | Current name + rename callback | Editable field (user schemes only) |

**Processing Flow**:
```
1. Select box change
   ├─ Load selected scheme data (preset or user)
   ├─ Re-render palette with new colors
   ├─ Update action button visibility
   └─ Apply color scheme for preview

2. Color input change (any of the 20 colors)
   ├─ If currently on a preset → trigger auto-copy
   │   ├─ Create user scheme copy
   │   ├─ Apply edited color to copy
   │   ├─ Add to custom_color_schemes
   │   ├─ Update select box to new scheme
   │   └─ Save settings
   ├─ If currently on a user scheme → update in place
   │   ├─ Update color in scheme data
   │   └─ Save settings
   └─ Apply color change for real-time preview

3. Action button click
   ├─ Duplicate → create copy, update select, save
   ├─ Delete → remove scheme, revert to emterm, save
   └─ Rename → validate, update name, update select, save
```

**Implementation Steps**:

1. **Build color input component**
   - Paired `input type="color"` + `input type="text"` (HEX)
   - Bidirectional sync: picker change → update text, text change → update picker
   - Key consideration: Validate HEX input before applying

2. **Build color palette layout**
   - 4 special colors (foreground, background, cursor, selection) as labeled rows
   - 8 standard colors (0-7) in a grid row
   - 8 bright colors (8-15) in a grid row
   - Key consideration: Grid layout should be compact within the settings panel width

3. **Build action buttons and rename field**
   - Duplicate: always visible, creates copy of current scheme
   - Delete: visible only for user schemes
   - Rename: inline text field, visible only for user schemes
   - Key consideration: After delete, auto-select "emterm" and re-render palette

4. **Integrate into settings-sections.ts**
   - Replace the existing `renderSelect` call for terminal-color-scheme
   - Call `renderColorSchemeEditor` which internally handles select + palette + buttons
   - Wire up save/apply callbacks through SectionContext

5. **Add i18n labels**
   - English and Japanese labels for all new UI elements
   - Color names (Foreground, Background, Standard Colors, Bright Colors, etc.)

**Dependencies**:
- Requires: Phase 1 (types), Phase 2 (CRUD logic), Phase 3 (applier extension)
- Blocks: Nothing (final phase)

**Testing Approach**:

*Unit Tests*:
- Already covered by Phase 2 tests (logic layer)
- Additional: color input sync behavior (if extractable as pure function)

*Manual Testing*:
- Visual verification of palette layout in settings panel
- Color picker interaction and HEX input sync
- Auto-copy flow: select preset → edit color → verify new user scheme created
- Delete flow: verify revert to emterm
- Rename flow: verify select box label updates

**Acceptance Criteria**:
- [ ] Color palette renders inline below Terminal Color Scheme select
- [ ] Color pickers and HEX inputs are synchronized bidirectionally
- [ ] Editing a preset color triggers auto-copy with `_copy_N` naming
- [ ] Select box shows `{name} [User]` for user schemes
- [ ] Presets appear first, user schemes appear second in select
- [ ] Duplicate, Delete, Rename buttons work correctly
- [ ] Delete reverts to "emterm" and re-renders palette
- [ ] Color changes preview in real-time on the terminal
- [ ] Settings save correctly to settings.json via Tauri command
- [ ] All i18n labels display in English and Japanese

**Estimated Effort**: 中

---

## Complete File Structure

```
src/settings/
├── color-scheme-editor.ts         # NEW: Palette editor UI + scheme CRUD logic
├── color-scheme-editor.test.ts    # NEW: Tests for CRUD logic and utilities
├── settings-sections.ts           # MODIFIED: Integrate color editor
├── settings-applier.ts            # MODIFIED: Support user scheme application
├── settings-applier.test.ts       # MODIFIED: Add user scheme tests
├── types.ts                       # MODIFIED: Add UserColorScheme interface
├── settings-components.ts         # UNCHANGED
├── settings-service.ts            # UNCHANGED
├── settings-panel.ts              # UNCHANGED
├── index.ts                       # UNCHANGED (may need re-export)

src/terminal/
├── colors.ts                      # MODIFIED: Add hexToRgb, rgbToHex

src-tauri/src/commands/
├── config.rs                      # MODIFIED: Add UserColorScheme struct + field

src/i18n/locales/
├── en.json                        # MODIFIED: Add color editor labels
├── ja.json                        # MODIFIED: Add color editor labels
```

**File Descriptions**:
- `color-scheme-editor.ts`: Contains both the CRUD logic (generateCopyName, CRUD ops, buildSelectOptions) and the UI rendering functions (palette, buttons, rename). Single file for cohesion since the UI is tightly coupled with the logic.
- `color-scheme-editor.test.ts`: Tests the pure logic functions (naming, CRUD, validation, select options). DOM rendering tested manually.
- `settings-sections.ts`: Modified to delegate terminal color scheme rendering to the new editor module.
- `settings-applier.ts`: Extended to look up user schemes when applying a color scheme.
- `types.ts`: Adds `UserColorScheme` interface and `custom_color_schemes` to `AppSettings`.
- `colors.ts`: Adds `hexToRgb` and `rgbToHex` conversion utilities.
- `config.rs`: Adds `UserColorScheme` Rust struct and field to `AppSettings`.

## Testing Strategy

### Unit Testing

**Approach**:
- Bun test runner for TypeScript
- `cargo test` for Rust
- Table-driven test style where appropriate
- Mock `document` and `window` objects following existing test patterns (see `settings-applier.test.ts`)

**Test Coverage Goals**:
- CRUD logic and utilities: 90%+
- Hex conversion utilities: 100%
- Applier extension: 80%+
- Rust deserialization: 90%+

**Key Test Areas**:
1. **Hex Conversion** (`src/terminal/colors.ts`): Round-trip, edge cases
2. **Scheme CRUD** (`src/settings/color-scheme-editor.ts`): All operations, naming, validation
3. **Applier Extension** (`src/settings/settings-applier.ts`): User scheme application, fallback
4. **Rust Serialization** (`src-tauri/src/commands/config.rs`): Backward compatibility, round-trip

### Integration Testing

**Scenarios**:
1. Full flow: select preset → edit color → auto-copy → save → reload settings → user scheme preserved
2. Delete user scheme → settings saved → reload → scheme gone, reverted to emterm

### Manual Testing

- [ ] Color palette visual layout matches design
- [ ] Native color picker interaction is smooth
- [ ] Real-time terminal preview responds to color changes
- [ ] Settings panel scroll behavior with expanded palette
- [ ] Multiple user schemes can coexist

## Dependencies

### External Dependencies

None new.

### Internal Dependencies

**Implementation Order** (respecting dependencies):
1. Phase 1: Data Model (no dependencies)
2. Phase 2: CRUD Logic (depends on Phase 1 types)
3. Phase 3: Applier Extension (depends on Phase 1 types)
4. Phase 4: UI (depends on all prior phases)

Note: Phases 2 and 3 can be implemented in parallel.

**Component Dependencies**:
- `color-scheme-editor.ts` depends on `types.ts`, `colors.ts`
- `settings-sections.ts` depends on `color-scheme-editor.ts`
- `settings-applier.ts` depends on `types.ts`

## Risk Assessment

### Technical Risks

1. **Color Picker Platform Differences**
   - **Risk**: `input type="color"` renders differently on different OS/WebView versions
   - **Likelihood**: Low (Tauri uses system WebView)
   - **Impact**: Low (cosmetic only)
   - **Mitigation**: Use native picker + HEX text as backup

2. **Settings File Size Growth**
   - **Risk**: Many user schemes could bloat settings.json
   - **Likelihood**: Low (typical users create few schemes)
   - **Impact**: Low (JSON is small per scheme)
   - **Mitigation**: No action needed for now

3. **Real-time Preview Performance**
   - **Risk**: Frequent color picker changes could cause excessive saves
   - **Likelihood**: Medium
   - **Impact**: Low (Tauri file I/O is fast)
   - **Mitigation**: Debounce save operations; apply CSS immediately but batch save

## Security Considerations

- HEX color values validated against `#RRGGBB` regex before saving
- Colors applied via CSS `setProperty` (not innerHTML), preventing injection
- No user-provided values used in DOM `innerHTML` or `eval`

## Open Questions

### Implementation-Specific:
- [ ] Should save be debounced during rapid color picker changes? (Recommend: yes, 300ms debounce on save, immediate CSS apply)

## Success Metrics

### Functional Completeness
- [ ] All functional requirements (FR1–FR11) implemented
- [ ] All user stories (US1, US2) acceptance criteria met
- [ ] All test scenarios pass

### Quality Metrics
- [ ] TypeScript tests coverage ≥ 80% for new code
- [ ] Rust tests coverage ≥ 80% for new code
- [ ] All existing tests still pass
- [ ] No TypeScript type errors (`bun run typecheck`)

### User Experience
- [ ] Color changes reflect in terminal within one frame (< 16ms)
- [ ] Color palette is visually clear and easy to use
- [ ] User scheme management (create, rename, delete) is intuitive

## References

- **Specification**: `doc/tasks/custom-terminal-color-scheme/SPEC.md`
- **Requirements**: `doc/tasks/custom-terminal-color-scheme/要件定義書.md`
- **Existing Applier Tests**: `src/settings/settings-applier.test.ts` (mock patterns)
- **Existing Components**: `src/settings/settings-components.ts` (UI patterns)
- **Rust Config**: `src-tauri/src/commands/config.rs` (serde patterns)
