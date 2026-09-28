# Threat Model: mux-bridge-capacity-capture-validation

## Verdict
threats-identified

## Rationale
Inspected SPEC.md (FR1-FR3, NFR1-NFR2) and the mux bridge's stdin-to-daemon
forwarding path, its retained reconnect state, and its upgrade-driven
reconnect resend. Tier: reduced. task0001 declares `input-handling`, which
sets TB-1 to deep depth.

One trust boundary is in scope: frames the bridge parses from its standard
input, which carries bytes from outside the bridge process (the terminal
client, and anything else able to write an escape sequence into that input
stream). The bridge-to-daemon socket is not modeled as a separate boundary
for this feature: the daemon's handling of length-invalid capacity payloads
is unchanged (NFR1).

At TB-1 only Tampering realistically applies. The retained value is a
scrollback line count that carries no secret, identity or privilege, so no
other STRIDE category applies. task0001's other declared file, the bridge
test module, is test code and implements no boundary.

## Trust Boundaries

### TB-1: Bridge standard input to retained reconnect capacity
Crossing: ClientScrollbackCapacity frames parsed from the bridge's standard
input (another process: the terminal client, or any writer able to inject an
escape sequence into that input) cross into the bridge's retained reconnect
state, which is replayed to a new daemon connection after an upgrade-driven
reconnect.
Boundary files: src-tauri/src/mux/bridge/mod.rs, src-tauri/src/mux/bridge/forward.rs
Depth: deep (input-handling)

| STRIDE category | Threat | Mitigation ID | Mitigation | Implemented by | Verified by |
|---|---|---|---|---|---|
| Tampering | A length-invalid ClientScrollbackCapacity frame on standard input replaces the retained last valid capacity; after an upgrade-driven reconnect only that invalid body is replayed, the daemon ignores it, and the new connection falls back to the default capacity (FR1; SPEC.md Objectives) | TM-1 | The bridge retains a capacity frame body only when its payload is accepted by the protocol crate's capacity payload decoder (exactly 4 bytes) and otherwise keeps the previously retained value; forwarding of every frame stays unchanged (FR1, FR2) | task0001 AC-1, AC-5 | VERIFICATION.md Performance / Security Verification item TM-1 (TS-1, TS-2) |
