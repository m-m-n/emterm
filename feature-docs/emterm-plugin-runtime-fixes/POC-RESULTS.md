# M-1 live check results

**Executed**: 2026-07-25, in an eMterm tab running Claude Code 2.1.220 with the plugin installed
from the merged `main` (`emterm@emterm-plugins` 0.1.0, hooks 16 → 21 on `/reload-plugins`).
**Theme in effect**: `ui_theme: dark`, `ui_theme_preset: pink`, so the badge colours were
`working` = `#FFB1C8`, `idle` = `#D4BFC5`, `blocked` = `#F9DEDC`.

## Outcome

The check found a design error that four static review rounds did not: **`blocked` was wired to
the wrong event**. It was fixed on branch `fix/emterm-plugin-permission-request`.

## Steps

| Step | Observed | Verdict |
| --- | --- | --- |
| 1. Prompt sent → `working` | Badge present and filled | Consistent, but see the note on colour below |
| 2. Turn complete → `idle` | Badge present and filled | Same |
| 3. Permission dialog → `blocked` | **Badge stayed filled `working`. No ring, no colour change.** | **FAIL** |
| 4a. Approve → succeed → `working` | Not isolated (badge was already `working`) | Inconclusive |
| 4b. Approve → fail → `working` | A non-zero-exit tool ran; badge remained `working` | Consistent, not decisive |
| 4c. Deny → stays `blocked` | Premise never held — the badge was never `blocked` | Not reachable |
| 5. API-error turn end → stays `working` | Not exercised | Not run |

## Step 3: the finding

With a permission dialog on screen, the badge showed a filled dot in the `working` colour. The
expected `blocked` rendering is a **ring** (`agent_badge_filled` in `src-tauri/src/ui/tab_bar.rs`
renders Blocked as a ring once seen), so the shape alone settles it: `blocked` never arrived.

Cause: `blocked` was carried by `Notification` with a matcher including `permission_prompt`. The
hooks documentation's lifecycle description lists `Notification` among the **"standalone async
events"**, and Claude Code raises those as OS-level notifications — they fire when the user is
*not* watching the terminal. The event defined as "**Runs when the user is shown a permission
dialog**" is `PermissionRequest`, which sits inside the agentic loop next to `PreToolUse` and
`PostToolUse`.

So on a dialog the user is actually looking at — the normal case, and the entire point of the
feature — nothing fired.

### Why the reviews missed it

Rounds 1-4 examined this matcher repeatedly and productively: round 1 found it unmatched and
added one, round 2 anchored it, round 4 found the type list covered seven of eight documented
types and added `agent_completed`. Every one of those was a correct finding about the *value*
inside the construct. None asked whether `Notification` was the right *construct*.

That is the generalisable lesson: reviewing a value inside a construct never tests whether the
construct was the right choice, and static review cannot distinguish "this matcher is wrong" from
"this event never fires here". Only a live run separates them.

It is also the same class of error as the `/dev/tty` transport this whole feature existed to
undo — a platform behaviour taken from a plausible reading of the documentation and built on
without a live check. The pre-spec POC verified the *transport*; it did not verify the *event
selection*, and the event selection was equally load-bearing.

## Fix applied

```
PermissionRequest  (no matcher)                            -> blocked   [added]
Notification       ^(elicitation_dialog|agent_needs_input)$ -> blocked   [permission_prompt removed]
```

`Notification` is kept for the cases where Claude Code genuinely raises an OS notification instead
of a dialog. `permission_prompt` is dropped from it because `PermissionRequest` already covers that
wait and would otherwise double-report.

`PermissionDenied` was considered for step 4c and rejected: the documentation states it "only fires
in auto mode: it doesn't run when you manually deny a permission dialog". The denied-dialog gap
therefore stands as a known limitation.

## Re-run against the fixed wiring

The plugin was reinstalled (see the propagation note below) and step 3 was repeated with the same
command that failed before, `hostnamectl status`.

| Step | Observed | Verdict |
| --- | --- | --- |
| 3. Permission dialog → `blocked` | **Badge became a ring.** Same command, same dialog, and before the fix it stayed a filled dot. | **PASS** |
| 4a. Approve → succeed → `working` | The call was approved and exited 0; the badge left the ring state | **PASS** |

The shape change is what makes this decisive: `agent_badge_filled` renders Working and Idle as
filled in every case, so a ring can only be Blocked or Done. Nothing else in the wiring emits
`done`.

`PermissionRequest` was therefore the correct event, and the original `Notification(permission_prompt)`
choice was the whole of the defect.

## Still outstanding
- **Step 5** (API-error turn ending) has never been exercised. If the badge does reach `idle`
  there, the documentation's claim that `StopFailure` output is ignored would be wrong for
  `terminalSequence`, and the removed `StopFailure` entry should be reinstated on that evidence.
- **Steps 1 and 2 remain unconfirmed as a pair.** Both states render as a filled dot and, under
  the pink dark theme, in closely related hues (`#FFB1C8` vs `#D4BFC5`). Choosing a discriminator
  that differs only in colour was a poor test design on the verification side; the `blocked` ring
  is the only shape-distinguished state and is what made step 3 conclusive.

## Propagation note — a pinned version does not update in place

Reinstalling to pick up the fix was not straightforward, and the reason is worth recording.
`/plugin marketplace update` refreshed the catalogue, but `/plugin install` answered "already
installed" and copied nothing: the plugin cache is keyed by version, and `version` was still
`0.1.0`. The documentation is explicit — "If set, the plugin is pinned to this string and users
only receive updates when it changes." The cached `hooks.json` kept its original mtime and the old
matcher, so a reload appeared to succeed while running the unfixed wiring.

`/plugin uninstall` followed by `/plugin install` forced the re-copy, after which the cached file
matched the branch.

Pinning `0.1.0` across the whole feature was right while the plugin was unpublished, but it stops
being right the moment anyone has it installed — including us. Any behavioural change from here
needs either a version bump or an explicit uninstall/reinstall, and only the first is something a
user would discover on their own.

## Environment note

`permissions.defaultMode: "auto"` is set in `~/.claude/settings.json`. Under auto mode the
classifier resolves most tool calls without a dialog, so `permission_prompt` and `PermissionRequest`
both fire rarely. Reaching step 3 required manual mode plus a command matching no allow rule
(`hostnamectl status`). Worth knowing when judging how often `blocked` appears in practice.
