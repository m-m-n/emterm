# Implementation Plan: Markdown Viewer Enhancement (Outline + Mermaid)

## Overview

Add two new capabilities to the fullscreen Markdown viewer: a left-side outline panel for heading navigation (visible when viewport >= 1200px), and Mermaid diagram rendering via lazy-loaded mermaid.js.

## Objectives

- Display h1-h3 headings as a clickable, scroll-synced outline panel on the left side
- Render `mermaid` code blocks as SVG diagrams using mermaid.js
- Maintain all existing viewer functionality (zoom, keyboard nav, copy buttons, link dialog)

## Prerequisites

### Development Environment
- Bun (package manager and bundler)
- Tauri CLI

### Dependencies
- `mermaid` npm package (already installed: `^11.12.2`)
- No additional dependencies required

## Architecture Overview

### Technology Stack
- **Language**: TypeScript (frontend)
- **Framework**: Vanilla TypeScript + Tauri WebView (WebKit)
- **Key Libraries**: mermaid.js (diagram rendering), marked (Markdown parsing), DOMPurify (sanitization)

### Design Approach

Post-processing pipeline: Markdown is first rendered to HTML via marked + DOMPurify (existing flow). Then two post-processing steps run on the sanitized DOM:

1. **OutlinePanel** scans h1-h3 elements and builds a navigation tree
2. **MermaidRenderer** detects `.language-mermaid` code blocks, lazy-loads mermaid.js, and replaces them with SVG

This approach preserves the existing security model — DOMPurify sanitizes all user content first, and Mermaid-generated SVG is injected as a controlled post-processing step.

### Component Interaction

```
FullscreenMarkdownView.show()
  ├── Creates overlay + content (existing)
  ├── OutlinePanel.build(contentElement)
  │     ├── Scans h1-h3 elements, assigns IDs if missing
  │     ├── Builds outline tree DOM
  │     ├── Sets up IntersectionObserver for active tracking
  │     └── Registers click handlers for smooth scroll
  ├── MermaidRenderer.renderAll(contentElement)
  │     ├── Finds .language-mermaid code blocks
  │     ├── Dynamic import('mermaid') on first use
  │     ├── Renders each block to SVG
  │     └── Replaces code block with SVG container
  └── Responsive: shows/hides outline on resize
```

## Implementation Phases

### Phase 1: Mermaid Renderer

**Goal**: Render `mermaid` code blocks as SVG diagrams in the fullscreen viewer.

**Files to Create**:
- `src/markdown/mermaid-renderer.ts` - MermaidRenderer class
- `src/markdown/mermaid-renderer.test.ts` - Unit tests

**Files to Modify**:
- `src/markdown/fullscreen.ts` - Call MermaidRenderer after content insertion
- `src/markdown/fullscreen.css` - Mermaid container styling

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| MermaidRenderer | Detect mermaid code blocks and render to SVG | Content element with code blocks inserted in DOM | Mermaid blocks replaced with SVG containers |
| MermaidRenderer.initMermaid | Lazy-load and configure mermaid.js | First mermaid block detected | mermaid initialized with dark theme, securityLevel strict |
| MermaidRenderer.renderBlock | Render single mermaid source to SVG | mermaid initialized, source text available | Code block replaced with SVG, or kept on error |

**Processing Flow**:
1. Scan content for `pre > code.language-mermaid` elements
   - No mermaid blocks found -> return immediately (no library load)
   - Mermaid blocks found -> proceed to step 2
2. Lazy-load mermaid.js via dynamic import (cached after first load)
3. Initialize mermaid with configuration: theme=dark, securityLevel=strict
4. For each mermaid code block:
   - Extract text content
   - Call mermaid render API with unique ID
   - Success -> wrap SVG in a container div with class `mermaid-diagram`, replace the code block wrapper
   - Failure -> leave original code block unchanged (fallback)

**Implementation Steps**:
1. **Create MermaidRenderer class** - Encapsulates lazy loading, initialization, and rendering logic
2. **Implement mermaid detection** - Find code blocks with language-mermaid class in content element
3. **Implement lazy loading** - Dynamic import of mermaid, initialize with dark theme and strict security
4. **Implement block rendering** - For each detected block, render to SVG and replace in DOM
5. **Implement error fallback** - On render failure, preserve original code block
6. **Integrate into FullscreenMarkdownView** - Call renderAll after content is inserted into DOM
7. **Add CSS for mermaid containers** - Styling for SVG containers (max-width, centering, margin)

