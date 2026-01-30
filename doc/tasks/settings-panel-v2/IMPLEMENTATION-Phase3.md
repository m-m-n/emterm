# Implementation Plan: Settings Panel - Phase 3

## Overview

Add the Rich Content settings (inline_images_enabled, markdown_rendering) to the Appearance category. These toggle switches control whether the terminal renders inline images and Markdown content via the Kitty Graphics Protocol / SIXEL and custom OSC extension respectively.

## Objectives

- Implement Rich Content subsection in Appearance category with two toggle controls
- Integrate inline_images_enabled with the image rendering pipeline
- Integrate markdown_rendering with the Markdown rendering pipeline
- Add feature flag checks in rendering code to respect these settings

## Prerequisites

### Development Environment
- Phase 1 completed (type definitions, toggle switch UI control)
- Phase 2 completed (all other settings implemented)
- All previous tests passing

### Dependencies
- Phase 1 toggle switch control (reused for both settings)
- Phase 1 type definitions (fields already defined)
- Existing image rendering pipeline (Kitty Graphics Protocol / SIXEL)
- Existing Markdown rendering pipeline (custom OSC extension)

## Architecture Overview

### Design Approach

Phase 3 is the smallest phase, adding two boolean toggle switches to the Appearance category. The main complexity is in the integration with existing rendering pipelines -- each pipeline must check the corresponding setting before processing content.

### Integration Points

```
inline_images_enabled setting
    |
    v
Image rendering pipeline --> check setting before rendering inline image
    +-- enabled: render image as normal
    +-- disabled: skip image rendering (show placeholder or nothing)

markdown_rendering setting
    |
    v
Markdown rendering pipeline --> check setting before rendering markdown block
    +-- enabled: render markdown as normal
    +-- disabled: skip markdown rendering (output raw text or nothing)
```

## Implementation Steps

### Step 1: Render Rich Content Subsection in Appearance

**Goal**: Rich Content subsection appears in Appearance category with two toggle switches.

**Files to Modify**:
- `src/settings/settings-panel.ts`

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| renderAppearanceSection | Add Rich Content subsection after Layout | Settings loaded | inline_images_enabled and markdown_rendering toggles displayed |

**Settings to Add**:

| Setting | Subsection | Control | Behavior |
|---------|-----------|---------|----------|
| inline_images_enabled | Rich Content | Toggle switch | Save on toggle; takes effect on next image received |
| markdown_rendering | Rich Content | Toggle switch | Save on toggle; takes effect on next markdown block received |

**Processing Flow**:
```
Appearance category content:
1. Font subsection (Phase 1)
2. Theme & Color subsection (Phase 1 + Phase 2)
3. Layout subsection (Phase 2)
4. Rich Content subsection (Phase 3) [NEW]
   +-- Inline Images toggle
   +-- Markdown Rendering toggle
```

**Key Considerations**:
- Reuse the Phase 1 toggle switch control -- no new UI control needed
- Both settings take effect on the next received content, not retroactively on already-rendered content
- Hint text should communicate that changes apply to new content only

**Acceptance Criteria**:
- [ ] Rich Content subsection visible in Appearance category
- [ ] Inline Images toggle renders with correct current value
- [ ] Markdown Rendering toggle renders with correct current value
- [ ] Both toggles save on click

---

### Step 2: Integrate with Image Rendering Pipeline

**Goal**: Image rendering respects the inline_images_enabled setting.

**Files to Modify**:
- Image rendering code (Kitty Graphics Protocol handler, SIXEL handler)

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Image rendering entry point | Check inline_images_enabled before processing | Settings accessible | Images rendered only when enabled |
| Setting access | Read current inline_images_enabled value | Settings loaded at startup and on change | Current value available synchronously |

**Processing Flow**:
```
1. Image data received (Kitty/SIXEL protocol)
2. Check inline_images_enabled setting
   +-- true --> proceed with image rendering
   +-- false --> skip rendering, discard image data
3. Continue terminal output processing
```

