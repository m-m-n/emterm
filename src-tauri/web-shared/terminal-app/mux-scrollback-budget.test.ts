/**
 * Tests for the cross-pane global scrollback budget enforcer (FR4 / NFR2).
 *
 *  TS-5  Total scrollback over the cap evicts oldest down to the cap.
 *  TS-6  Total at/below the cap performs no eviction.
 *  TS-7  Eviction respects each pane's per-pane minimum.
 *  TS-12 Enforcement runs on a coarse cadence, not per PTY byte.
 */

import { describe, expect, it, mock } from "bun:test";
import {
	ENFORCE_CHECK_INTERVAL_LINES,
	GLOBAL_SCROLLBACK_LINE_BUDGET,
	PER_PANE_SCROLLBACK_MIN,
	ScrollbackBudgetEnforcer,
	type ScrollbackPane,
	planScrollbackEviction,
} from "./mux-scrollback-budget.ts";

/** A fake live pane with an in-memory scrollback length and oldest-first eviction. */
function makePane(initialLength: number): ScrollbackPane & { length: number; evictions: number } {
	const pane = {
		length: initialLength,
		evictions: 0,
		getScrollbackLength() {
			return this.length;
		},
		evictOldestScrollback(targetLen: number) {
			if (this.length <= targetLen) return 0;
			const evicted = this.length - targetLen;
			this.length = targetLen;
			this.evictions += evicted;
			return evicted;
		},
	};
	return pane;
}

describe("planScrollbackEviction", () => {
	it("TS-6: total at/below the cap → no eviction targets", () => {
		expect(planScrollbackEviction([10000, 10000, 10000], 60000, 1000).size).toBe(0);
		// Exactly at cap.
		expect(planScrollbackEviction([30000, 30000], 60000, 1000).size).toBe(0);
	});

	it("TS-5: total over the cap → targets bring total down to the cap", () => {
		const lengths = [40000, 40000]; // total 80000, cap 60000 → must shed 20000
		const plan = planScrollbackEviction(lengths, 60000, 1000);
		expect(plan.size).toBeGreaterThan(0);

		const after = lengths.map((len, i) => plan.get(i) ?? len);
		const total = after.reduce((a, b) => a + b, 0);
		expect(total).toBeLessThanOrEqual(60000);
		// Should not over-evict far below the cap.
		expect(total).toBeGreaterThanOrEqual(60000 - 1);
	});

	it("TS-5: evicts from the largest contributor first", () => {
		// One huge pane, one tiny pane. Cap forces shedding only from the big one.
		const lengths = [100000, 2000]; // total 102000, cap 60000 → shed 42000
		const plan = planScrollbackEviction(lengths, 60000, 1000);
		const after = lengths.map((len, i) => plan.get(i) ?? len);
		expect(after.reduce((a, b) => a + b, 0)).toBeLessThanOrEqual(60000);
		// The small pane is untouched.
		expect(plan.get(1)).toBeUndefined();
	});

	it("TS-7: never evicts a pane below the per-pane minimum", () => {
		// Three panes, tiny cap that would otherwise demand cutting below the min.
		const lengths = [5000, 5000, 5000]; // total 15000
		const plan = planScrollbackEviction(lengths, 3000, 1000); // cap 3000, min 1000
		const after = lengths.map((len, i) => plan.get(i) ?? len);
		for (const len of after) {
			expect(len).toBeGreaterThanOrEqual(1000);
		}
		// With 3 panes pinned at the 1000 minimum, the best achievable total is
		// 3000 — exactly the cap.
		expect(after.reduce((a, b) => a + b, 0)).toBe(3000);
	});

	it("TS-7: leaves total above cap when every pane is already at the minimum", () => {
		const lengths = [1000, 1000, 1000]; // total 3000, all at min
		const plan = planScrollbackEviction(lengths, 1000, 1000); // cap below what's possible
		// Cannot evict any pane below the minimum → no targets.
		expect(plan.size).toBe(0);
	});
});

