# Implementation Plan: tmux Session Rows in the New-Tab Chooser

## Overview

Replace the new-tab chooser's one-row-per-tmux-socket listing with one row per tmux session, obtained by asking each live tmux server for its session names, and attach to the selected session by exact name.

## Technology Stack

- **Language**: Rust (existing `emterm` binary, `gui` feature).
- **New dependencies**: none. Session enumeration uses the standard library's process-spawning facility; the bounded wait is built from standard-library primitives already available to the crate. If a bounded child wait turns out to require a crate, that is a plan deviation to report rather than an unreviewed dependency addition — the project license is MIT and any candidate must be license-compatible per `references/license-compat.md`.

## Layer Structure

The existing three-layer split for this area is preserved:

| Layer | Module | Responsibility | May depend on |
|-------|--------|----------------|---------------|
| Discovery | `src-tauri/src/tmux_sockets.rs` | Knows what tmux is running: live sockets and, new in this feature, the sessions inside them. Owns all tmux-specific knowledge (socket directory resolution, liveness probing, session querying, output parsing, timeouts). | std only |
| Application | `src-tauri/src/app.rs` | Calls Discovery when the chooser opens, hands the result to the UI layer, decodes a confirmed row into a spawn action. | Discovery, UI |
| UI | `src-tauri/src/ui/profile_selector.rs` | Renders rows and decodes row indices into domain choices. Holds no tmux knowledge beyond rendering the label it is handed and reporting which entry was chosen. | — |

Dependency direction is one-way: Application → Discovery and Application → UI. The UI layer never calls Discovery (unchanged rule from the predecessor feature).

## Shared Components

| Component | Responsibility | Contract (pre/postcondition) | Used by tasks |
|-----------|----------------|------------------------------|---------------|
| Tmux entry list (Discovery output) | One ordered list describing every tmux row the chooser should show | **Pre:** none (callable any time). **Post:** returns a possibly-empty ordered list. Each element carries the socket's display name, the socket's absolute path, and an optional session name (present = a real session; absent = the socket could not be enumerated and this is a fallback entry). Ordering is by socket name, then session name, with the fallback entry taking the socket's position. Never returns an error and never panics regardless of the environment. | task0001 |
| Row label rule | Turns one entry into the text shown in the chooser | **Pre:** one entry. **Post:** for an entry with a session on the socket named `default` → `tmux: {session}`; with a session on any other socket → `tmux: {socket}: {session}`; without a session → `tmux: {socket}`. Pure function of the entry, used by both the renderer and any test. | task0001 |
| Attach argument rule | Turns one entry into the PTY spawn arguments | **Pre:** one entry. **Post:** executable is `tmux`; arguments for an entry with a session are the socket flag, the socket path, the attach-session subcommand, the target flag, and the session name prefixed with tmux's exact-match marker; for an entry without a session they are the socket flag, the socket path, and the plain attach subcommand. Every value is a discrete argument — never concatenated into a shell string. | task0001 |

This feature is planned as a single task, so no contract crosses task boundaries. The contracts are pinned here anyway because they are the seams the review and verify phases check against, and because a later follow-up task must not redefine them silently.

## Conventions

- **Naming**: keep the existing module and type naming style of `tmux_sockets.rs`; the socket-discovery type and function names that other code already depends on stay as they are unless the change is required by the new entry shape.
- **Error handling**: Discovery is total — every failure mode (missing binary, spawn error, non-zero exit, unparseable or empty output, timeout) degrades to the fallback entry for that socket. No `Result` is propagated to the Application layer for enumeration, matching the module's existing contract that it "never returns an error to the caller".
- **Logging**: enumeration failures are logged at a level that survives release builds only when they are actionable and rare; per-open, per-socket failures are expected in normal operation (no tmux installed) and must not spam the log on every chooser open. Prefer a level below `warn` for the routine cases.
- **Platform gating**: all new code stays under the existing `#[cfg(unix)]` gating; the non-Unix stub keeps returning an empty list so the Application layer needs no platform branching.
- **No shell**: the tmux binary is invoked with an argument vector. No shell interpreter is introduced anywhere on this path.

## Cross-task Design Decisions

### D1: Enumeration lives in the Discovery module

Session names are tmux knowledge. Putting the query, the output parsing, the timeout, and the fallback decision in `tmux_sockets.rs` keeps the Application layer's role to "call, receive a list, render" and keeps every tmux-specific failure mode testable in one place. Affected: task0001.

### D2: A single entry list drives label, row count, and attach arguments

The chooser previously carried a list of socket-name/path pairs, and the label and attach arguments were each derived separately. Because a row may now be either a session row or a fallback socket row, the three derivations must agree. They are therefore all functions of one entry type held in one list; the row-index decode remains the single authority it already is (`row_to_choice`). Affected: task0001.

### D3: Relaxing the "no subprocess during discovery" rule

The predecessor feature forbade spawning processes during discovery for latency reasons. Session names cannot be read without talking to the tmux server, so that rule is relaxed to: no subprocess for socket discovery (unchanged), one bounded subprocess per already-proven-live socket for session enumeration. The liveness connect probe stays in front of enumeration precisely so dead sockets never cost a process spawn. Affected: task0001.

### D4: Exact-match attach target

Attaching by bare session name lets tmux fall back to prefix and pattern matching, which silently attaches to the wrong session when one name is a prefix of another. The target is therefore always given with tmux's exact-match marker. Affected: task0001.

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| A hung tmux server stalls chooser opening | Low | High (UI freeze) | Bounded wait per socket (≤ 300 ms, NFR1); on expiry the child is terminated and the socket degrades to a fallback entry. The bound must hold even if the child never writes output and never exits. |
| Spawning a child per chooser open adds noticeable latency | Medium | Medium | Enumerate only sockets that already passed the liveness probe; a healthy local query is sub-millisecond. Verified by the manual scenario. |
| Terminating the timed-out child leaks a zombie process | Medium | Medium | The termination path must also reap the child, not just signal it. Called out explicitly in the task's acceptance criteria. |
| Session names with unusual content (spaces, non-ASCII, leading dash) break the label or the argument vector | Low | Medium | Names are carried verbatim as one argument and one label string; the exact-match marker prefix also removes ambiguity for names that look like flags. Covered by edge-case tests. |
| Regression in the existing chooser (row order, keyboard wrap, default preselection) | Medium | High | The row-index decode stays the single authority; existing chooser tests must keep passing unchanged, and NFR3 is verified explicitly. |

## Open Questions

- None. All specification ambiguities were resolved as SPEC.md Assumptions A1–A7.
