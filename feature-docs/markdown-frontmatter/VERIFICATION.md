# Verification Document: Markdown Viewer Front Matter Support

## Overview

**Feature**: markdown-frontmatter /
**SPEC.md**: `feature-docs/markdown-frontmatter/SPEC.md` /
**IMPLEMENTATION.md**: `feature-docs/markdown-frontmatter/IMPLEMENTATION.md`

## Build Verification

- Command (webview-ts): `bun run build:viewer && bun run build:settings`
- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo check --manifest-path src-tauri/Cargo.toml`
- Expected: exit code 0, no errors

## Test Verification

- Command (webview-ts): `bun test`
- Command (rust): `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib`
- Typecheck: `bun run typecheck`
- Coverage target: all new `frontmatter/` modules have colocated tests; no
  numeric coverage gate (repo has none)

### Test Scenarios from SPEC.md

| ID | Scenario | Expected Result | Test Type |
|----|----------|-----------------|-----------|
| TS-1 | YAML front matter detected, extracted, parsed | Body stripped; YAML value tree produced | Unit |
| TS-2 | TOML front matter (`+++`) detected, extracted, parsed | Body stripped; TOML value tree produced | Unit |
| TS-3 | Bare JSON front matter via brace balancing (incl. braces/escapes in strings) | Correct block boundary; JSON value tree | Unit |
| TS-4 | No front matter (body first, `---` mid-file, blank first line, `***`) | Source passed through unmodified; no block | Unit |
| TS-5 | Unterminated delimiter (`---` unclosed, unbalanced `{`) | Treated as no front matter | Unit |
| TS-6 | Broken content inside valid delimiters | Parse-failure result; body still stripped | Unit |
| TS-7 | Tree builder: nested objects/arrays, MAX_DEPTH cap | Expected flat node list; safe truncation | Unit |
| TS-8 | View: collapsed default, label + badge, expand → tree, error state → notice + raw | Correct DOM states | Unit (happy-dom) |
| TS-9 | HTML/script in front matter values | Escaped as literal text in block DOM (XSS) | Unit (happy-dom) |
| TS-10 | Empty front matter block (`---\n---\n`) | Empty tree, no error | Unit |
| TS-11 | Pipeline: front matter document | Body HTML has no delimiter artifacts; block DOM present | Integration |
| TS-12 | Pipeline: no front matter | Output identical to pre-change renderer | Integration |

## Code Quality Verification

- Format: `bunx biome format --write src-tauri/web-shared/markdown/frontmatter`
  (repo Biome config; PostToolUse hook also enforces)
- Static analysis: `bun run typecheck` (tsc --noEmit)

## SPEC.md Compliance

### Success Criteria

| ID | Criterion | How to Verify |
|----|-----------|---------------|
| SC-1 | FR1–FR7 implemented and tested | TS-1..TS-12 all pass under `bun test` |
| SC-2 | Typecheck passes | `bun run typecheck` exit 0 |
| SC-3 | No regression for front-matter-less documents | TS-12 |
| SC-4 | XSS scenario passes | TS-9 |
| SC-5 | Viewer bundle builds with new dependencies | `bun run build:viewer` exit 0 |

### Functional Requirements Coverage

| Requirement | Tasks | Verification |
|-------------|-------|--------------|
| FR1 | task0001, task0005 | TS-1, TS-2, TS-3, TS-4, TS-5 |
| FR2 | task0001, task0005 | TS-1, TS-6, TS-11 |
| FR3 | task0002 | TS-1, TS-2, TS-3, TS-6, TS-10 |
| FR4 | task0004 | TS-8, MT-1 |
| FR5 | task0003, task0004 | TS-7, TS-8, MT-2 |
| FR6 | task0004, task0005 | TS-6, TS-8, TS-11 (error path) |
| FR7 | task0005 | TS-4, TS-12 |
| NFR1 | task0004 | TS-9 |
| NFR2 | task0004 | MT-1 (MD3 tokens: CSS scan in TS-8's suite / AC-7) |
| NFR3 | task0004 | TS-8 (locale keys present in en/ja) |
| NFR4 | task0001, task0005 | TS-12 (passthrough), PT-1 |

## E2E Testing

No automated E2E infrastructure exists (no `docker-compose.e2e.yml`, no
`e2e-tests/`; per `test/README.md` end-to-end behavior is validated
manually).

## Manual Testing (E2E Not Possible)

- [ ] MT-1: `emterm markdown` on a YAML-front-matter file — collapsed block
      with label + YAML badge above a clean body; MD3 styling consistent in
      light and dark themes (Linux WebKitGTK; Windows WebView2 if
      available).
- [ ] MT-2: Expand/collapse via header click; tree shows the data fully
      expanded and matches the file; TOML / JSON / broken-YAML variants
      show badge / error + raw text respectively.

## Performance / Security Verification

- PT-1 (NFR4): rendering a large Markdown document without front matter
  shows no perceptible slowdown vs. before the change (manual side-by-side
  or timing log; the fast path is also pinned structurally by TS-12).
- Security: covered by TS-9 (XSS escaping) — no other attack surface
  (local read-only rendering).

## Verification Summary

| Category | Items | Automated | E2E | Manual |
|----------|-------|-----------|-----|--------|
| Build | 2 | 2 | 0 | 0 |
| Unit/Integration tests | 12 (TS-1..12) | 12 | 0 | 0 |
| Code quality | 2 | 2 | 0 | 0 |
| Manual scenarios | 2 (MT-1, MT-2) | 0 | 0 | 2 |
| Performance | 1 (PT-1) | 0 | 0 | 1 |
