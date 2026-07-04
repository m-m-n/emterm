# Implementation Plan: Markdown Viewer Front Matter Support

## Overview

Add a pre-processing stage to the TS Markdown viewer that detects YAML /
TOML / JSON front matter, strips it from the body, and mounts a
collapsed-by-default block (label + format badge, fully expanded tree on
open) above the rendered body.

## Technology Stack

- **Language**: TypeScript (vanilla, no framework) — child WebView bundle
  code under `src-tauri/web-shared/` / `src-tauri/viewer/web/`
- **Test runner**: `bun test` with `test-setup.ts` (happy-dom + i18n init)
- **Existing rendering**: marked + DOMPurify (`web-shared/markdown/renderer.ts`)
- **New dependencies**: one YAML parser and one TOML parser JS library —
  selected and added inside task0002 (candidates per SPEC.md)
- **Porting reference**: `legacy/webview` branch `src/data-viewer/`
  (tree-builder, data-viewer.css look)

## Layer Structure

| Layer | Modules | Rule |
|-------|---------|------|
| Pure logic (no DOM) | `frontmatter/extractor.ts`, `frontmatter/parser.ts`, `frontmatter/tree-builder.ts` | No DOM/document access; unit-testable as plain functions |
| View (DOM builder) | `frontmatter/view.ts`, `frontmatter/frontmatter.css` | Builds the block subtree programmatically; consumes pure-logic outputs only |
| Integration | `markdown/renderer.ts`, `viewer/web/entry.ts` | Wires extraction into the render pipeline and mounts the block above the body |

Dependency direction: Integration → View → Pure logic. Never the reverse.

## Shared Components

All shared contracts live in `src-tauri/web-shared/markdown/frontmatter/types.ts`
(created by task0001; consumed by task0002 / task0004 / task0005).

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Format discriminator (`types.ts`) | Name the three supported formats | Exactly three values: yaml / toml / json | task0001, task0002, task0004, task0005 |
| Extraction result (`types.ts`) | Carry the outcome of detection | Either "no front matter" (body = original source, unmodified), or "found" with: format, raw content (delimiters excluded), and body with the block plus its trailing newline removed | task0001, task0002, task0004, task0005 |
| Parse result (`types.ts`) | Carry the outcome of parsing | Either success with a plain JS value tree (object / array / string / number / boolean / null), or failure with the parser's error message string | task0002, task0004, task0005 |
| Extractor (`extractor.ts`) | FR1/FR2 detection + body stripping | Pre: full Markdown source string. Post: extraction result per SPEC.md FR1 rules (BOM tolerated; blank first line, missing closing delimiter, unbalanced brace → "no front matter") | task0005 |
| Parser (`parser.ts`) | FR3 format dispatch | Pre: raw content + format. Post: parse result; any parser exception is captured as failure, never thrown | task0005 |
| Tree builder (`tree-builder.ts`) | FR5 flat node list | Pre: parsed JS value. Post: flat node array in display order — key, depth, path, value, has-children per node; array elements keyed `[i]`; recursion capped at depth 128. Defines and exports its own node type (self-contained; NOT in types.ts) | task0004 |
| Block view (`view.ts`) | FR4/FR5/FR6 DOM | Pre: extraction + parse results (+ tree nodes). Post: a single detached DOM element, collapsed by default; caller mounts it | task0005 |

## Conventions

- **XSS policy (NFR1)**: every front-matter-derived string enters the DOM
  via text-node / escaped paths only; the block DOM is built element-by-
  element, never from concatenated HTML strings.
- **Styling (NFR2)**: `frontmatter.css` uses `--md-sys-*` tokens only (no
  hard-coded colors); tree look follows the native data viewer.
- **i18n (NFR3)**: user-visible strings (parse-error notice) added to
  `web-shared/i18n/locales/{en,ja}.json`. "Front Matter" label and format
  badges (YAML / TOML / JSON) are proper nouns / format names, not
  translated.
- **Tests**: colocated `*.test.ts` next to sources, same style as existing
  `mermaid-renderer.test.ts`; DOM tests rely on happy-dom via `test-setup.ts`.
- **Formatting**: Biome (repo config), 2-space indent.

## Cross-task Design Decisions

### D1: Shared types are established in wave 1 (types.ts, task0001)

Extractor, parser, view, and integration all exchange data through the
contracts in `types.ts`. task0001 creates the file together with the
extractor; later-wave tasks import it and MUST NOT edit it. This keeps
task0002 / task0004 / task0005 conflict-free and worktree-independent.

### D2: Pure logic / DOM separation

Extraction, parsing, and tree building are DOM-free so their unit tests run
without happy-dom setup and the JSON brace-balancing logic (FR1) is testable
in isolation. Only `view.ts` touches the DOM. Affects: task0001, task0002,
task0003, task0004.

### D3: Renderer keeps its string API; the entry mounts the block

`MarkdownRenderer.render()` continues to return an HTML string. Front matter
handling is wired at the integration layer (task0005): extract first, render
the stripped body through the existing pipeline, and mount the block element
above the rendered body in the viewer container. This avoids embedding a
DOM component inside a string-producing API and keeps FR7 passthrough
trivially safe (no extraction hit → identical pipeline). Affects: task0004
(view exposes a mountable element), task0005.

### D4: Tree builder is a self-contained legacy port

`tree-builder.ts` is ported from `legacy/webview` `src/data-viewer/
tree-builder.ts` (flat node list, always-fully-expanded, MAX_DEPTH 128) and
defines its own node type instead of using types.ts. This lets task0003 run
in wave 1 in parallel with task0001 without file overlap. Affects:
task0003, task0004.

### D5: Extractor decides "is front matter", parser decides "is valid"

The extractor only checks delimiter integrity (FR1): a well-delimited block
is extracted even if its content is garbage. The parser then reports
success/failure (FR3/FR6). The fallback UI state (FR6) is derived purely
from the parse result. This keeps the "unterminated → body" vs "broken →
quarantined" distinction in one place each. Affects: task0001, task0002,
task0004, task0005.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| JSON brace balancing mishandles string literals / escapes | Medium | High (body corruption) | Dedicated unit tests (TS-3, TS-5) incl. `{`/`}` and escaped quotes inside strings |
| YAML/TOML library choice bloats the bundle or fights the bundler | Low | Medium | task0002 evaluates size + bun-bundler compatibility before committing; smallest adequate lib wins |
| happy-dom behavior differs from WebKitGTK/WebView2 for the toggle UI | Low | Medium | Keep view logic to explicit class/state toggling; manual verification (MT-1/MT-2) covers real WebViews |
| Regression in front-matter-less rendering (FR7) | Low | High | TS-12 pins pre-change output; fast-path returns the original string untouched |

## Open Questions

- [ ] None
