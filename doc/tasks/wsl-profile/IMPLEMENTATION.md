# Implementation Plan: WSL Profile Support

## Overview

Add WSL distribution detection, import, and profile integration to eMterm on Windows. A new WSL section in settings shows detected distributions with import capability. The profile editor gains a WSL tab for creating WSL-based profiles that launch `wsl.exe -d <distro>`.

## Objectives

- Detect installed WSL distributions via `wsl.exe --list --quiet`
- Import distributions into `wsl_distributions` in settings
- Add WSL tab to profile editor (Shell | SSH | WSL on Windows)
- Launch WSL sessions via PTY
- Hide all WSL UI on Linux

## Prerequisites

### Development Environment
- Rust toolchain (for backend changes)
- Bun (for frontend changes)
- Docker (for testing)

### Dependencies
- No new external dependencies required
- Existing `PtySession`, settings system, and profile editor are reused

## Architecture Overview

### Technology Stack
- **Backend**: Rust (Tauri commands, WSL detection)
- **Frontend**: TypeScript (settings section, profile editor tab)
- **Testing**: Rust unit tests, Bun tests, Docker E2E

### Design Approach

Follow the SSH connection pattern exactly:
1. Backend Tauri command detects WSL distributions (like `detect_ssh_command` + `load_ssh_config_hosts`)
2. Settings stores imported distributions (like `ssh_connections`)
3. Profile references distribution by name (like `ssh_connection_name`)
4. Frontend settings section mirrors SSH section layout (detected list + imported list)
5. Profile editor adds WSL tab alongside Shell and SSH tabs

### Component Interaction

```
Settings WSL Section → detect_wsl_distributions (Tauri cmd) → wsl.exe --list --quiet
  ↓ Import
wsl_distributions (settings.json)
  ↓ Referenced by
Profile (wsl_distro_name field)
  ↓ Launch
wsl.exe -d <distro> → PtySession → New Tab
```

## Implementation Phases

### Phase 1: Backend - WSL Detection and Settings Schema

**Goal**: Add WSL distribution detection command and extend settings with WSL fields. All Rust changes complete.

**Files to Create**:
- `src-tauri/src/wsl/mod.rs` - WSL module declarations
- `src-tauri/src/wsl/detect.rs` - WSL distribution detection logic (Windows only)
- `src-tauri/src/commands/wsl.rs` - Tauri command for WSL detection

**Files to Modify**:
- `src-tauri/src/commands/mod.rs` - Register wsl command module
- `src-tauri/src/commands/config/settings.rs` - Add `WslDistribution` struct, `wsl_distributions` field to `AppSettings`, `wsl_distro_name` field to `Profile`
- `src-tauri/src/lib.rs` - Register `detect_wsl_distributions` and `get_platform` commands
- `src-tauri/src/main.rs` - Register commands (if separate from lib.rs)

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `wsl::detect::list_distributions` | Execute `wsl.exe --list --quiet`, parse output | Windows OS | Returns list of distro names (may be empty) |
| `commands::wsl::detect_wsl_distributions` | Tauri command wrapping detection | GUI feature enabled | Returns distro names to frontend |
| `WslDistribution` | Settings struct for imported distros | Valid settings | Serializes to/from JSON with null safety |
| `get_platform` | Return platform identifier | None | Returns "windows" or "linux" |

**Processing Flow**:
1. Frontend invokes `detect_wsl_distributions`
2. Backend executes `wsl.exe --list --quiet`
   - Windows → parse stdout (handle UTF-16LE encoding, BOM, empty lines)
   - Linux → command not registered (gated by `#[cfg(windows)]`)
3. Return filtered list of distribution names

**Implementation Steps**:
1. **WslDistribution struct and settings fields** - Add struct with `name` field, add `wsl_distributions` to AppSettings, add `wsl_distro_name` to Profile. Follow `SshConnection` deserialization pattern.
2. **WSL detection module** - Create `wsl/detect.rs` with `list_distributions()` function. Execute `wsl.exe --list --quiet`, handle UTF-16LE output encoding, filter empty lines and BOM. Gate with `#[cfg(windows)]`.
3. **Tauri commands** - Create `commands/wsl.rs` with `detect_wsl_distributions` command. Add `get_platform` command if not already available. Register in module and command handler.
4. **Unit tests** - Test output parsing with various formats (UTF-16LE, BOM, empty lines, whitespace). Test empty/error cases.

