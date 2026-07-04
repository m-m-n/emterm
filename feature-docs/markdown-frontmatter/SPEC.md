# Feature: Markdown Viewer Front Matter Support

## Overview

The Markdown viewer currently has no front matter handling: a leading YAML
`---` block reaches marked as-is and is mis-rendered as a horizontal rule or
setext heading. This feature detects YAML / TOML / JSON front matter at the
very start of the document, strips it from the body before marked runs, and
presents the parsed data in a collapsed-by-default block with a fully
expanded tree view.

## Objectives

- Stop front matter delimiters and content from leaking into the rendered
  Markdown body.
- Let users inspect front matter metadata via a collapsible tree UI.
- Keep documents without front matter rendering exactly as before.

## User Stories

### US1: View a Markdown file with front matter
As an eMterm user, I want front matter to be shown as a collapsed metadata
block instead of garbled body content, so that the document renders
correctly and I can still inspect the metadata.

**Acceptance Criteria:**
- [ ] YAML (`---`), TOML (`+++`), and bare JSON (`{...}`) front matter at the
      start of the file is detected, stripped from the body, and shown in a
      collapsed block above the rendered body.
- [ ] Expanding the block shows an always-fully-expanded tree of the parsed
      data.

### US2: View a Markdown file with broken front matter
As an eMterm user, I want broken front matter to be quarantined with an
error notice and its raw text visible, so that the body still renders
correctly and I can spot the broken part.

**Acceptance Criteria:**
- [ ] On parse failure the block is still stripped from the body.
- [ ] The collapsed block indicates the parse failure; expanding it shows
      the raw front matter text.

### US3: View a Markdown file without front matter
As an eMterm user, I want documents without front matter to render exactly
as before, so that existing behavior does not regress.

**Acceptance Criteria:**
- [ ] No front matter block is shown and the rendered output is unchanged,
      including documents that contain `---` rules or setext headings in the
      body.

## Technical Requirements

### Functional Requirements

- **FR1: Detection & extraction.** A pre-processing step in the TS Markdown
  pipeline detects at most one front matter block at the very start of the
  source and extracts it before the source reaches marked. Format is decided
  by the leading delimiter (exclusive branch):
  - YAML: first line is exactly `---` (trailing whitespace allowed); the
    block ends at the next line that is exactly `---`.
  - TOML: same as YAML with `+++` as both delimiters.
  - JSON: the document starts with `{`; the block ends at the matching
    closing `}` found by brace balancing that is aware of JSON string
    literals and escapes (Hugo-style bare JSON object).
  - A UTF-8 BOM before the delimiter is tolerated. Any other leading
    content (including a blank first line) means "no front matter".
  - If the closing delimiter / matching brace is not found, the document is
    treated as having no front matter and the full source is rendered as
    body (FR5 does NOT apply — fallback applies only to delimited blocks
    whose content fails to parse).
- **FR2: Body stripping.** The extracted block (delimiters included, plus
  the immediately following newline) is removed from the source passed to
  marked, so delimiters can never be mis-rendered as `hr` / setext headings.
- **FR3: Parsing.** The extracted content is parsed into a plain JS value
  tree (object / array / string / number / boolean / null):
  - JSON via built-in `JSON.parse`.
  - YAML and TOML via JS libraries added as dependencies (library selection
    happens in the implementation phase).
- **FR4: Collapsed block UI.** The viewer shows a collapsible block above
  the rendered body. Default state is collapsed: only a header row with the
  label "Front Matter" and a format badge (`YAML` / `TOML` / `JSON`) is
  visible. Clicking the header toggles expansion.
- **FR5: Expanded tree view.** Expanding the block shows the parsed data as
  an always-fully-expanded tree (no per-node toggles): one row per key at
  every nesting level, array elements keyed as `[i]`, nesting shown by
  indentation, leaf values rendered with their scalar value. The
  tree-building logic is a TS re-implementation ported from the
  `legacy/webview` branch (`src/data-viewer/tree-builder.ts`); the visual
  style follows the native JSON/YAML data viewer's look. Recursion depth is
  capped (`MAX_DEPTH = 128`) and total tree size is capped by a node budget
  (`MAX_NODES = 2000`, applied to normalization at parse time as well as to
  tree building); past either cap the tree is truncated and a localized
  notice marks it as partial.
