#!/bin/sh
# Patch wasm-bindgen generated JS/TS to export a reset function.
# This allows reinitializing the WASM module after memory corruption
# by clearing the cached instance so init() creates a fresh one.

set -e

WASM_JS="wasm/pkg/emterm_wasm.js"
WASM_DTS="wasm/pkg/emterm_wasm.d.ts"

if [ ! -f "$WASM_JS" ]; then
  echo "Error: $WASM_JS not found" >&2
  exit 1
fi

# --- Patch JS ---
# Identifiers reset by the injected __wbg_reset body. Each MUST be declared in
# the generated module, otherwise the runtime reset() throws ReferenceError and
# WASM auto-recovery silently breaks. The post-patch guard below enforces this.
# NOTE: `heap` / `heap_next` were removed — current wasm-bindgen no longer emits
# the legacy object table, so resetting them would ReferenceError.
RESET_IDENTIFIERS='wasm cachedDataViewMemory0 cachedUint16ArrayMemory0 cachedUint8ArrayMemory0 WASM_VECTOR_LEN'

if ! grep -q '__wbg_reset' "$WASM_JS"; then
  # Build the reset function and updated export line.
  # Reset only state that the current generated module actually declares.
  RESET_FN='function __wbg_reset() {
    wasm = undefined;
    cachedDataViewMemory0 = null;
    cachedUint16ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    WASM_VECTOR_LEN = 0;
}'
  EXPORT_LINE='export { initSync, __wbg_init as default, __wbg_reset as reset };'

  # wasm-bindgen emits either of two export forms depending on version:
  #   Old (<= 0.2.99): `export { initSync, __wbg_init as default };`
  #   New (>= 0.2.100): `export { initSync };` followed by
  #                     `export default __wbg_init;` on the next line.
  # The awk script handles both shapes.
  awk -v reset="$RESET_FN" -v exportline="$EXPORT_LINE" '
    /^export \{ initSync, __wbg_init as default \};$/ {
      print reset
      print ""
      print exportline
      next
    }
    /^export \{ initSync \};$/ {
      print reset
      print ""
      print exportline
      seen_init = 1
      next
    }
    seen_init && /^export default __wbg_init;$/ {
      # Skip — combined into the merged export line above.
      seen_init = 0
      next
    }
    { print }
  ' "$WASM_JS" > "${WASM_JS}.tmp" && mv "${WASM_JS}.tmp" "$WASM_JS"

  if grep -q '__wbg_reset' "$WASM_JS"; then
    echo "Patched $WASM_JS: added reset() export"
  else
    echo "Error: failed to patch $WASM_JS" >&2
    exit 1
  fi
fi

# --- Post-patch guard (FR2) ---
# Every identifier the injected __wbg_reset writes MUST be declared/defined in
# the generated module. If wasm-bindgen output drifts and drops one, fail the
# build here (non-zero) naming the missing identifier, so recovery never
# silently breaks at runtime again.
guard_failed=0
for ident in $RESET_IDENTIFIERS; do
  # A declaration looks like `let <ident>`, `var <ident>`, `const <ident>`,
  # or appears in a combined declarator list `let a, <ident>;`.
  if grep -Eq "(^|[^A-Za-z0-9_])(let|var|const)[[:space:]]+([A-Za-z0-9_]+[[:space:]]*,[[:space:]]*)*${ident}([[:space:]]*[,;=]|[[:space:]]*$)" "$WASM_JS"; then
    continue
  fi
  echo "Error: __wbg_reset references identifier '${ident}' which is not declared in $WASM_JS" >&2
  echo "       wasm-bindgen output likely drifted; update RESET_FN / RESET_IDENTIFIERS in $0" >&2
  guard_failed=1
done

if [ "$guard_failed" -ne 0 ]; then
  echo "Error: patch guard failed — injected reset references missing identifier(s)" >&2
  exit 1
fi
echo "Patch guard passed: __wbg_reset references only declared identifiers"

# --- Patch d.ts ---
if [ -f "$WASM_DTS" ] && ! grep -q 'export function reset' "$WASM_DTS"; then
  sed -i '/^export default function __wbg_init/i\
/**\
 * Reset the WASM module singleton, allowing re-initialization via init().\
 * Added by scripts/patch-wasm-bindgen.sh for crash recovery.\
 */\
export function reset(): void;\
' "$WASM_DTS"

  if grep -q 'export function reset' "$WASM_DTS"; then
    echo "Patched $WASM_DTS: added reset() declaration"
  else
    echo "Error: failed to patch $WASM_DTS" >&2
    exit 1
  fi
fi