**Dependencies**: None (first phase)

**Testing Approach**:
- Unit: Parse WSL output with various encodings and edge cases
- Unit: WslDistribution serialization/deserialization round-trip
- Unit: Profile with wsl_distro_name persists correctly

**Acceptance Criteria**:
- [ ] `WslDistribution` struct added to settings with null-safe deserialization
- [ ] `wsl_distro_name` field added to Profile
- [ ] `detect_wsl_distributions` command returns distro names on Windows
- [ ] `get_platform` command returns platform identifier
- [ ] Existing settings.json loads correctly with new fields defaulting
- [ ] All existing tests pass

**Estimated Effort**: medium

---

### Phase 2: Frontend - WSL Settings Section

**Goal**: Add WSL category to settings panel with detected and imported distribution lists. Windows only.

**Files to Create**:
- `src/settings/sections/wsl-section.ts` - WSL settings section renderer

**Files to Modify**:
- `src/settings/types.ts` - Add `WslDistribution` interface, update `AppSettings` and `Profile`
- `src/settings/settings-panel.ts` - Add WSL category (conditional on Windows platform)
- `src/i18n/locales/en.json` - Add WSL-related translation keys
- `src/i18n/locales/ja.json` - Add WSL-related translation keys

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| `renderWslSection` | Render WSL settings with detected + imported lists | Platform is Windows | WSL section visible with functional Import/Delete |
| WSL category entry | Conditionally add WSL to settings nav | Platform detected | Category visible on Windows, hidden on Linux |

**Processing Flow**:
1. Settings panel initializes → check platform via `get_platform` command
   - Windows → add WSL category to navigation
   - Linux → skip WSL category
2. WSL section renders:
   a. Call `detect_wsl_distributions` → display detected list
   b. Load `wsl_distributions` from settings → display imported list
3. Import button clicked → add to `wsl_distributions`, save settings, refresh UI
4. Delete button clicked → remove from `wsl_distributions`, save settings, refresh UI

**Implementation Steps**:
1. **TypeScript types** - Add `WslDistribution` interface, extend `AppSettings` with `wsl_distributions`, extend `Profile` with `wsl_distro_name`
2. **WSL section renderer** - Create `wsl-section.ts` following `ssh-section.ts` pattern. Two subsections: detected distros (read-only + Import) and imported distros (+ Delete). Disable Import for already-imported distros.
3. **Settings panel integration** - Add WSL category to categories array, conditionally enabled based on platform detection result. Place after SSH category.
4. **i18n labels** - Add translation keys for WSL section title, subsection headers, buttons, empty states

**Dependencies**: Phase 1 (Tauri commands and types must exist)

**Testing Approach**:
- Unit: WslDistribution type matches Rust struct
- Manual: WSL section visible on Windows, hidden on Linux
- Manual: Import/Delete workflow functions correctly

**Acceptance Criteria**:
- [ ] WSL category appears in settings navigation on Windows only
- [ ] Detected distributions listed with Import buttons
- [ ] Already-imported distributions have disabled Import buttons
- [ ] Imported distributions listed with Delete buttons
- [ ] Import adds to settings and refreshes UI
- [ ] Delete removes from settings and refreshes UI
- [ ] i18n works for English and Japanese

**Estimated Effort**: medium

---

### Phase 3: Frontend - Profile Editor WSL Tab and Session Launch

**Goal**: Add WSL tab to profile editor and implement WSL session launch logic.

**Files to Modify**:
- `src/profile/profile-editor.ts` - Add WSL tab (Windows only), dropdown for imported distributions
- `src/tab-bar/tab-bar-ui.ts` - Add WSL profile launch logic (parallel to `launchSshProfile`)
- `src/i18n/locales/en.json` - Add WSL tab label
- `src/i18n/locales/ja.json` - Add WSL tab label

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| WSL tab in profile editor | Show/select imported WSL distributions | Platform is Windows | Profile created with `wsl_distro_name` set |
| `launchWslProfile` | Build wsl.exe args and create PTY tab | Profile has `wsl_distro_name` | New tab running WSL session |

**Processing Flow**:

Profile Editor:
1. Modal opens → check platform
   - Windows → render Shell | SSH | WSL tabs
   - Linux → render Shell | SSH tabs (existing)
