/**
 * Cross-pane global scrollback budget enforcer (FR4).
 *
 * The per-core scrollback limit (10000 lines, in `wasm/src/ring_buffer.rs`)
 * bounds a single pane, but nothing bounds the SUM across many panes. Under
 * heavy multi-pane output the aggregate scrollback can drive the single shared
 * WASM heap toward the observed ~99MB `memory.grow` ceiling, causing
 * `RuntimeError: Out of bounds memory access`.
 *
 * This module caps the TOTAL scrollback (in lines) across all live panes. When
 * the total exceeds `GLOBAL_SCROLLBACK_LINE_BUDGET`, oldest scrollback is
 * evicted from the largest contributors first, never reducing any single pane
 * below `PER_PANE_SCROLLBACK_MIN`.
 *
 * Metric rationale (see IMPLEMENTATION.md FR4 resolution): line count is the
 * only cheap, per-core-attributable, deterministic metric. `getWasmMemoryBytes`
 * is module-wide (shared by every core in the single WASM instance) and cannot
 * attribute bytes to one pane.
 *
 * Performance (NFR2 / TS-12): enforcement is NOT run per PTY byte. The hot path
 * only feeds a growth counter via `noteScrollbackGrowth`; the actual aggregate
 * scan + eviction runs only when the counter crosses
 * `ENFORCE_CHECK_INTERVAL_LINES`.
 */

/** Total scrollback lines allowed across all live panes before eviction. */
export const GLOBAL_SCROLLBACK_LINE_BUDGET = 60000;

/** Eviction never reduces a single pane below this many scrollback lines. */
export const PER_PANE_SCROLLBACK_MIN = 1000;

/** Newly-added scrollback lines that must accumulate before a coarse check. */
export const ENFORCE_CHECK_INTERVAL_LINES = 512;

/** A live pane the enforcer can measure and evict from. */
export interface ScrollbackPane {
	/** Current scrollback length in lines. */
	getScrollbackLength(): number;
	/** Drop oldest scrollback rows down to `targetLen`. Returns rows evicted. */
	evictOldestScrollback(targetLen: number): number;
}

/**
 * Pure planner: given each pane's current scrollback length, decide a target
 * length per pane so the total is ≤ `budget`, evicting from the largest
 * contributors first and never below `perPaneMin`.
 *
 * Returns a target length for every pane that must shrink (omits panes that
 * stay unchanged). If the budget cannot be met without violating the per-pane
 * minimum (every pane already at/below the minimum), panes are left at the
 * minimum and the total may legitimately remain above budget.
 */
export function planScrollbackEviction(
	lengths: number[],
	budget: number = GLOBAL_SCROLLBACK_LINE_BUDGET,
	perPaneMin: number = PER_PANE_SCROLLBACK_MIN,
): Map<number, number> {
	const targets = new Map<number, number>();
	const total = lengths.reduce((a, b) => a + b, 0);
	let over = total - budget;
	if (over <= 0) return targets;

	// Work on a mutable copy of (index, length), largest first. Repeatedly trim
	// the current largest pane down toward the next-largest (or its minimum),
	// which spreads eviction across the biggest contributors.
	const working = lengths.map((len, index) => ({ index, len }));

	while (over > 0) {
		// Largest contributor that is still above the per-pane minimum.
		working.sort((a, b) => b.len - a.len);
		const top = working.find((p) => p.len > perPaneMin);
		if (!top) break; // every pane at/below minimum — cannot evict further.

		// How far we may cut this pane: down to the larger of (next pane length,
		// per-pane minimum). Cutting to the next-largest keeps eviction balanced;
		// if it is the sole large pane we cut straight toward the minimum.
		const secondLen = working
			.filter((p) => p.index !== top.index)
			.reduce((max, p) => Math.max(max, p.len), 0);
		const floor = Math.max(perPaneMin, secondLen);
		const maxCut = top.len - floor;
		const cut = Math.min(over, maxCut > 0 ? maxCut : top.len - perPaneMin);
		if (cut <= 0) break;

		top.len -= cut;
		over -= cut;
		targets.set(top.index, top.len);
	}

	return targets;
}

/**
 * Stateful enforcer wrapping the pure planner with a coarse cadence so the PTY
 * hot path never triggers a full scan per byte.
 */
export class ScrollbackBudgetEnforcer {
	private pendingGrowth = 0;

	constructor(
		private readonly budget: number = GLOBAL_SCROLLBACK_LINE_BUDGET,
		private readonly perPaneMin: number = PER_PANE_SCROLLBACK_MIN,
		private readonly checkInterval: number = ENFORCE_CHECK_INTERVAL_LINES,
	) {}

	/**
	 * Hot-path hook: record that `lines` new scrollback lines were produced.
	 * Returns true when enough growth has accumulated to warrant a coarse check
	 * (the caller should then invoke `enforce`). Cheap: an add + compare only.
	 */
	noteScrollbackGrowth(lines: number): boolean {
		if (lines > 0) this.pendingGrowth += lines;
		return this.pendingGrowth >= this.checkInterval;
	}

	/** True if accumulated growth has reached the coarse-check threshold. */
	shouldEnforce(): boolean {
		return this.pendingGrowth >= this.checkInterval;
	}

	/**
	 * Aggregate scrollback across `panes` and evict oldest from the largest
	 * contributors until the total is ≤ budget (respecting the per-pane
	 * minimum). Resets the growth counter. Returns the total lines evicted.
	 */
	enforce(panes: ScrollbackPane[]): number {
		this.pendingGrowth = 0;
		if (panes.length === 0) return 0;

		const lengths = panes.map((p) => p.getScrollbackLength());
		const plan = planScrollbackEviction(lengths, this.budget, this.perPaneMin);
		if (plan.size === 0) return 0;

		let evicted = 0;
		for (const [index, targetLen] of plan) {
			const pane = panes[index];
			if (pane) evicted += pane.evictOldestScrollback(targetLen);
		}
		return evicted;
	}
}
