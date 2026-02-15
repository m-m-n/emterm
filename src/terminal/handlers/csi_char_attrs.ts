/**
 * CSI character attributes (SGR) handler.
 *
 * Handles Select Graphic Rendition sequences.
 */

import type { TerminalStateAccessor } from "./types.ts";
import { parseSgrParams } from "../sgr.ts";
import { applySgrAttr } from "../attributes.ts";

/**
 * Handle SGR (CSI Ps m).
 *
 * Apply Select Graphic Rendition attributes to cursor.
 *
 * @param state - Terminal state accessor
 * @param params - SGR parameter array
 */
export function handleSgr(
  state: TerminalStateAccessor,
  params: number[]
): void {
  const sgrAttrs = parseSgrParams(params);
  for (const sgrAttr of sgrAttrs) {
    applySgrAttr(state.cursor.attrs, sgrAttr);
  }
  state.syncCursorAttrsToWasm();
}
