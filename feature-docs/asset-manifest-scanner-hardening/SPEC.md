# Feature: asset-manifest-scanner-hardening

## Overview

`src-tauri/tests/asset_manifest.rs` guards against font-embedding regressions by
scanning Rust sources under `src-tauri/` for `include_bytes!` arguments and
comparing the result against the declared font set. Two scanning gaps let
embeddings pass through the guard silently: the build-output exclusion rule is
applied to the final path component as well as to directories, and the
`include_bytes!` argument extraction does not respect the macro call's
parenthesis boundary. This feature closes both gaps without changing the guard's
verdict on the current tree.

Requirements source: `feature-docs/asset-manifest-scanner-hardening/REQUIREMENTS.md`.

## Objectives

- Keep the font-embedding regression guard in `src-tauri/tests/asset_manifest.rs`
  from silently passing when future paths or files are added.
- Improve only the accuracy of the scan, leaving the guard's verdict on the
  current tree (7 embedded fonts, matched against the declaration) unchanged.

## Technical Requirements

### Functional Requirements

- **FR1 — Apply the `target` prefix exclusion to directory components only:**
  `is_excluded_path` applies the `BUILD_OUTPUT_PREFIX` (`"target"`) prefix rule
  only to the components of the relative path excluding the final one (that is,
  the directory portion). Ordinary files whose name starts with `target` (e.g.
  `src/target_resolver.rs`, `src/target.rs`) therefore stay in scope. Exclusion
  of the directories themselves is preserved as before by this rule together
  with the `is_build_output_dir(name_str)` check inside `collect_rust_source`
  (currently `asset_manifest.rs:209`).
  Evidence: `src-tauri/tests/asset_manifest.rs:175-183` (`is_excluded_path`
  applies the rule to every `Component::Normal`), `:194-219`
  (`collect_rust_source` calls `is_excluded_path` before the directory check).

- **FR2 — Confine `include_bytes!` argument extraction to the call's
  parentheses:** after matching the fixed substring `include_bytes!(`,
  `embedded_font_names` determines the argument region as everything up to that
  call's matching closing parenthesis, and restricts the string-literal search to
  that region. Scanning resumes immediately after that call's closing
  parenthesis, never from a quote found outside the call.
  Evidence: `src-tauri/tests/asset_manifest.rs:86-113` (after the needle match it
  searches for the next `"` without regard to boundaries and advances `rest` to
  just past the quote).

- **FR3 — Keep the scan result on the current tree unchanged:** after the change,
  the 7 embeddings in `src-tauri/src/render/font/resolver.rs`
  (NotoSansCJKjp-Regular.otf / NotoSansCJKjp-Bold.otf / Noto-COLRv1.ttf /
  NotoEmoji-Regular.ttf / Inconsolata-Regular.otf / Inconsolata-Bold.otf /
  NotoSansSymbols2-Regular.ttf) are still extracted, and
  `include_bytes!({abs_str:?})` at `src-tauri/build.rs:160` still yields the empty
  set. Today that site runs past the call and picks up the quote belonging to the
  `println!("cargo:rerun-if-changed=...")` side (without producing a false
  positive, because that is not a font extension); once FR2 is applied, that
  traversal no longer happens at all.
  Evidence: `src-tauri/src/render/font/resolver.rs:21,27,34,39,50,58,64`;
  `src-tauri/build.rs:160`; the other `include_bytes` occurrences in build.rs
  (lines 8, 42, 44, 68) are comments without `(` and do not match the needle.

- **FR4 — Parenthesis delimiter form only:** the scan continues to target only the
  parenthesis form `include_bytes!( ... )`. Support for the bracket form
  `include_bytes![...]` and the brace form `include_bytes!{...}` is not added.
  The only addition in this change is boundary detection.
  Evidence: answer `requirement.include-bytes-delimiter-forms = parens_only`
  (Codex scanned 287 Rust sources under `src-tauri/` and found zero bracket/brace
  forms and zero `concat!`-assembled embeds).

