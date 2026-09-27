# Threat Model: mux-probe-scrollback-capacity

## Verdict

threats-identified

## Rationale

**What was inspected.** The analysis covers the feature at the full tier:

- SPEC.md FR1 to FR9 and NFR1 to NFR5.
- The new client-to-daemon capacity message (task0001).
- Its relay through the bridge (task0001).
- The daemon's per-connection intake and the four wrap-aware snapshot sites (task0002).
- The dump-block probe that the value sizes (task0002).

Both tasks declare `input-handling`, so the one boundary where the untrusted value
enters the daemon is analysed deep. task0001 also declares `external-io` and
`api-contract`, and task0002 also declares `concurrency` and `api-contract`.

**The one trust boundary (TB-1).** A client process chooses a number, and the
daemon uses it to size a scratch allocation on every wrap-restore snapshot for that
connection.

**Why the other legs are not separate boundaries.**

- *GUI to bridge (stdin) and bridge to daemon (socket).* These existing transports
  carry the message. The bridge relays the frame verbatim and keeps one latest
  copy. It never interprets or sizes anything by the value, so the bridge files
  (task0001, `external-io`) add no threat beyond TB-1's.
- *GUI sender.* The GUI only emits its own value.

**STRIDE categories not recorded at TB-1.**

- *Spoofing and cross-connection Tampering.* Any peer that completes the handshake
  on the daemon socket already controls the attached session. A report changes only
  that connection's own snapshots, and per-connection scoping (FR4) is a correctness
  requirement rather than the answer to an attacker.
- *Information disclosure.* The value is a display setting and no reply is sent.
- *Repudiation.* There is no audit requirement.
- *Elevation of privilege.* No privilege changes.

## Trust Boundaries

### TB-1: Client-reported scrollback capacity into the daemon's snapshot probe

Crossing: a u32 chosen by a client process crosses from that process into the
daemon. The client can be a GUI tab reporting its setting through the bridge, or any
peer that completes the Hello/Welcome handshake on the daemon socket, including an
older, newer or modified build. The value is stored in the daemon's per-connection
state and sizes the scratch terminal that every wrap-restore snapshot for that
connection builds.

Boundary files: `crates/mux_ipc/src/protocol.rs`, `src-tauri/src/mux/ipc/connection/dispatch.rs`, `src-tauri/src/mux/ipc/connection/mod.rs`, `src-tauri/src/mux/snapshot_bytes.rs`, `src-tauri/src/mux/snapshot_bytes/dump_block.rs`

Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Denial of service | A reported capacity far above any useful depth (up to u32::MAX, or a legitimately very large scrollback setting) makes every wrap-restore snapshot for that connection build its probe terminal at that history depth. This amplifies daemon memory and CPU per snapshot, on the connection task and inside the pane lock that the resume paths hold (NFR2, FR5, FR6) | TM-1 | Probe capacity is resolved as min(reported, 10,000), with unreported mapped to 10,000. The probe enforces the same cap at its own allocation point, whatever value it is passed. Above the cap the dump block is still appended | task0002 AC-2 | VERIFICATION.md Performance / Security Verification: TM-1 |
| Denial of service | A capacity frame whose payload is not exactly one u32 (empty, truncated or oversized, from a mixed-version or faulty peer) causes one of three failures: a panic, a read or allocation driven by the payload length, or an error that ends the connection loop and cuts the client off. It could also overwrite the stored value with garbage (FR4, NFR1, NFR2) | TM-2 | The payload decoder accepts exactly 4 bytes and reports anything else as absent, without panicking and without allocating from the payload. The daemon ignores an absent decode: the connection's current value stays unchanged, no reply is sent and the connection stays open | task0001 AC-2; task0002 AC-5 | VERIFICATION.md Performance / Security Verification: TM-2 |
