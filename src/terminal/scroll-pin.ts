/**
 * Pure helper for pinning the viewport when the scrollback grows beneath
 * a user that has scrolled up.
 *
 * See: doc/tasks/pin-viewport-when-scrolled-up/SPEC.md (FR1〜FR6).
 *
 * Given the previously observed scrollback length, the current scrollback
 * length, and the renderer's current scrollOffset, return the adjusted
 * scrollOffset and the new baseline to remember for the next call.
 *
 * Behavior:
 *   - Δ = currSbLen - prevSbLen
 *   - Δ < 0 (scrollback shrank: clear, alt-screen no-op, resize that
 *     shrinks scrollback, active eviction, partial DECSTBM scroll region
 *     without push, etc.):
 *       scrollOffset is clamped to currSbLen so a scrolled-up viewport
 *       can't point past the new top; prevSbLen is re-baselined to
 *       currSbLen. (FR4 reset cases / FR5 / FR6)
 *   - Δ == 0 (no change) or scrollOffset === 0 (follow-the-tail):
 *       scrollOffset is left unchanged, prevSbLen re-baselined.
 *   - Δ > 0 && scrollOffset === 0:
 *       follow-the-tail mode; scrollOffset stays 0 (FR2).
 *   - Δ > 0 && scrollOffset > 0:
 *       scrollOffset += Δ, clamped to currSbLen (FR1 + FR3).
 */
export interface AdjustedScrollOffset {
	nextScrollOffset: number;
	nextPrevSbLen: number;
}

export function computeAdjustedScrollOffset(
	prevSbLen: number,
	currSbLen: number,
	scrollOffset: number,
): AdjustedScrollOffset {
	const delta = currSbLen - prevSbLen;

	if (delta < 0) {
		// Scrollback shrank (clear, resize, or active eviction): clamp the
		// offset so a scrolled-up viewport can't point past the new top.
		return {
			nextScrollOffset: scrollOffset > currSbLen ? currSbLen : scrollOffset,
			nextPrevSbLen: currSbLen,
		};
	}

	if (delta === 0 || scrollOffset === 0) {
		return {
			nextScrollOffset: scrollOffset,
			nextPrevSbLen: currSbLen,
		};
	}

	const grown = scrollOffset + delta;
	const clamped = grown > currSbLen ? currSbLen : grown;
	return {
		nextScrollOffset: clamped,
		nextPrevSbLen: currSbLen,
	};
}
