# Test-docs Records

Records under `test-docs/{feature}/taskNNNN.tests.yaml` list the tests that verify each
acceptance criterion. They follow current test names: when a later feature renames a test
that an earlier feature's record lists, the record is updated to the new name. Records are
not frozen as point-in-time snapshots.

## Scope

- Target: the `acceptance_tests[].tests` lists of `test-docs/{feature}/taskNNNN.tests.yaml`
  records.
- Applies to renames that have a successor test.
- Outside the rule: deleting a test without a successor.
- Outside the rule: old-name mentions in a predecessor feature's `feature-docs/` prose
  (SPEC / tasks / VERIFICATION).

## Update duty

A feature that renames a test listed in a predecessor feature's
`test-docs/*/taskNNNN.tests.yaml` updates those `acceptance_tests[].tests` entries to the
current names itself.

## Supersede note

When the rename comes with a behavior change that inverts a predecessor AC's expectation,
add a supersede note to that AC's block as a YAML comment. The note names the superseding
SPEC and its FR IDs. Leave `red_reason` unchanged.

```yaml
  AC-2:
    # Superseded: <old expectation> is replaced by feature-docs/{feature}/SPEC.md FR1/FR3.
    tests:
      - <current test name>
    red_confirmed: true
    red_reason: "<unchanged>"
```

## The renaming feature's own records

The renaming feature's documents that record the rename stay as written:

- its `SPEC.md`
- its `reviews/roundN.yaml`
- its own `test-docs/{feature}/taskNNNN.tests.yaml`

## Resolution check

Each updated name matches at least one test in the project's cargo test listing. Produce the
listing with the `--lib` unit-test command from `core-commands.md` / `core-build-location.md`
(quick-check target directory, manifest path `src-tauri/Cargo.toml`, run from the project
root) plus the libtest `--list` flag:

```bash
CARGO_TARGET_DIR=src-tauri/target cargo test --manifest-path src-tauri/Cargo.toml --lib -- --list
```

Each updated name appears in the output as a test (a `<name>: test` line).
