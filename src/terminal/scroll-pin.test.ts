/**
 * Tests for scroll-pin pure function (FR1〜FR6 / TS-1〜TS-6).
 *
 * scrollOffset > 0 のときの PTY scrollback 増加に対する補正ロジックを
 * 純粋関数として検証する。SPEC.md / VERIFICATION.md と対応:
 *   - TS-1: FR1 (Pin offset on PTY scrollback growth)
 *   - TS-2: FR2 (Follow-tail when offset is zero)
 *   - TS-3: FR3 (Clamp at scrollback top)
 *   - TS-4: reset/clear (scrollbackLength が減少した場合の再ベースライン化)
 *   - TS-5: FR5 (Alt-screen Δ===0 で no-op、退出後の growth で FR1)
 *   - TS-6: Δ===0 ケース (partial DECSTBM scroll region 等の境界)
 */
import { describe, expect, test } from "bun:test";
import { computeAdjustedScrollOffset } from "./scroll-pin.ts";

describe("computeAdjustedScrollOffset", () => {
	test("TS-1: scrollOffset=5, prev=10, curr=13 → nextScrollOffset=8, nextPrevSbLen=13", () => {
		const result = computeAdjustedScrollOffset(10, 13, 5);
		expect(result.nextScrollOffset).toBe(8);
		expect(result.nextPrevSbLen).toBe(13);
	});

	test("TS-2: scrollOffset=0, prev=10, curr=13 → nextScrollOffset=0, nextPrevSbLen=13", () => {
		const result = computeAdjustedScrollOffset(10, 13, 0);
		expect(result.nextScrollOffset).toBe(0);
		expect(result.nextPrevSbLen).toBe(13);
	});

	test("TS-3a: capacity-cap (prev=100, curr=100) → no offset change, prev re-baselines", () => {
		// capacity 上限で curr が増えない場合、Δ===0 なので no-op
		const result = computeAdjustedScrollOffset(100, 100, 95);
		expect(result.nextScrollOffset).toBe(95);
		expect(result.nextPrevSbLen).toBe(100);
	});

	test("TS-3b: scrollOffset + Δ > currSbLen → clamp to currSbLen", () => {
		// scrollOffset=95, prev=100, curr=110 (Δ=10) → 95+10=105 > 110 ではない
		// より明確にクランプを起こすケースを作る:
		// scrollOffset=95, prev=100, curr=98 (curr<prev で reset path に行く) ではダメ
		// scrollOffset=95, prev=90, curr=100 (Δ=10) → 95+10=105 > 100 → clamp to 100
		const result = computeAdjustedScrollOffset(90, 100, 95);
		expect(result.nextScrollOffset).toBe(100);
		expect(result.nextPrevSbLen).toBe(100);
	});

	test("TS-3c: scrollOffset already at scrollbackLength, more growth → still clamp to currSbLen", () => {
		const result = computeAdjustedScrollOffset(100, 110, 100);
		expect(result.nextScrollOffset).toBe(110);
		expect(result.nextPrevSbLen).toBe(110);
	});

	test("TS-4: prev=50, curr=0 (clear), scrollOffset > currSbLen → clamped to 0, prev re-baselines to 0", () => {
		const result = computeAdjustedScrollOffset(50, 0, 7);
		expect(result.nextScrollOffset).toBe(0);
		expect(result.nextPrevSbLen).toBe(0);
	});

	test("TS-4c: active eviction shrinks scrollback, scrollOffset > currSbLen → clamp to currSbLen", () => {
		// Cross-pane budget eviction trims the oldest rows; a scrolled-up
		// offset that now exceeds the new length is clamped to it.
		const result = computeAdjustedScrollOffset(700, 400, 500);
		expect(result.nextScrollOffset).toBe(400);
		expect(result.nextPrevSbLen).toBe(400);
	});

	test("TS-4d: scrollback shrinks but scrollOffset <= currSbLen → unchanged (pin preserved)", () => {
		const result = computeAdjustedScrollOffset(700, 400, 300);
		expect(result.nextScrollOffset).toBe(300);
		expect(result.nextPrevSbLen).toBe(400);
	});

	test("TS-4b: 後続 growth で誤補正なし (clear 直後の次フレーム)", () => {
		// clear 後 prevSbLen=0 に再ベースライン化されている前提で、curr=5 まで growth
		// scrollOffset=0 なので変更なし
		const result = computeAdjustedScrollOffset(0, 5, 0);
		expect(result.nextScrollOffset).toBe(0);
		expect(result.nextPrevSbLen).toBe(5);
	});

	test("TS-5: alt-screen 入退場 (prev === curr) → scrollOffset unchanged", () => {
		// state.getScrollbackLength() は常に primary buffer 値を返すため Δ===0
		const result = computeAdjustedScrollOffset(20, 20, 3);
		expect(result.nextScrollOffset).toBe(3);
		expect(result.nextPrevSbLen).toBe(20);
	});

	test("TS-5b: alt-screen 退出後 primary growth で FR1 が機能", () => {
		const result = computeAdjustedScrollOffset(20, 25, 3);
		expect(result.nextScrollOffset).toBe(8);
		expect(result.nextPrevSbLen).toBe(25);
	});

	test("TS-6: Δ===0 (partial DECSTBM scroll region active 時) → no-op", () => {
		// partial scroll region 時は scrollback に push されないため curr 不変
		const result = computeAdjustedScrollOffset(42, 42, 10);
		expect(result.nextScrollOffset).toBe(10);
		expect(result.nextPrevSbLen).toBe(42);
	});

	test("TS-6b: scrollOffset=0 かつ Δ===0 → 全部不変", () => {
		const result = computeAdjustedScrollOffset(42, 42, 0);
		expect(result.nextScrollOffset).toBe(0);
		expect(result.nextPrevSbLen).toBe(42);
	});
});