**Dependencies**: None (standalone)

**Testing Approach**:
- Unit: mermaid code block detection, fallback behavior on render error, no-op when no mermaid blocks
- Integration: SVG rendered in fullscreen view

**Acceptance Criteria**:
- [ ] Mermaid code blocks render as SVG
- [ ] No mermaid blocks -> mermaid.js not loaded
- [ ] Render error -> original code block preserved
- [ ] securityLevel strict configured
- [ ] Dark theme applied

**Estimated Effort**: medium

---

### Phase 2: Outline Panel

**Goal**: Display a left-side outline panel with h1-h3 heading navigation and scroll-synced active tracking.

**Files to Create**:
- `src/markdown/outline.ts` - OutlinePanel class
- `src/markdown/outline.css` - Outline panel styles
- `src/markdown/outline.test.ts` - Unit tests

**Files to Modify**:
- `src/markdown/fullscreen.ts` - Integrate outline panel into layout
- `src/markdown/fullscreen.css` - Responsive 2-column layout
- `src/markdown/types.ts` - Add outline-related types
- `src/markdown/index.ts` - Export OutlinePanel

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| OutlinePanel | Build and manage heading navigation | Content element with rendered headings | Outline DOM created, event listeners attached |
| OutlinePanel.extractHeadings | Scan DOM for h1-h3, build tree | Content element in DOM | Heading array with text, level, element refs |
| OutlinePanel.buildDOM | Create outline panel DOM | Heading array available | Panel element with clickable items |
| OutlinePanel.setupScrollTracking | IntersectionObserver for active heading | Panel and content in DOM | Active heading updates on scroll |
| OutlinePanel.dispose | Cleanup observers and listeners | Panel active | All resources released |

**Processing Flow**:
1. Extract headings from content element
   - Scan for h1, h2, h3 elements
   - No headings found -> do not create panel, return null
   - Headings found -> assign IDs to elements if missing, build heading tree
2. Build outline panel DOM
   - Create panel container with class `markdown-outline-panel`
   - Create list items with indentation based on heading level
   - Attach click handlers for smooth scroll
3. Set up IntersectionObserver on heading elements
   - Track which headings are in/near viewport
   - Update active class on corresponding outline item
4. Return panel element for insertion into overlay

**Implementation Steps**:
1. **Create OutlinePanel class** - Heading extraction, DOM building, and lifecycle management
2. **Implement heading extraction** - Scan content for h1-h3, build structured array with text, level, and element reference
3. **Implement panel DOM creation** - Render heading tree as nested list with indentation per level
4. **Implement click navigation** - Smooth scroll to heading on outline item click
5. **Implement scroll tracking** - IntersectionObserver to highlight current heading in outline
6. **Implement responsive layout** - Modify fullscreen overlay CSS for 2-column layout at >= 1200px
7. **Integrate into FullscreenMarkdownView** - Build outline, insert into overlay, handle resize

**Dependencies**: Phase 1 not strictly required, but completing Mermaid first avoids layout conflicts during development.

**Testing Approach**:
- Unit: heading extraction (correct levels, correct text), empty heading case, tree building, ID assignment
- Unit: dispose cleans up observers
- Integration: outline + content layout in fullscreen view

**Acceptance Criteria**:
- [ ] h1-h3 extracted and displayed in outline
- [ ] h4-h6 ignored
- [ ] No headings -> no outline panel
- [ ] Click navigates to heading with smooth scroll
- [ ] Active heading highlighted on scroll
- [ ] Outline visible at >= 1200px, hidden below
- [ ] Content area maintains max-width 900px

**Estimated Effort**: medium

---

### Phase 3: Integration and Polish

**Goal**: Ensure all features work together, update exports, and verify no regressions.

**Files to Modify**:
- `src/markdown/fullscreen.ts` - Final integration of outline + mermaid with existing features (zoom, keyboard nav)
- `src/markdown/fullscreen.css` - Final responsive layout refinements
- `src/markdown/index.ts` - Export new components
- `src/markdown/types.ts` - Finalize type definitions

**Key Components**:

