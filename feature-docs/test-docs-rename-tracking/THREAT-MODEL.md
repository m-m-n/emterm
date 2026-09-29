# Threat Model: test-docs-rename-tracking

## Verdict
no-trust-boundary

## Rationale
Inspected: SPEC.md (FR1–FR6, NFR1–NFR3; its Security Considerations section is empty),
the two predecessor test-docs records the feature edits
(`test-docs/notification-summary-markup-escape/task0001.tests.yaml`,
`test-docs/notification-body-markup-escape/task0001.tests.yaml`), the current test names
in `src-tauri/src/callbacks/tests.rs` (read-only name source), and the planned rule file
`.claude/rules/test-docs-records.md`. Tier: reduced. Both tasks declare only the
`config-infra` domain; none of `auth`, `input-handling`, `external-io` or
`data-persistence` is declared, so no boundary needed deep analysis and the
post-decomposition consistency check has nothing to reconcile.

The feature changes project-authored text in project-owned files: test identifiers taken
from the project's own test source, YAML comment lines, and one Markdown rule. Their
consumers — the verify phase using the records' test paths as `cargo test` filters, and
Claude Code sessions loading `.claude/rules/` — read content of the same trust as the rest
of the repository, reviewed and merged through the same workflow. No user input, external
service, network call, inter-process channel, file from outside the project, or privilege
change is introduced or altered, so no trust boundary is crossed.
