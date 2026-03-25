# Implementation Plan: Status Bar

## Overview

A configurable status bar at the bottom of the application window with template variable resolution, OSC protocol for external content injection, and custom command execution. Follows existing tab bar implementation patterns.

## Objectives

- Display persistent context information (time, cwd, git branch) below the terminal screen
- Support OSC 777;statusbar protocol for external process content injection
- Provide full appearance customization and user-defined commands via settings

## Prerequisites

### Development Environment
- Rust toolchain (existing)
- Bun (existing)
- Docker for testing (existing)

### Dependencies
- No new external dependencies required
- Uses existing Tauri shell command infrastructure for custom command execution

## Architecture Overview

### Technology Stack
- **Backend**: Rust (settings persistence, validation, custom command execution)
- **Frontend**: TypeScript (status bar UI, template engine, variable providers, OSC controller)
- **Styling**: CSS (following UI design tokens)

### Design Approach

The status bar is a WebView HTML/CSS element positioned below the terminal content area, following the same pattern as the existing tab bar (`src/tab-bar/`). It consists of three layers (OSC, App Line 1, App Line 2) each with left/right sections. Template variables are resolved by independent providers with configurable polling intervals.

### Component Interaction

```
Settings (Rust) ─serialization─> Frontend Settings Cache
                                        │
                                        v
StatusBarUI (TypeScript) ─────> DOM (HTML/CSS)
  ├── StatusBarRenderer         ^
  ├── TemplateEngine ───────────┘
  ├── VariableProviders (Time, Cwd, Git, Cmd)
  └── OscLayerController <── osc-handler.ts <── WASM Parser <── PTY
```

## Implementation Phases

### Phase 1: Core Infrastructure and Settings

**Goal**: Status bar container renders in the UI with enable/disable toggle and basic appearance customization. No variable resolution yet (static template strings displayed).

**Files to Create**:
- `src/status-bar/index.ts` - StatusBarUI main class (initialization, lifecycle, settings application)
- `src/status-bar/renderer.ts` - DOM rendering, layer creation, visibility management
- `src/status-bar/types.ts` - Interfaces and type definitions
- `src/styles/status-bar.css` - Status bar styles following UI design tokens
- `src/settings/sections/status-bar-section.ts` - Settings panel "Status Bar" category

**Files to Modify**:
- `src-tauri/src/commands/config/settings.rs` - Add statusbar_* fields to AppSettings, CustomCommand struct, defaults
- `src-tauri/src/commands/config/validation.rs` - Add statusbar settings validation
- `src-tauri/src/commands/config/types.rs` - Add StatusbarCustomCommand type if needed
- `src/settings/types.ts` - Add statusbar fields to AppSettings interface
- `src/settings/settings-sections.ts` - Export renderStatusBarSection
- `src/settings/settings-panel.ts` - Add "status-bar" category to category list and icon
- `src/settings/settings-applier.ts` - Add applyStatusBar function
- `src/index.html` - Add status-bar container div after tab-content-area
- `src/terminal-app/index.ts` - Instantiate StatusBarUI, wire to settings changes

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| StatusBarUI | Lifecycle management, settings application | Settings loaded | Status bar visible/hidden per settings |
| StatusBarRenderer | DOM element creation, layer visibility | Container element exists | 3-layer structure rendered |
| AppSettings (Rust) | Persist statusbar_* fields with defaults | Valid JSON | All fields have valid values |
| StatusBarSection | Settings UI for enable/toggle and appearance | Settings panel initialized | User can toggle and customize |

**Processing Flow** (diagram-convertible):
1. Application starts -> read settings
   - statusbar_enabled = true -> create StatusBarUI, show container
   - statusbar_enabled = false -> hide container (default)
2. Settings change event -> StatusBarUI.applySettings()
   - Enabled state changed -> show/hide container
   - Appearance changed -> update CSS variables
   - Template changed -> re-render (Phase 2)

**Implementation Steps**:
1. **Rust Settings Fields** - Add all statusbar_* fields to AppSettings with serde defaults, CustomCommand struct, validation rules
2. **TypeScript Settings Interface** - Mirror Rust fields in AppSettings interface, add validation constants
3. **DOM Structure** - Create status bar container with 3 layers (OSC, app line 1, app line 2), each with left/right sections
4. **CSS Styling** - Status bar styles using UI design tokens (colors, typography, spacing), layer visibility rules
5. **Settings Panel Section** - Enable/disable toggle, template string inputs, appearance controls (colors, font size, opacity)
6. **Integration** - Instantiate StatusBarUI in TerminalApp, wire settings applier, add to HTML layout

**Dependencies**: None (first phase)

**Testing Approach**:
- Unit: Renderer layer visibility based on content, settings defaults
- Integration: Settings toggle shows/hides status bar
- E2E (Docker): Status bar hidden by default, appears when enabled