- **FR5 — A call with no string literal inside the parentheses extracts nothing
  and moves on:** only the first string literal within the determined
  parenthesis region is treated as the argument path. When no string literal
  exists in that region, nothing is extracted from that call and scanning
  continues from just after the closing parenthesis (it does not `break` and
  abandon the rest of the scan). Nested macros (`concat!` and the like) are not
  descended into.
  Evidence: answer `requirement.non-literal-macro-argument = skip_non_literal`
  (`build.rs:160` is the only non-literal call shape in the tree and must yield
  the empty set).

- **FR6 — A recurrence-detecting test for each of the two gaps:** add tests to
  `src-tauri/tests/asset_manifest.rs` that detect recurrence of FR1 and of FR2
  respectively (TS1 through TS5). The existing tests
  (`walk_excludes_build_output_directories`, `walk_excludes_its_own_source_file`,
  `exactly_the_documented_fonts_are_embedded`, and the rest) must pass unmodified.
  Evidence: the definition of done in the task description — "there is a test that
  detects recurrence".

### Non-Functional Requirements

- **NFR1 — Structure:** preserve the IMPLEMENTATION D5 structure — the comparison
  and text-analysis functions stay pure functions, and filesystem access is
  confined to the repository-binding section at the end of the file
  (`collect_rust_source` / `read_from_crate_root` / `crate_root`).
- **NFR2 — Dependencies:** add no new dependency. Use only std and the existing
  dev-dependency (`tempfile = "3"`). Do not introduce proptest, criterion, or any
  other test framework.
- **NFR3 — Build-output exclusion:** build-output directories (`target` /
  `target-host` / `target-win` and directories sharing that prefix) continue to be
  left out of the scan.
- **NFR4 — Performance:** keep the scan cost roughly linear in the length of the
  source text (a single-pass text scan).
- **NFR5 — Change containment:** the change is confined to
  `src-tauri/tests/asset_manifest.rs`. Production code (`src-tauri/build.rs`,
  `src-tauri/src/render/font/resolver.rs`) is not modified. `build.rs:160` is a
  reference instance of gap 2's shape, not a target of repair.
- **NFR6 — Conventions:** match the existing test naming convention
  `fn <subject>_<scenario>_<expected>()` and the construction style of the
  existing tests in the same file (synthetic text / temporary trees via
  `tempfile::tempdir`).

## Implementation Approach

### Scope of change

All changes are confined to `src-tauri/tests/asset_manifest.rs` (NFR5). Two pure
functions in that file are affected, plus the new tests:

```
src-tauri/tests/asset_manifest.rs
├── is_excluded_path        # FR1 — prefix rule limited to directory components
├── embedded_font_names     # FR2, FR4, FR5 — parenthesis-bounded argument region
├── collect_rust_source     # unchanged; is_build_output_dir check preserved (FR1)
└── (new tests)             # FR6 — TS1..TS5
```

### Data flow

```
crate_root() → collect_rust_source() → source text
             → embedded_font_names() → font file names
             → compared against documented_embedded_fonts() / fetch script / inventory
```

`collect_rust_source` keeps its build-output directory check
(`is_build_output_dir(name_str)`, currently `asset_manifest.rs:209`), so directory
exclusion is unaffected by the narrowing in `is_excluded_path` (FR1, NFR3).

### Dependencies

**Internal:** none beyond the file itself and the tree it scans
(`src-tauri/src/render/font/resolver.rs`, `src-tauri/build.rs` — read-only
references).

**External:** std and the existing dev-dependency `tempfile = "3"`
(`src-tauri/Cargo.toml:202-204`). No new dependency (NFR2, AC7).

## Declared Change Set

This section states the create-plan derivation instead of a hand-authored list:
the feature-specific paths are derived at create-plan from every task's `files`
entries in `workflow.yaml` (`references/phases/create-plan-phase.md`).

Every SPEC declares, by default, the following two workflow-generated entries in
addition to the feature-specific paths:

- `feature-docs/asset-manifest-scanner-hardening/**`
- `test-docs/asset-manifest-scanner-hardening/**`

`feature-docs/{feature}/**` covers `REQUIREMENTS.md`, `SPEC.md`,
`IMPLEMENTATION.md`, `workflow.yaml`, `phase-state/`, `tasks/`,
`reviews/roundN.yaml`, `VERIFICATION.md`, `retrospect.yaml`, and the design
artifacts the design step produces. These are generated and owned by the phase
documents and by `references/phase-state.md`; this section cites them and
restates none of their rules.

`test-docs/{feature}/**` covers
`test-docs/asset-manifest-scanner-hardening/{T}.tests.yaml`, the per-task test
record. It is generated and owned by `implement-phase.md`; this section cites it
and restates none of its rules.

These two default entries are part of the declaration unless the SPEC author
explicitly removes them; their absence is never assumed by silence — removal is a
deliberate, explicit narrowing.

This declaration is a SUPERSET assertion: the actual change set observed at
verification time must be CONTAINED IN the declared set, not equal to it. A
feature that produces no implement tasks generates no
`test-docs/{feature}/` directory at all; the declared `test-docs/{feature}/**`
entry is still correct in that case — a declared path that never materializes is
not a violation.

## Acceptance Criteria

- [ ] **AC1:** placing a `.rs` file whose name starts with `target` under
      `src-tauri/` and `include_bytes!`-ing an undeclared font in it makes
      `embedded_fonts_are_declared_in_fetch_script_and_inventory` fail (the
      reproduction steps in the task description no longer reproduce).
- [ ] **AC2:** `is_excluded_path` keeps excluding paths containing a directory
      component that starts with `target`, and does not exclude an ordinary file
      name (final component) that starts with `target`.
- [ ] **AC3:** `embedded_font_names` returns the empty set for text shaped like
      `build.rs:160` (a format string containing `include_bytes!({abs_str:?})`)
      and does not pick up the unrelated string literal that follows.
- [ ] **AC4:** when a real font embedding follows a call with a non-literal
      argument, exactly the one real embedding is extracted (the scan is not cut
      short).
- [ ] **AC5:** on an unmodified tree, `scan_embedded_fonts(crate_root())` keeps
      matching the 7 entries of `documented_embedded_fonts()` exactly.
- [ ] **AC6:** `CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --test asset_manifest`
      is green. The existing 12 tests pass unmodified.
- [ ] **AC7:** there is no diff in the dependencies of `Cargo.toml`.

## Test Scenarios

### Unit Tests

- [ ] **TS1** (FR1, AC2): `is_excluded_path(Path::new("src/target_resolver.rs"))`
      is false. `src/target.rs` (final component exactly equal to `target`) is
      false as well.
- [ ] **TS2** (FR1, NFR3, AC2): the 4 paths of the existing
      `walk_excludes_build_output_directories`
      (`target/debug/build/x/out.rs`, `target-host/...`, `target-win/...`,
      `target-anything-else/out.rs`) remain true.
      `src/render/font/resolver.rs` stays false.
- [ ] **TS4** (FR2, FR5, AC3): for synthetic text shaped like `build.rs:160` (a
      format string containing `include_bytes!({abs_str:?})` followed by a string
      literal equivalent to `println!("cargo:rerun-if-changed=...")`),
      `embedded_font_names` returns the empty set.
- [ ] **TS5** (FR2, FR5, AC4): for text where a call with a non-literal argument
      (e.g. `include_bytes!(SOME_PATH_CONST)`) is immediately followed by
      `include_bytes!("../assets/fonts/Real-Regular.ttf")`, the extraction result
      is exactly one entry, `Real-Regular.ttf`.

### Integration Tests

- [ ] **TS3** (FR1, AC1): creating `src/target_resolver.rs` in a
      `tempfile::tempdir` tree containing
      `include_bytes!("../assets/fonts/Fake-Regular.ttf")` makes
      `scan_embedded_fonts(root)` return `Fake-Regular.ttf` (the current
      implementation returns the empty set). Additionally verify that the same
      embedding placed at `target/x.rs` still yields the empty set.