- **FR6: Parse-failure fallback.** If a properly delimited block fails to
  parse, it is still stripped from the body; the header shows a parse-error
  indication instead of the normal state, and the expanded content shows
  the raw front matter text (escaped, preformatted) instead of a tree.
- **FR7: No-front-matter passthrough.** Documents with no front matter
  (per FR1's detection rules) render byte-identically to today's output:
  no block UI is injected and the source passed to marked is unmodified.

### Non-Functional Requirements

- **NFR1 - Security (XSS):** Front matter keys, values, raw text, and error
  messages are inserted into the DOM only via escaping/`textContent`-safe
  paths, consistent with the viewer's existing DOMPurify-based XSS policy.
  Front matter can never inject active HTML.
- **NFR2 - UI conformance:** The collapsible block follows the viewer's
  effective theme (light/dark). Shape/motion use MD3 design tokens
  (`--md-sys-*` CSS variables per `doc/UI-DESIGN-GUIDELINES.yaml`); colors
  are drawn from the viewer's theme-aware palette variables
  (`--markdown-*`).
- **NFR3 - i18n:** User-visible strings (e.g. the parse-error notice) are
  provided via the shared WebView i18n mechanism
  (`src-tauri/web-shared/i18n/locales/{en,ja}.json`). The "Front Matter"
  label and format badges are format names / proper nouns and stay as-is.
- **NFR4 - Performance:** Pre-processing adds no perceptible rendering
  delay for typical documents; detection is O(front matter length), not
  O(document length), except for the initial delimiter scan.

## Implementation Approach

### Architecture

```
viewer payload (markdown source)
        │
        ▼
frontmatter extract()          ── new pre-processing step
  │            │
  │ body       │ FrontMatter { format, raw, parsed | error }
  ▼            ▼
MarkdownRenderer.render()    frontmatter block DOM (header + tree / raw)
  │                            │
  ▼                            ▼
   rendered body  ◄── block injected above body in the viewer container
```

- Extraction/parsing is pure logic (no DOM) so it is unit-testable under
  `bun test`.
- The block UI builder produces a DOM subtree; the viewer entry (or
  renderer integration point) mounts it above the rendered body.
- CLI (`emterm markdown`) is unchanged: it already ships the file content
  verbatim.
- The native data viewer (`--data-viewer`, Rust/egui) is unchanged.

### Data Flow

```
source → detect delimiter → extract raw block → parse (JSON.parse / yaml / toml)
       → { body without block, FrontMatterResult }
       → render body via marked/DOMPurify (existing path)
       → build collapsible block (header + tree | error + raw)
       → mount block above body
```

### Dependencies

**Internal Dependencies:**
- `src-tauri/web-shared/markdown/renderer.ts`: integration point for the
  pre-processing step.
- `src-tauri/web-shared/i18n/`: locale strings for the error notice.
- `legacy/webview` branch `src/data-viewer/tree-builder.ts` /
  `data-viewer.css`: porting reference for tree building and look.

**External Dependencies:**
- One YAML parser JS library and one TOML parser JS library (candidates:
  `js-yaml` / `yaml`, `smol-toml` / `@iarna/toml`) — selected in the
  implementation phase.

### File Structure

```
src-tauri/web-shared/markdown/
├── frontmatter/
│   ├── extractor.ts          # FR1/FR2: delimiter detection + extraction
│   ├── extractor.test.ts
│   ├── parser.ts             # FR3: format dispatch to JSON/YAML/TOML parsers
│   ├── parser.test.ts
│   ├── tree-builder.ts       # FR5: legacy port (flat node list, MAX_DEPTH)
│   ├── tree-builder.test.ts
│   ├── view.ts               # FR4/FR5/FR6: collapsible block + tree DOM
│   ├── view.test.ts
│   └── frontmatter.css       # MD3-token-based styles
├── renderer.ts               # integration (pre-processing hook)
└── ...
src-tauri/web-shared/i18n/locales/{en,ja}.json   # error notice strings
package.json                  # + YAML/TOML parser dependencies
```

## Test Scenarios

### Unit Tests
- [ ] TS-1: YAML front matter is detected, extracted, and parsed; body no
      longer contains the block.
- [ ] TS-2: TOML front matter (`+++`) is detected, extracted, and parsed.
- [ ] TS-3: Bare JSON front matter is detected via brace balancing,
      including `{`/`}` inside string literals and escaped quotes.
- [ ] TS-4: No front matter → source is passed through unmodified and no
      block is produced (documents starting with body text, `---` mid-file,
      blank first line, `***` etc.).
- [ ] TS-5: Unterminated delimiter (`---` with no closing line; unbalanced
      `{`) → treated as no front matter.
- [ ] TS-6: Broken content inside valid delimiters → parse error result;
      body is still stripped.
- [ ] TS-7: tree-builder produces the expected flat node list for nested
      objects/arrays and respects MAX_DEPTH.
- [ ] TS-8: view builds a collapsed-by-default block with label + format
      badge; expanding shows the tree; error state shows notice + raw text.
- [ ] TS-9: HTML/script content in front matter values is escaped in the
      block DOM (XSS).
- [ ] TS-10: Empty front matter block (`---\n---\n`) renders an empty tree
      without error.

### Integration Tests
- [ ] TS-11: Renderer pipeline with a front-matter document produces body
      HTML without `hr`/`h2` artifacts from delimiters, plus a block DOM.
- [ ] TS-12: Renderer pipeline without front matter produces output
      identical to the pre-change snapshot (regression).

### E2E Tests
**Existing E2E tests**: None (no `docker-compose.e2e.yml` / `e2e-tests/`;
end-to-end behavior is validated manually per `test/README.md`).
**Run command**: Not detected.
- [ ] Manual scenario 1: `emterm markdown` on a YAML-front-matter file shows
      the collapsed block and a clean body.
- [ ] Manual scenario 2: expanding/collapsing works; tree matches the data.

### Edge Cases
- [ ] File consisting of only front matter (empty body) → block shown,
      empty body rendered.
- [ ] Deeply nested data (> MAX_DEPTH) → tree truncated safely at the cap,
      no crash.
- [ ] CRLF line endings → delimiters still recognized.
- [ ] UTF-8 BOM before `---` → still recognized.

### Performance Tests
- [ ] Large document without front matter shows no measurable slowdown from
      the detection fast-path.

## Security Considerations

- **Input Validation:** Delimiter scanning is bounded; parse errors are
  contained to the fallback path (FR6). Recursion depth capped at 128;
  normalization and tree building bounded by the `MAX_NODES` budget;
  cyclic parse results (YAML aliases) are guarded.
- **XSS Prevention:** All front-matter-derived strings enter the DOM via
  escaped/`textContent` paths (NFR1); the block DOM is built
  programmatically, never via unsanitized `innerHTML`.
- **Authentication / Authorization / SQL / CSRF:** Not applicable
  (local, read-only rendering feature).

## Error Handling

| Case | Handling |
|------|----------|
| Parse failure inside valid delimiters | Strip from body; error notice in header; raw text (escaped) shown when expanded (FR6) |
| Missing closing delimiter / unbalanced brace | Not front matter; render whole source as body (FR1) |
| Parser library throws unexpectedly | Same path as parse failure (FR6) |

## Success Criteria

- [ ] All functional requirements (FR1–FR7) are implemented and tested
- [ ] All unit/integration test scenarios pass under `bun test`
- [ ] `bun run typecheck` passes
- [ ] No regression for front-matter-less documents (TS-12)
- [ ] XSS scenario (TS-9) passes
- [ ] Code review is completed

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. (YAML/TOML parser library selection and detailed MD3 styling are
explicitly deferred to the implementation phase by decision, not open
requirements.)

## References

- Discussion report: `tmp/discussion-markdown-frontmatter.md`
- Requirements: `feature-docs/markdown-frontmatter/REQUIREMENTS.md`
- Legacy tree implementation: `legacy/webview` branch `src/data-viewer/`
- UI tokens: `doc/UI-DESIGN-GUIDELINES.yaml`
