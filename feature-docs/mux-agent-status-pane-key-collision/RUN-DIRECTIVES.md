# Run directives

Operator instructions supplied at `/em-workflow:develop` launch time that
outlive a single `--once` invocation. A later session resuming this feature
MUST read this file before Step C (completion processing).

## Step C completion handling (overrides the batch default)

The batch default for the `develop.completion` gate is "keep branch — no
merge, no push, no PR". The operator overrode it at launch:

- Push `em-workflow/mux-agent-status-pane-key-collision/integration` to the remote.
- Open a pull request against `base_branch` (`main`) with `gh pr create`.
- Keep the integration branch on BOTH the local repo and the remote.
- Do NOT merge into `base_branch` automatically.

This is the "PR を作成" branch of Step C, with the added requirement that
the local branch is kept as well.

## Launch flags

`--batch --once` — one phase per invocation.

## Task source

Notion task: https://www.notion.so/3ce3509ec8ee81459f45de40843433c3
Report progress and the final batch report back as a comment on that page
(per the `notion-batch-develop` skill). Never edit the page body or its
status property.