- [ ] **TS6** (FR3, AC5): the existing `exactly_the_documented_fonts_are_embedded`
      passes unmodified and the set of 7 entries does not change.
- [ ] **TS7** (FR6, AC6): the whole `--test asset_manifest` suite is green
      (existing tests plus the added tests).

### E2E Tests

**Existing E2E tests**: None
**Run command**: Not detected

## Assumptions

- **a1:** this change presupposes that the purity of the comparison and analysis
  functions (IMPLEMENTATION D5) and the one-way judgement (D6) are maintained.
  Source: the documentation comment at the head of
  `src-tauri/tests/asset_manifest.rs` and the existing tests. Reversible.
- **a2:** the exclusion of build-output directories is maintained as an invariant
  pinned by the existing test `walk_excludes_build_output_directories`.
  Source: pinned by the existing test. Reversible.
- **a3:** the set of 7 embedded fonts is a fact about the current tree and is not
  changed by this work. Source: the existing test
  `exactly_the_documented_fonts_are_embedded` / `resolver.rs`. Reversible.
- **a4:** the scan exclusion of `tests/asset_manifest.rs` itself (the
  `SELF_RELATIVE_PATH` rule) is maintained; it is required so the synthetic text
  in the added tests is not mistaken for a real embedding. Source: the existing
  test `walk_excludes_its_own_source_file`. Reversible.
- **a5:** the scan keeps targeting only the parenthesis form of `include_bytes!`,
  adding boundary detection alone (bracket and brace forms are out of scope).
  Source: answer `requirement.include-bytes-delimiter-forms` (`parens_only`,
  batch-codex-consultation). Reversible.
- **a6:** only the first string literal inside the parentheses is examined; when
  there is none, that call extracts nothing and the scan moves on (nested macros
  are not descended into). Source: answer
  `requirement.non-literal-macro-argument` (`skip_non_literal`,
  batch-codex-consultation). Reversible.
- **a7:** no new dev-dependency is added; the temporary-tree tests can be written
  with the existing `tempfile = "3"` and std alone. Source:
  `src-tauri/Cargo.toml:202-204`. Reversible.
- **a8:** `src-tauri/build.rs:160` is a reference instance of the same shape, not
  a target of repair (it is the intended form on the generated-code side).
  Source: the "該当箇所" field of the task description and the generated-code
  structure of build.rs. Reversible.

## Success Criteria

- [ ] All functional requirements (FR1–FR6) are implemented and tested
- [ ] All acceptance criteria (AC1–AC7) are satisfied
- [ ] All test scenarios (TS1–TS7) pass
- [ ] Non-functional requirements (NFR1–NFR6) are satisfied

## Open Questions

> **Note**: 未解決の要件は workflow.yaml で `status: tbd` として管理されています。
> plan フェーズの実行前に解決してください。

None. Every requirement (FR1–FR6, NFR1–NFR6) is resolved.

## Design Step

Skipped. This change has no user-visible UI / UX surface: it is a hardening of
text-scanning logic confined to a single existing integration test file
(`src-tauri/tests/asset_manifest.rs`). It adds no new screen, component, or
design token and does not touch the design system. Gate
`create-spec.design-step` adopted this recommendation as-is via the answer
`decide_autonomously`.

## References

- Requirements document: `feature-docs/asset-manifest-scanner-hardening/REQUIREMENTS.md`
- Scanner under change: `src-tauri/tests/asset_manifest.rs`
  (`is_excluded_path` :175-183, `embedded_font_names` :86-113,
  `collect_rust_source` :194-219)
- Embedding sites: `src-tauri/src/render/font/resolver.rs` :21,27,34,39,50,58,64
- Gap 2 reference instance: `src-tauri/build.rs` :160
- Dev-dependency: `src-tauri/Cargo.toml` :202-204