2. WSL tab selected → show dropdown of imported distributions
3. Save → set `wsl_distro_name`, clear shell and SSH fields

Session Launch:
1. Profile selected with `wsl_distro_name` set
2. Validate distribution exists in `wsl_distributions`
3. Build spawn options: shell_path = "wsl.exe", shell_args = ["-d", distro_name]
4. Create tab via existing tab creation flow

**Implementation Steps**:
1. **Profile editor WSL tab** - Add third tab button (Windows only). Follow SSH tab pattern. Show dropdown populated from `wsl_distributions`. Mutually exclusive with Shell/SSH: switching clears other fields.
2. **WSL session launch** - Add `launchWslProfile` function in tab-bar-ui.ts. Pattern mirrors `launchSshProfile`: validate distro exists → build spawn options → create tab. No Tauri command needed (args are trivial).
3. **Tab auto-selection** - When editing profile with `wsl_distro_name`, auto-select WSL tab and pre-select distro in dropdown. Disable WSL tab when no distributions imported.
4. **i18n labels** - Add translation key for WSL tab label

**Dependencies**: Phase 1 (settings schema), Phase 2 (WSL section for importing distros)

**Testing Approach**:
- Manual: Shell | SSH | WSL tabs visible on Windows
- Manual: WSL tab disabled when no distributions imported
- Manual: Save from WSL tab creates profile with wsl_distro_name
- Manual: WSL profile launches wsl.exe session in new tab

**Acceptance Criteria**:
- [ ] Profile editor shows 3 tabs on Windows, 2 on Linux
- [ ] WSL tab shows dropdown of imported distributions
- [ ] WSL tab disabled when no distributions imported
- [ ] Tab switching clears other mode fields (mutual exclusivity)
- [ ] WSL profile launches `wsl.exe -d <distro>` in new tab
- [ ] Error shown if referenced distribution removed from imports
- [ ] TypeScript typecheck passes
- [ ] All existing tests pass

**Estimated Effort**: medium

---

## Complete File Structure

```
src-tauri/src/
├── wsl/
│   ├── mod.rs                    # NEW: Module declarations
│   └── detect.rs                 # NEW: WSL distribution detection (Windows only)
├── commands/
│   ├── wsl.rs                    # NEW: detect_wsl_distributions command
│   └── mod.rs                    # MODIFIED: Register wsl module
├── commands/config/
│   └── settings.rs               # MODIFIED: WslDistribution, wsl_distributions, wsl_distro_name
├── lib.rs                        # MODIFIED: Register new Tauri commands

src/
├── settings/
│   ├── types.ts                  # MODIFIED: WslDistribution, AppSettings, Profile
│   ├── settings-panel.ts         # MODIFIED: Add WSL category (Windows conditional)
│   └── sections/
│       └── wsl-section.ts        # NEW: WSL settings section renderer
├── profile/
│   └── profile-editor.ts         # MODIFIED: Add WSL tab (Windows conditional)
├── tab-bar/
│   └── tab-bar-ui.ts             # MODIFIED: Add launchWslProfile
└── i18n/locales/
    ├── en.json                   # MODIFIED: WSL labels
    └── ja.json                   # MODIFIED: WSL labels
```

## Testing Strategy

- **Unit tests**: WSL output parsing (UTF-16LE, BOM, edge cases), serialization round-trip. Target 80%+ coverage for new Rust code.
- **Integration tests**: Settings persistence with WSL fields
- **E2E (Docker)**: Regression only (WSL not available in Docker Linux)
- **Manual**: UI verification on Windows (WSL section, profile editor tabs, session launch)

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none)  | -       | No new dependencies required |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| UTF-16LE encoding from `wsl --list` | High | Medium | Explicit encoding handling in parser |
| WSL not available in CI/Docker | Medium | Low | Unit tests use mock output; E2E covers regression only |
| Platform detection timing | Low | Low | Cache platform result on app init |

## Open Questions

- None. All requirements clarified during specification.

## Success Metrics

- [ ] WSL distributions detected and displayed on Windows
- [ ] Import/delete workflow functional
- [ ] Profile editor Shell | SSH | WSL tabs on Windows
- [ ] WSL session launches correctly
- [ ] Linux shows no WSL UI
- [ ] All existing tests pass
- [ ] TypeScript typecheck passes