**Acceptance Criteria**:
- [ ] Status bar container renders below terminal when enabled
- [ ] Default OFF (hidden)
- [ ] Settings panel shows Status Bar category with all appearance options
- [ ] Layer structure: OSC (hidden when empty), App Line 1, App Line 2 (hidden when empty)
- [ ] Mux mode: status bar visible regardless of mux state (FR10)

**Estimated Effort**: medium

---

### Phase 2: Template Variables and Providers

**Goal**: Template engine resolves `{time}`, `{cwd}`, `{git_branch}` variables with independent refresh rates and git state coloring.

**Files to Create**:
- `src/status-bar/template-engine.ts` - Template string parsing, variable extraction, resolution
- `src/status-bar/providers/types.ts` - VariableProvider interface definition
- `src/status-bar/providers/time-provider.ts` - Time formatting with configurable format string
- `src/status-bar/providers/cwd-provider.ts` - CWD from OSC 7 events and polling
- `src/status-bar/providers/git-provider.ts` - Git branch name and dirty/clean state

**Files to Modify**:
- `src/status-bar/index.ts` - Integrate template engine and providers, manage polling lifecycle
- `src/status-bar/renderer.ts` - Accept resolved template output, apply git state colors

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| TemplateEngine | Parse template strings, identify variables, resolve with provider values | Template string provided | Resolved string with variable values substituted |
| TimeProvider | Format current time per configurable format string | Format string valid | Formatted time string |
| CwdProvider | Track current working directory via OSC 7 and polling | Terminal session active | Basename of current cwd |
| GitBranchProvider | Detect branch name and dirty/clean state | CWD available | Branch name + state color |

**Processing Flow** (diagram-convertible):
1. Template string contains variables -> TemplateEngine.parse() extracts variable names
2. Each variable registered with a provider and individual refresh rate
3. Timer fires for a variable -> provider.getValue() called
   - Value changed -> TemplateEngine.resolve() -> Renderer.update()
   - Value unchanged -> skip render
4. CWD change event (OSC 7) -> CwdProvider updates immediately (bypass polling)
5. Git state detection -> execute git commands -> parse output
   - Clean repo -> default/green color
   - Dirty repo -> yellow/orange color
   - Not a git repo -> empty string

**Implementation Steps**:
1. **VariableProvider Interface** - Define contract: getValue(), getColor() (optional), dispose()
2. **TemplateEngine** - Parse `{variable_name}` patterns, resolve with registered providers, handle unknown variables (render empty)
3. **TimeProvider** - Clock with configurable format string, own refresh interval
4. **CwdProvider** - Extract basename from path, listen to OSC 7 events on TerminalState, poll as fallback
5. **GitBranchProvider** - Execute git commands asynchronously, parse branch name and porcelain status, map to color
6. **Integration** - Wire providers to TemplateEngine, connect to StatusBarRenderer with differential updates

**Dependencies**: Requires Phase 1

**Testing Approach**:
- Unit: Template parsing, variable resolution, unknown variable handling, time formatting, basename extraction, git output parsing, dirty/clean state detection
- Integration: Variable updates trigger re-render, per-variable refresh rates
- E2E (Docker): Default display shows time and cwd

**Acceptance Criteria**:
- [ ] `{time}` displays formatted current time (FR3)
- [ ] `{cwd}` displays basename of working directory (FR4)
- [ ] `{git_branch}` displays branch with state color (FR5)
- [ ] Each variable has individual configurable refresh rate (FR2)
- [ ] Unknown variables render as empty string
- [ ] Differential rendering: only changed content triggers DOM update (NFR1)
- [ ] Default display: left = `{time}`, right = `{cwd}` (FR9)

**Estimated Effort**: medium

---

### Phase 3: OSC Protocol and Custom Commands

**Goal**: External processes can inject content via OSC 777;statusbar protocol. Users can define custom commands that execute periodically.

**Files to Create**:
- `src/status-bar/osc-controller.ts` - OSC layer content management, HTML stripping, show/hide
- `src/status-bar/providers/command-provider.ts` - Custom command execution with per-command intervals

**Files to Modify**:
- `src/terminal-app/osc-handler.ts` - Add "statusbar" verb routing in OSC 777 handler (case 100)
- `src/terminal-app/osc-handler.ts` - Add statusBarOscCallback to OscHandlerContext
- `src/status-bar/index.ts` - Wire OscLayerController and CustomCmdProvider
- `src/settings/sections/status-bar-section.ts` - Add custom command definition UI

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OscLayerController | Manage OSC layer content, strip HTML, show/hide | StatusBarRenderer initialized | OSC layer updates on protocol commands |
| CommandProvider | Execute user-defined commands at intervals, capture stdout | Command path valid, executable | Command output as variable value |
| OSC Router | Route statusbar verb from OSC 777 to OscLayerController | OscHandlerContext has statusbar callback | Commands dispatched correctly |

