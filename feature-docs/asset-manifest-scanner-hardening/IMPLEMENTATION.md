# Implementation Plan: asset-manifest-scanner-hardening

## Overview

Close two scanning gaps in the font-embedding regression guard
(`src-tauri/tests/asset_manifest.rs`): the build-output exclusion rule is
narrowed to directory components, and byte-include macro argument extraction is
confined to the call's own parenthesis region. The guard's verdict on the
current tree stays identical (7 embedded fonts, matched against the
declarations).

## Technology Stack

- **Language**: Rust — the change lives entirely in one integration-test target
  of the `src-tauri` crate.
- **Key libraries**: the standard library only for the analysis logic; the
  already-present dev-dependency `tempfile` for temporary-tree tests.
- **New dependencies**: none. No dependency is added, removed, or version-moved,
  so no license question arises; `project.license: MIT` is unchanged and no
  license entry needs recording for this feature.

## Layer Structure

The guard file keeps the two-layer structure its original design decision (D5)
established. Both changed functions belong to the upper layer.

1. **Pure analysis layer** — comparison, text parsing and path predicates. Takes
   values in, returns values out, never touches the filesystem, never panics on
   malformed input.
2. **Repository-binding layer** — the only filesystem-touching section, at the
   end of the file: the recursive source collection, the crate-root resolution,
   and the file reads.

Allowed dependency direction: binding layer → pure layer. Never the reverse. A
change that moves filesystem access into the pure layer, or that makes a pure
function depend on process state (current directory, environment beyond the
compile-time manifest directory), violates NFR1.

## Shared Components

These three contracts are the invariants every task in this feature — and any
rework task appended later — implements against.

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Path-exclusion predicate (`is_excluded_path`) | Decide whether one repository-relative path is out of scan scope | **Pre**: a path relative to the crate manifest directory. **Post**: true when the path equals the scanner's own relative-path constant; true when at least one ordinary-name component *other than the final one* begins with the build-output name prefix; false otherwise. In particular, a final component beginning with that prefix does not on its own cause exclusion. Pure; no filesystem access. | task0001 |
| Build-output directory-name predicate (`is_build_output_dir`) | Decide whether one directory *name* is a build-output root | **Pre**: a single path-component name. **Post**: true when the name begins with the build-output prefix. **Unchanged by this feature** — it remains the sole mechanism that stops the walk from descending into a build-output directory reached at the top of the walk, because the path predicate above no longer excludes a single-component path. | task0001 |
| Embedded-font extractor (`embedded_font_names`) | List font file names embedded by the parenthesis form of the byte-include macro in a source text | **Pre**: arbitrary source text. **Post**: the set of file-name components of the first string literal found inside each macro call's own argument region, kept only when the name ends in one of the configured font extensions. Each call contributes at most one candidate; a call whose region holds no complete string literal contributes none and does not end the scan. Scanning always resumes immediately after that call's region terminator. The traversal is a single forward pass over the text (NFR4). | task0001 |

## Conventions

- **Test naming**: `<subject>_<scenario>_<expected>`, matching every test already
  in the file (NFR6).
- **Test construction**: pure-layer behaviour is pinned with synthetic text or
  synthetic path values; scan-level behaviour is pinned with a temporary
  directory tree built from the existing dev-dependency. No new test framework,
  no property-testing or benchmarking crate (NFR2).
- **Traceability comments**: added tests keep the file's existing practice of
  naming, in a section comment, the requirement / scenario IDs they pin.
- **Error-handling policy for the analysis layer**: malformed or truncated input
  never panics and never aborts the caller. An unresolvable macro call
  contributes nothing and the scan continues; input that cannot be bounded at
  all ends the scan with whatever was collected so far. Failure is reported only
  through test assertions, whose messages name the drifted set.
- **Self-exclusion invariant**: the scanner's own source file stays excluded from
  the walk. Every fixture text that looks like a real embedding must live inside
  that one file, and the narrowing in this feature must not weaken the
  self-path equality check that provides the exclusion.

## Cross-task Design Decisions

### D1 — Exclusion is split between a path rule and a walk rule

The narrowed path predicate and the untouched directory-name check in the walk
together preserve build-output exclusion (FR1 + NFR3):

1. The walk builds each entry's repository-relative path and asks the path
   predicate first. A path whose *directory* portion contains a build-output
   name is dropped here — this is what keeps files underneath such a directory
   out of scope even if they were reached by another route.
2. A build-output directory encountered as an entry itself now passes the path
   predicate (its name is the final component), and is stopped one step later by
   the directory-name check before the walk descends into it.

Neither step may be removed in favour of the other. Removing (1) lets nested
build-output paths through; removing (2) lets the walk descend into a top-level
build-output tree.

**Affected tasks**: task0001.

### D2 — The argument region is bounded by parenthesis nesting depth

After the fixed opening substring matches, the argument region runs from just
after the opening parenthesis to the parenthesis that returns the nesting depth
to zero, counting both parenthesis characters as they are encountered. The first
string literal *wholly inside* that region is the candidate argument path; a
literal whose opening quote lies inside the region but whose closing quote lies
outside it yields no candidate. Scanning resumes just past the region's closing
parenthesis, so a quote belonging to unrelated code is never reached (FR2, FR5).
When no closing parenthesis exists in the remaining text, the scan ends and
returns what it has collected.

Rationale: depth counting is a single forward pass (NFR4), needs no tokenizer,
and correctly handles balanced parentheses appearing inside the argument. Its
one known limitation — an argument literal containing an *unbalanced*
parenthesis, or an escaped quote — is accepted; no such literal exists in the
scanned tree, and the font-extension filter keeps a mis-bounded pickup from
becoming a false positive unless it happens to end in a font extension.

**Affected tasks**: task0001.

### D3 — Delimiter forms other than parentheses stay unsupported

The scan continues to recognize only the parenthesis call form. Bracket and
brace macro forms, and arguments assembled by a nested macro, are deliberately
not handled: the tree contains none, and adding them would widen the surface
this feature is meant to tighten (FR4). A nested macro call inside the argument
region is never descended into — it simply yields no string literal of its own
at region level and the call contributes nothing.

**Affected tasks**: task0001.

### D4 — Single-task decomposition

The two gaps are implemented as one task rather than two. They share one file,
one set of file-level invariants, and one regression surface (the 7-font
result), and both fixes are small. Two parallel tasks would edit the same
function region and the same test section of the same file with no ordering
between them, producing avoidable conflict for no isolation benefit.

**Affected tasks**: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Boundary detection changes the extraction result on the current tree | Low | High | TS6 pins the 7-entry set against the real tree; TS4 and TS5 pin the two boundary shapes that could move it |
| Narrowing the exclusion lets build-output trees back into the walk | Low | High | D1 keeps both rules; TS2 pins the four build-output paths and TS3 pins the walk-level behaviour of a build-output directory |
| Fixture text added by the new tests is mistaken for a real embedding | Low | High | The self-path exclusion check is untouched and pinned by the existing self-exclusion test; all fixtures stay inside the guard file |
| An argument literal with an unbalanced parenthesis mis-bounds the region | Very low | Low | Accepted limitation (D2); the font-extension filter suppresses a false positive in almost every mis-bounding |
| The change spreads beyond the guard file | Low | Medium | NFR5 confines the change set; the task's file list contains exactly one file and the diff is inspected at verification |

## Open Questions

- [ ] None. Every requirement (FR1–FR6, NFR1–NFR6) is resolved in SPEC.md, and
      the D2 limitation is an accepted scope decision rather than an open item.
