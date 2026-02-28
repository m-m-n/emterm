# Feature: Markdown Viewer Enhancement (Outline + Mermaid)

## Overview

Enhance the fullscreen Markdown viewer with two new capabilities: a left-side outline (table of contents) panel for heading navigation when viewport width permits, and Mermaid diagram rendering for `mermaid` code blocks.

## Objectives

- Add an outline panel that displays h1-h3 headings as a navigable tree on the left side
- Support Mermaid diagram rendering with lazy-loaded mermaid.js library
- Maintain existing viewer behavior and security model

## User Stories

### US1: Navigate Long Documents via Outline
As a developer, I want to see a table of contents on the left side when viewing long Markdown documents, so that I can quickly jump to specific sections.

**Acceptance Criteria:**
- [ ] Outline panel appears on the left when viewport >= 1200px wide
- [ ] Outline lists h1, h2, h3 headings in a tree structure with indentation
- [ ] Clicking a heading smoothly scrolls content to that heading
- [ ] Currently visible heading is highlighted in the outline
- [ ] Outline panel is hidden when no headings exist in the document

### US2: View Mermaid Diagrams
As a developer, I want Mermaid code blocks to render as SVG diagrams, so that I can view flowcharts, sequence diagrams, and other diagrams directly in the viewer.

**Acceptance Criteria:**
- [ ] Mermaid code blocks (```mermaid) render as SVG diagrams
- [ ] All Mermaid diagram types are supported
- [ ] Dark theme is used for diagram rendering
- [ ] On syntax error, the original source is shown as a regular code block
- [ ] mermaid.js is loaded only when needed (lazy loading)

## Technical Requirements

### Functional Requirements
- **FR1: Outline panel** - Extract h1-h3 headings from rendered HTML and display as a clickable tree in a left-side panel
- **FR2: Active heading tracking** - Highlight the currently visible heading in the outline based on scroll position (IntersectionObserver)
- **FR3: Smooth scroll navigation** - Click on an outline item to smoothly scroll the content to the corresponding heading
- **FR4: Responsive layout** - Show outline panel only when viewport width >= 1200px; hide it below that threshold
- **FR5: Mermaid rendering** - Detect `mermaid` code blocks and render them as SVG using mermaid.js
- **FR6: Mermaid lazy loading** - Dynamically import mermaid.js only when mermaid code blocks are present
- **FR7: Mermaid error fallback** - On render failure, display the original Mermaid source as a regular code block

### Non-Functional Requirements
- **NFR1 - Performance:** Mermaid lazy loading must not affect render time for documents without Mermaid blocks
- **NFR2 - Security:** Mermaid-generated SVG bypasses DOMPurify; user-authored SVG in Markdown remains blocked. Mermaid securityLevel: strict
- **NFR3 - Compatibility:** Existing keyboard navigation, zoom, copy buttons, and link handling must continue to work
- **NFR4 - Compatibility:** Existing E2E tests must pass without regression

## Implementation Approach

### Architecture

**Component Overview:**
```
FullscreenMarkdownView (modified)
├── OutlinePanel (new)         - Heading extraction, tree display, active tracking
├── markdown-fullscreen-content (existing, modified layout)
└── MermaidRenderer (new)      - Lazy load mermaid.js, render code blocks to SVG
```

**Layout (viewport >= 1200px):**
```
┌───────────────────────────────────────────────────┐
│ markdown-fullscreen-overlay (flex container)       │
│ ┌──────────┐ ┌──────────────────────────────────┐ │
│ │ Outline  │ │ markdown-fullscreen-content       │ │
│ │ Panel    │ │ (max-width: 900px, centered)      │ │
│ │ (~250px) │ │                                    │ │
│ │          │ │ ┌─────────────────────────────┐   │ │
│ │ h1       │ │ │ Mermaid SVG (rendered)      │   │ │
│ │   h2     │ │ └─────────────────────────────┘   │ │
│ │   h2     │ │                                    │ │
│ │     h3   │ │                                    │ │
│ │   h2     │ │                                    │ │
│ └──────────┘ └──────────────────────────────────┘ │
└───────────────────────────────────────────────────┘
```

**Layout (viewport < 1200px):**
```
┌───────────────────────────────────────────────────┐
│ markdown-fullscreen-overlay                        │
│ ┌───────────────────────────────────────────────┐ │
│ │ markdown-fullscreen-content                    │ │
│ │ (max-width: 900px, centered)                   │ │
│ │                                                │ │
│ │ ┌────────────────────────────────────────┐    │ │
│ │ │ Mermaid: shown as code block           │    │ │
│ │ │ (or rendered SVG if mermaid loaded)     │    │ │
│ │ └────────────────────────────────────────┘    │ │
│ └───────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────┘
```

### Data Flow

**Outline panel:**
```
Rendered HTML → scan h1-h3 elements → build heading tree → render outline
User scrolls → IntersectionObserver → update active heading in outline
User clicks outline item → scrollIntoView({ behavior: 'smooth' }) on target heading
```

**Mermaid rendering:**
```
Markdown source → marked.parse() → DOMPurify sanitize → HTML with mermaid code blocks
→ detect .language-mermaid code blocks → dynamic import('mermaid')
→ mermaid.render(source) → replace code block with SVG container
→ on error: keep original code block
```

### Dependencies

**Internal Dependencies:**
- `src/markdown/fullscreen.ts` - Main fullscreen view (layout modification)
- `src/markdown/fullscreen.css` - Styles (layout, outline panel styles)
- `src/markdown/renderer.ts` - Rendering pipeline (Mermaid code block detection)

**External Dependencies:**
- `mermaid` (npm): Diagram rendering library (lazy loaded via dynamic import)

### File Structure

```
src/markdown/
├── outline.ts          # NEW: OutlinePanel class
├── outline.css         # NEW: Outline panel styles
├── mermaid-renderer.ts # NEW: MermaidRenderer class (lazy load + render)
├── fullscreen.ts       # MODIFIED: integrate outline panel, mermaid renderer
├── fullscreen.css      # MODIFIED: responsive layout for outline
├── renderer.ts         # MODIFIED: preserve mermaid code block info for post-processing
└── types.ts            # MODIFIED: add outline/mermaid types
```

## Test Scenarios

### Unit Tests
- [ ] OutlinePanel: extracts h1-h3 headings correctly
- [ ] OutlinePanel: builds correct tree hierarchy
- [ ] OutlinePanel: returns empty array when no headings
- [ ] OutlinePanel: ignores h4-h6 headings
- [ ] MermaidRenderer: detects mermaid code blocks
- [ ] MermaidRenderer: handles render errors (fallback to code block)
- [ ] MermaidRenderer: does not load mermaid.js when no mermaid blocks exist

### Integration Tests
- [ ] Fullscreen view with outline panel renders correctly
- [ ] Outline click navigates to heading
- [ ] Mermaid diagram renders in fullscreen view

### E2E Tests
**Existing E2E tests**: `e2e-tests/specs/markdown.e2e.js`
**Run command**: `./scripts/run-e2e-docker.sh test`
- [ ] Existing E2E tests pass without regression
- [ ] Mermaid code block renders as SVG (new test)

### Edge Cases
- [ ] Document with no headings: outline panel not shown
- [ ] Document with only h4-h6: outline panel not shown
- [ ] Viewport resize across 1200px threshold: outline toggles visibility
- [ ] Mermaid syntax error: shows source as code block
- [ ] Multiple mermaid blocks in one document: all render correctly
- [ ] Zoom interaction: outline and mermaid work with CSS zoom

## Security Considerations

- **SVG Injection:** Mermaid-generated SVG is inserted directly into DOM, bypassing DOMPurify. Only SVG produced by mermaid.render() is allowed; user-authored SVG in Markdown continues to be stripped by DOMPurify.
- **Mermaid securityLevel:** Set to `strict` to disable click events and other interactive features in diagrams.
- **XSS Prevention:** DOMPurify configuration for Markdown content remains unchanged. Mermaid rendering is isolated to a post-processing step after sanitization.

## Error Handling

### Error Scenarios

| Scenario | Handling |
|----------|----------|
| Mermaid syntax error | Display original source as code block |
| mermaid.js load failure | Display mermaid blocks as regular code blocks |
| No headings in document | Do not render outline panel |

## Performance Optimization

### Strategies
- **Lazy loading:** mermaid.js is only imported when mermaid code blocks are detected in the rendered HTML
- **IntersectionObserver:** Efficient scroll tracking for active heading, avoiding scroll event listeners
- **CSS media query or resize observer:** Responsive outline panel toggle without JS overhead

## Success Criteria

- [ ] All functional requirements (FR1-FR7) are implemented and tested
- [ ] All test scenarios pass
- [ ] Existing E2E tests pass without regression
- [ ] Security requirements are satisfied (SVG isolation, strict mode)
- [ ] No performance regression for Mermaid-free documents

## Open Questions

> **Note**: No unresolved requirements.

## References

- Mermaid.js documentation: https://mermaid.js.org/
- Existing Markdown display protocol: `doc/markdown-display-protocol.md`
- Current renderer: `src/markdown/renderer.ts`
- Current fullscreen view: `src/markdown/fullscreen.ts`
