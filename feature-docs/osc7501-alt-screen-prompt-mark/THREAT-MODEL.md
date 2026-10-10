# Threat Model: osc7501-alt-screen-prompt-mark

## Verdict
no-applicable-threat

## Rationale
Inspected SPEC.md and REQUIREMENTS.md (FR1–FR3, NFR1–NFR2, A1–A4) at the
`full` tier. One trust boundary exists: bytes written by any program in a plain
tab (untrusted, possibly remote over SSH) reach `term_core`'s OSC dispatch, the
host's ordered OSC 7501 feed and the tab's record table
(`crates/term_core/src/callbacks.rs`, `crates/term_core/src/osc_handler.rs`,
`src-tauri/src/callbacks.rs`, `src-tauri/src/tabs/output_pipeline.rs`). Depth
is deep (input-handling, declared by task0001). The change only decides
whether an already parsed OSC 133 candidate enters the 7501 feed, from the
screen mode read at dispatch. No STRIDE category applies. Tampering and
spoofing: the program writing the bytes controls the screen mode and can
already set or remove any record directly with OSC 7501 reports and RIS, so
the classification grants it no capability it lacks today. Denial of service:
one constant-time mode read per dispatch, the feed only shrinks, and its
application stays linear (NFR1). Information disclosure: no new output; the
query response is unchanged (NFR2). Repudiation and elevation of privilege:
no identity or privilege is involved. The mitigations recorded by the
osc7501-program-status feature for this boundary are unaffected.
Post-decomposition re-check: task0001 is the only task declaring one of the
four depth domains (input-handling); its boundary files are the ones named
above, and no Boundary files line exists because the short form records no
threat.