**Key Considerations**:
- The setting must be accessible from the rendering pipeline without async overhead
- Disabling images should not cause errors in the terminal output stream
- Already-rendered images remain visible when the setting is toggled off (no retroactive removal)

**Acceptance Criteria**:
- [ ] New images render when inline_images_enabled is true
- [ ] New images are skipped when inline_images_enabled is false
- [ ] Toggling the setting does not affect already-rendered images
- [ ] No errors in terminal output when images are disabled

---

### Step 3: Integrate with Markdown Rendering Pipeline

**Goal**: Markdown rendering respects the markdown_rendering setting.

**Files to Modify**:
- Markdown rendering code (custom OSC extension handler)

**Component Responsibilities**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| Markdown rendering entry point | Check markdown_rendering before processing | Settings accessible | Markdown rendered only when enabled |
| Setting access | Read current markdown_rendering value | Settings loaded at startup and on change | Current value available synchronously |

**Processing Flow**:
```
1. Markdown OSC sequence received
2. Check markdown_rendering setting
   +-- true --> proceed with markdown rendering
   +-- false --> skip rendering, output raw text or discard
3. Continue terminal output processing
```

**Key Considerations**:
- Same access pattern as inline_images_enabled -- setting must be synchronously readable
- When disabled, markdown OSC sequences should be silently consumed (no visible garbage)
- Already-rendered markdown blocks remain visible when the setting is toggled off

**Acceptance Criteria**:
- [ ] New markdown blocks render when markdown_rendering is true
- [ ] New markdown blocks are skipped when markdown_rendering is false
- [ ] Toggling the setting does not affect already-rendered markdown
- [ ] No visible artifacts when markdown rendering is disabled

---

## Dependencies Between Steps

```
Step 1 (UI toggles) -- no dependencies on Steps 2/3
Step 2 (Image integration) -- independent of Step 3
Step 3 (Markdown integration) -- independent of Step 2
```

All three steps can proceed in parallel. Step 1 is the simplest and should be completed first.

## Complete File Changes

**Files to Modify**:
- `src/settings/settings-panel.ts` - Rich Content subsection with two toggles
- Image rendering code - Feature flag check for inline_images_enabled
- Markdown rendering code - Feature flag check for markdown_rendering

## Testing Strategy

### Unit Tests (TypeScript)

| Test | Description |
|------|-------------|
| Rich Content section renders | Both toggles appear in Appearance category |
| Toggle inline_images | Saves setting correctly |
| Toggle markdown_rendering | Saves setting correctly |

### Integration Tests

| Test | Description |
|------|-------------|
| Image with enabled=true | Image renders in terminal |
| Image with enabled=false | Image data processed without rendering |
| Markdown with enabled=true | Markdown block renders |
| Markdown with enabled=false | Markdown OSC consumed without rendering |

### Manual Testing

- [ ] Open settings -- Rich Content subsection visible in Appearance
- [ ] Toggle Inline Images OFF -- send image via Kitty protocol -- no image appears
- [ ] Toggle Inline Images ON -- send image -- image appears
- [ ] Toggle Markdown Rendering OFF -- send markdown OSC -- no markdown block appears
- [ ] Toggle Markdown Rendering ON -- send markdown OSC -- markdown block appears
- [ ] Both toggles persist after restart
- [ ] Old settings file without these fields -- defaults to true (both enabled)

## Estimated Effort

Small (1-2 days)

## Risks and Mitigation

- **Risk**: Image/Markdown rendering pipeline access to settings may require architectural changes
  - **Mitigation**: Settings are loaded at startup and stored in an accessible location. The rendering pipeline can read from the same shared state. If the pipeline is in Rust backend, the setting may need to be communicated via a Tauri event or stored in backend state.

- **Risk**: Disabling rendering mid-stream may cause partial render artifacts
  - **Mitigation**: Check the setting at the entry point of each rendering operation (before any DOM manipulation begins). Partial operations are not possible since the check happens before processing starts.