**Processing Flow** (diagram-convertible):
1. PTY output contains OSC 777;statusbar;... -> WASM parser -> osc-handler.ts case 100
2. Verb "emterm", params[0] "statusbar" -> route to OscLayerController
   - "set;left;content" -> strip HTML tags -> set OSC layer left text -> auto-show OSC layer
   - "set;right;content" -> strip HTML tags -> set OSC layer right text -> auto-show OSC layer
   - "clear" -> clear all OSC content -> auto-hide if empty
   - "clear;left" / "clear;right" -> clear specific side
   - "show" / "hide" -> explicit visibility control of OSC layer
   - Unknown command -> ignore, log at debug level
3. Custom command execution -> timer fires per command interval
   - Execute single executable path (no arguments, no shell expansion)
   - Capture stdout -> use as `{cmd:name}` value
   - Execution fails or times out -> empty string

**Implementation Steps**:
1. **OscLayerController** - Set/clear/show/hide operations, HTML tag stripping for security (NFR2), auto-show on content set
2. **OSC 777 Routing** - Add "statusbar" branch in existing case 100 handler, pass OscLayerController callback via context
3. **CommandProvider** - Execute single executable path via Tauri shell command, per-command interval, timeout handling
4. **Custom Command Settings UI** - Add command definition interface (name, executable path, interval_ms) to status bar section
5. **Security Enforcement** - Validate executable path (no arguments, no shell expansion), strip HTML from all OSC content

**Dependencies**: Requires Phase 1 and Phase 2

**Testing Approach**:
- Unit: OSC command parsing, HTML tag stripping, set/clear/show/hide state, command output handling
- Integration: OSC 777;statusbar routes through osc-handler to controller, settings change updates commands
- E2E (Docker): Send OSC sequence, verify status bar content updates

**Acceptance Criteria**:
- [ ] OSC 777;statusbar;set;left;content sets OSC layer left (FR7)
- [ ] OSC 777;statusbar;clear clears all OSC content (FR7)
- [ ] HTML tags stripped from OSC content (NFR2)
- [ ] OSC layer auto-shows when content set, auto-hides when cleared
- [ ] `{cmd:name}` resolves custom command output (FR6)
- [ ] Custom commands accept only executable path (no arguments) (FR6)
- [ ] Command failure/timeout shows empty string
- [ ] Works on Linux and Windows (NFR3)

**Estimated Effort**: medium

---

## Complete File Structure

```
src/
├── status-bar/
│   ├── index.ts                    # StatusBarUI main class (lifecycle, settings)
│   ├── renderer.ts                 # DOM rendering, layer management, visibility
│   ├── template-engine.ts          # Template variable parsing and resolution
│   ├── osc-controller.ts           # OSC layer content management, HTML stripping
│   ├── types.ts                    # Interfaces and type definitions
│   └── providers/
│       ├── types.ts                # VariableProvider interface
│       ├── time-provider.ts        # {time} variable
│       ├── cwd-provider.ts         # {cwd} variable
│       ├── git-provider.ts         # {git_branch} variable with state colors
│       └── command-provider.ts     # {cmd:name} custom command variables
├── styles/
│   └── status-bar.css              # Status bar styles
├── settings/
│   └── sections/
│       └── status-bar-section.ts   # Settings panel section
├── terminal-app/
│   └── osc-handler.ts              # Modified: add statusbar verb routing
└── index.html                      # Modified: add status-bar container

src-tauri/src/commands/config/
├── settings.rs                     # Modified: statusbar fields, CustomCommand struct
├── validation.rs                   # Modified: statusbar validation rules
└── types.rs                        # Modified: if new enum types needed
```

## Testing Strategy

- **Unit**: Template parsing, variable resolution, HTML stripping, provider logic, settings defaults. Target 80%+ for core logic
- **Integration**: Settings toggle, OSC routing, variable update triggers re-render
- **E2E (Docker)**: Status bar visibility, default display, OSC content injection
- **Manual**: Visual appearance, color accuracy, mux mode compatibility, window resize behavior

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| (none)  | -       | No new external dependencies |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Git command execution performance | Medium | Low | Per-variable polling with configurable rate, pause for inactive tabs |
| Custom command security | Low | High | Single executable path only, no arguments, no shell expansion |
| Cross-platform git command differences | Low | Medium | Use standard git commands, test on both Linux and Windows |
| Status bar layout interaction with mux mode | Medium | Medium | Test early in Phase 1, ensure container is outside mux-managed area |

## Open Questions

- (none - all requirements resolved)

## Success Metrics

- [ ] All functional requirements (FR1-FR10) implemented and tested
- [ ] All test scenarios pass
- [ ] No visible impact on terminal rendering performance (NFR1)
- [ ] OSC content HTML stripping verified (NFR2)
- [ ] Works on Linux and Windows (NFR3)
- [ ] Follows existing UI patterns and design tokens (NFR4)