describe("ScrollbackBudgetEnforcer", () => {
	it("TS-5: enforce evicts oldest across panes to bring total ≤ cap", () => {
		const panes = [makePane(40000), makePane(40000)];
		const enforcer = new ScrollbackBudgetEnforcer(60000, 1000, 512);

		const evicted = enforcer.enforce(panes);
		expect(evicted).toBe(20000);
		const total = panes.reduce((a, p) => a + p.getScrollbackLength(), 0);
		expect(total).toBeLessThanOrEqual(60000);
	});

	it("TS-6: enforce is a no-op when total is at/below cap", () => {
		const panes = [makePane(10000), makePane(10000)];
		const enforcer = new ScrollbackBudgetEnforcer(60000, 1000, 512);
		const evicted = enforcer.enforce(panes);
		expect(evicted).toBe(0);
		expect(panes[0]!.getScrollbackLength()).toBe(10000);
		expect(panes[1]!.getScrollbackLength()).toBe(10000);
	});

	it("TS-7: enforce respects per-pane minimum", () => {
		const panes = [makePane(5000), makePane(5000), makePane(5000)];
		const enforcer = new ScrollbackBudgetEnforcer(3000, 1000, 512);
		enforcer.enforce(panes);
		for (const p of panes) {
			expect(p.getScrollbackLength()).toBeGreaterThanOrEqual(1000);
		}
	});

	it("TS-12: noteScrollbackGrowth does not trigger enforcement until the coarse threshold", () => {
		const enforcer = new ScrollbackBudgetEnforcer(60000, 1000, 512);

		// Simulate per-byte/per-line growth below the threshold — never ready.
		for (let i = 0; i < 511; i++) {
			expect(enforcer.noteScrollbackGrowth(1)).toBe(false);
		}
		// The 512th line crosses the coarse interval.
		expect(enforcer.noteScrollbackGrowth(1)).toBe(true);
		expect(enforcer.shouldEnforce()).toBe(true);
	});

	it("TS-12: enforce resets the growth counter (coarse cadence re-arms)", () => {
		const enforcer = new ScrollbackBudgetEnforcer(60000, 1000, 512);
		enforcer.noteScrollbackGrowth(1000); // over threshold
		expect(enforcer.shouldEnforce()).toBe(true);
		enforcer.enforce([]); // empty panes, but still resets the counter
		expect(enforcer.shouldEnforce()).toBe(false);
		expect(enforcer.noteScrollbackGrowth(1)).toBe(false);
	});

	it("TS-12: enforce is invoked lazily — a heavy byte stream calls evict at most once per interval", () => {
		// Drive 2000 lines of growth through the coarse gate. enforce() should be
		// callable only when the gate opens, not 2000 times.
		const panes = [makePane(70000)];
		const evictSpy = mock(panes[0]!.evictOldestScrollback.bind(panes[0]!));
		panes[0]!.evictOldestScrollback = evictSpy;
		const enforcer = new ScrollbackBudgetEnforcer(60000, 1000, 512);

		let enforceCalls = 0;
		for (let line = 0; line < 2000; line++) {
			if (enforcer.noteScrollbackGrowth(1)) {
				enforcer.enforce(panes);
				enforceCalls++;
			}
		}
		// 2000 lines / 512 interval → at most a handful of enforce passes, NOT 2000.
		expect(enforceCalls).toBeLessThanOrEqual(2000 / 512 + 1);
		expect(enforceCalls).toBeGreaterThanOrEqual(1);
	});
});

describe("budget constants", () => {
	it("exposes sane defaults sized from the ~99MB ceiling rationale", () => {
		expect(GLOBAL_SCROLLBACK_LINE_BUDGET).toBe(60000);
		expect(PER_PANE_SCROLLBACK_MIN).toBe(1000);
		expect(ENFORCE_CHECK_INTERVAL_LINES).toBe(512);
		// Budget must allow more than one full per-pane (10000-line) history.
		expect(GLOBAL_SCROLLBACK_LINE_BUDGET).toBeGreaterThan(10000);
	});
});
