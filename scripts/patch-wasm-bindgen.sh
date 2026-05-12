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
if ! grep -q '__wbg_reset' "$WASM_JS"; then
  # Build the reset function and updated export line
  RESET_FN='function __wbg_reset() {
    wasm = undefined;
    cachedDataViewMemory0 = null;
    cachedUint16ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    WASM_VECTOR_LEN = 0;
    heap.length = 128;
    heap.fill(undefined);
    heap.push(undefined, null, true, false);
    heap_next = heap.length;
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
