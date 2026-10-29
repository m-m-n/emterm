# Threat Model: inherited-pty-drop-fd-tiocgptn

## Verdict
no-trust-boundary

## Rationale
Inspected REQUIREMENTS.md, SPEC.md and the two files the feature changes
(`src-tauri/src/mux/inherited_pty.rs`, `src-tauri/src/mux/upgrade/tests.rs`);
tier `full`, design step skipped. The change is confined to the test-only
section of one module and one doc comment (NFR1): it is compiled only into the
test binary, and the production adapter and every shipped binary are
unchanged. The tests act only on descriptors the test process creates and
releases itself (PTY pairs, duplicates of their masters, a slave handle, a
temporary file) plus the reserved invalid number `-1`, and only query them
through the kernel. No user, file, network or external-service input enters,
no data reaches another party, no untrusted text is interpolated anywhere, and
no privilege changes. The only task, task0001, declares the domain
`concurrency` and none of `auth` / `input-handling` / `external-io` /
`data-persistence`; the PTY devices and the temporary file it opens are
test-local, process-owned side effects and form no boundary. With no trust
boundary crossed, no STRIDE category applies.
