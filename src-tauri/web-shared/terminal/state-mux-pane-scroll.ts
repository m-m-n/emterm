/**
 * Per-pane scroll position save/restore helpers for mux.
 *
 * mux は 1 つの CanvasRenderer を全 pane で共有するため、renderer が保持する
 * スクロール位置 (scrollOffset) と scroll-pin ベースライン
 * (prevScrollbackLength) は、pane 切替時に save/restore しないと前 pane の値が
 * 次 pane に持ち越される。通常タブは各タブが独立 renderer を持つため影響しない。
 *
 * これらの helper は MuxPaneGridState スナップショットと renderer の間で
 * スクロール位置を退避・復元する純粋ロジック。renderer の最小インターフェース
 * (ScrollStateTarget) のみに依存するため WASM/PTY スタック無しで検証できる。
 *
 * alternate screen の pane は scrollback を持たず scroll-pin は no-op のため、
 * primary/alternate を区別せず退避時点の renderer の値をそのまま保存・復元する。
 */

/**
 * Scroll-related fields held in a MuxPaneGridState snapshot.
 */
export interface ScrollStateSnapshot {
  /** Number of lines scrolled back from the bottom (0 = at present). */
  scrollOffset: number;
  /** scroll-pin baseline (renderer's last observed scrollback length). */
  scrollPinBaseline: number;
}

/**
 * Minimal renderer surface the scroll helpers depend on. Both CanvasRenderer
 * and test fakes satisfy this.
 */
export interface ScrollStateTarget {
  getScrollOffset(): number;
  setScrollOffset(offset: number): void;
  getScrollPinBaseline(): number;
  setScrollPinBaseline(baseline: number): void;
}

/**
 * Apply the snapshot's saved scroll position to the renderer (pane restore).
 */
export function applyScrollState(
  snapshot: ScrollStateSnapshot,
  renderer: ScrollStateTarget,
): void {
  renderer.setScrollOffset(snapshot.scrollOffset);
  renderer.setScrollPinBaseline(snapshot.scrollPinBaseline);
}

/**
 * Reset the renderer's scroll position to the bottom (fresh / unsaved pane).
 */
export function resetScrollState(renderer: ScrollStateTarget): void {
  renderer.setScrollOffset(0);
  renderer.setScrollPinBaseline(0);
}