| Component | Responsibility | Precondition | Postcondition |
|-----------|----------------|--------------|---------------|
| FullscreenMarkdownView | Orchestrate outline + mermaid + existing features | All sub-components implemented | Complete viewer with outline and mermaid |

**Processing Flow**:
1. On show():
   - Create overlay and content (existing)
   - Call MermaidRenderer.renderAll(content)
   - Call OutlinePanel.build(content)
   - If outline returned -> insert into overlay, apply 2-column layout
   - Set up resize observer for responsive outline toggle
2. On close():
   - Dispose outline panel (disconnect observers)
   - Clean up resize observer
   - Existing cleanup (zoom, keyboard, link dialog)

**Implementation Steps**:
1. **Verify zoom compatibility** - Ensure CSS zoom works with outline panel and mermaid SVGs
2. **Verify keyboard navigation** - Arrow keys, Page Up/Down, Home/End, Escape all work with new layout
3. **Verify focus management** - Tab key cycles through outline items and content links
4. **Update module exports** - Add OutlinePanel and MermaidRenderer to index.ts
5. **Run existing tests** - Ensure no regressions in fullscreen, renderer, session, link-dialog tests

**Dependencies**: Requires Phase 1 and Phase 2.

**Testing Approach**:
- Integration: full viewer with outline + mermaid + zoom + keyboard
- E2E (Docker): markdown display with mermaid blocks
- Manual: visual layout verification, responsive behavior

**Acceptance Criteria**:
- [ ] Outline and mermaid work together in fullscreen view
- [ ] Zoom works with both outline panel and mermaid SVGs
- [ ] Keyboard navigation works with new layout
- [ ] Existing tests pass without regression
- [ ] Module exports updated

**Estimated Effort**: small

---

## Complete File Structure

```
src/markdown/
├── index.ts                    # MODIFIED: export OutlinePanel, MermaidRenderer
├── types.ts                    # MODIFIED: add HeadingInfo, OutlineConfig types
├── renderer.ts                 # EXISTING: no changes
├── session.ts                  # EXISTING: no changes
├── fullscreen.ts               # MODIFIED: integrate outline + mermaid
├── fullscreen.css              # MODIFIED: responsive 2-column layout, mermaid styles
├── outline.ts                  # NEW: OutlinePanel class
├── outline.css                 # NEW: outline panel styles
├── outline.test.ts             # NEW: outline unit tests
├── mermaid-renderer.ts         # NEW: MermaidRenderer class
├── mermaid-renderer.test.ts    # NEW: mermaid renderer unit tests
├── link-dialog.ts              # EXISTING: no changes
├── link-dialog.css             # EXISTING: no changes
├── fullscreen.test.ts          # EXISTING: may need updates for new layout
├── fullscreen-lifecycle.test.ts # EXISTING: no changes expected
├── integration.test.ts         # EXISTING: may add new integration tests
├── renderer.test.ts            # EXISTING: no changes
├── security.test.ts            # EXISTING: no changes
├── session.test.ts             # EXISTING: no changes
└── link-dialog.test.ts         # EXISTING: no changes
```

## Testing Strategy

- **Unit**: Core logic coverage 80%+. OutlinePanel heading extraction, tree building, MermaidRenderer detection, error fallback.
- **Integration**: Fullscreen view with outline panel and mermaid rendering.
- **E2E (Docker)**: Existing markdown E2E tests pass. New test for mermaid rendering.
- **Manual**: Visual layout verification (outline positioning, responsive toggle, mermaid SVG appearance).

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| mermaid | ^11.12.2 | Diagram rendering (already installed) |

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Mermaid SVG rendering issues in WebKit | Low | Medium | Test in Tauri WebView, fallback to code block |
| IntersectionObserver performance with many headings | Low | Low | Threshold tuning, debounce if needed |
| CSS zoom interaction with SVG | Medium | Medium | Test zoom levels with both features |
| Mermaid dynamic import bundling | Low | Medium | Verify Bun builds with dynamic import correctly |

## Open Questions

- None. All requirements resolved during specification phase.

## Success Metrics

- [ ] All FR1-FR7 functional requirements implemented
- [ ] All NFR1-NFR4 non-functional requirements met
- [ ] Unit test coverage for new modules >= 80%
- [ ] Existing E2E tests pass
- [ ] No performance regression for Mermaid-free documents
