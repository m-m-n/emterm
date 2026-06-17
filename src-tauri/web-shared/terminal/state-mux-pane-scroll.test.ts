/**
 * Tests for per-pane scroll position save/restore helpers (FR1 / FR2).
 *
 * mux は単一 CanvasRenderer を全 pane で共有するため、pane 切替時に renderer の
 * スクロール位置 (scrollOffset) と scroll-pin ベースライン (prevScrollbackLength)
 * を pane ごとに退避・復元しないと前 pane の値が次 pane に持ち越される。
 *
 * 退避は saveMuxPaneState(純粋関数) が ScrollStateTarget から直接スナップショットへ
 * 取り込む。復元/リセットは applyScrollState / resetScrollState helper が担う。
 * これらは renderer の最小インターフェース (getScrollOffset / setScrollOffset /
 * getScrollPinBaseline / setScrollPinBaseline) のみに依存する純粋ロジックなので、
 * WASM/PTY スタック無しで検証できる。
 *
 * VERIFICATION.md TS-1〜TS-6 と対応:
 *   - TS-1: saveMuxPaneState がスナップショットに現在位置を取り込む
 *   - TS-2: applyScrollState がスナップショットの値を renderer へ反映
 *   - TS-3: resetScrollState (新規/未保存 pane) は 0 に初期化
 *   - TS-4: scroll-pin ベースラインも pane ごとに退避・復元される
 *   - TS-5: 往復 A→B→A で A の値が復元される
 */
import { describe, expect, test } from "bun:test";
import {
  applyScrollState,
  resetScrollState,
  type ScrollStateTarget,
} from "./state-mux-pane-scroll.ts";
import {
  saveMuxPaneState,
  type MuxSaveContext,
} from "./state-mux-pane.ts";
import type { WasmGrid } from "./wasm/terminal-core.ts";

/** Minimal renderer fake exposing the scroll API the helpers depend on. */
function makeRenderer(offset = 0, baseline = 0): ScrollStateTarget & {
  offset: number;
  baseline: number;
} {
  return {
    offset,
    baseline,
    getScrollOffset(): number {
      return this.offset;
    },
    setScrollOffset(o: number): void {
      this.offset = o;
    },
    getScrollPinBaseline(): number {
      return this.baseline;
    },
    setScrollPinBaseline(b: number): void {
      this.baseline = b;
    },
  };
}

/**
 * Minimal MuxSaveContext for testing snapshot construction. The grids are only
 * stored as references in the snapshot (not dereferenced by saveMuxPaneState),
 * so dummy placeholders are sufficient.
 */
function makeSaveContext(): MuxSaveContext {
  const dummyGrid = {} as unknown as WasmGrid;
  return {
    primaryWasmGrid: dummyGrid,
    alternateWasmGrid: null,
    useAlternate: false,
    title: "",
    iconName: "",
    modes: {
      mouseTracking: "none",
      mouseEncoding: "default",
      cursorKeys: "normal",
    } as MuxSaveContext["modes"],
  };
}

describe("saveMuxPaneState scroll capture", () => {
  test("TS-1: scrollTarget を渡すと退避時の renderer のスクロール位置がスナップショットに取り込まれる", () => {
    const renderer = makeRenderer(7, 0);
    const snapshot = saveMuxPaneState(makeSaveContext(), renderer);
    expect(snapshot.scrollOffset).toBe(7);
  });

  test("TS-4: scroll-pin ベースラインもスナップショットに取り込まれる", () => {
    const renderer = makeRenderer(7, 42);
    const snapshot = saveMuxPaneState(makeSaveContext(), renderer);
    expect(snapshot.scrollOffset).toBe(7);
    expect(snapshot.scrollPinBaseline).toBe(42);
  });

  test("TS-1: スクロールしていない pane (offset=0) も 0 として取り込まれる", () => {
    const renderer = makeRenderer(0, 13);
    const snapshot = saveMuxPaneState(makeSaveContext(), renderer);
    expect(snapshot.scrollOffset).toBe(0);
    expect(snapshot.scrollPinBaseline).toBe(13);
  });

  test("scrollTarget 省略時は scrollOffset / scrollPinBaseline が 0 で初期化される", () => {
    const snapshot = saveMuxPaneState(makeSaveContext());
    expect(snapshot.scrollOffset).toBe(0);
    expect(snapshot.scrollPinBaseline).toBe(0);
  });
});

describe("applyScrollState", () => {
  test("TS-2: スナップショットのスクロール位置が renderer に反映される", () => {
    const renderer = makeRenderer(0, 0);
    applyScrollState({ scrollOffset: 9, scrollPinBaseline: 0 }, renderer);
    expect(renderer.offset).toBe(9);
  });

  test("TS-4: scroll-pin ベースラインも renderer に反映される", () => {
    const renderer = makeRenderer(0, 0);
    applyScrollState({ scrollOffset: 9, scrollPinBaseline: 55 }, renderer);
    expect(renderer.offset).toBe(9);
    expect(renderer.baseline).toBe(55);
  });

  test("前 pane の renderer 値を上書きする (持ち越さない)", () => {
    const renderer = makeRenderer(100, 200);
    applyScrollState({ scrollOffset: 3, scrollPinBaseline: 4 }, renderer);
    expect(renderer.offset).toBe(3);
    expect(renderer.baseline).toBe(4);
  });
});

describe("resetScrollState", () => {
  test("TS-3: 新規/未保存 pane は scrollOffset=0 に初期化される", () => {
    const renderer = makeRenderer(50, 0);
    resetScrollState(renderer);
    expect(renderer.offset).toBe(0);
  });

  test("TS-3/TS-4: scroll-pin ベースラインも 0 にリセットされる", () => {
    const renderer = makeRenderer(50, 99);
    resetScrollState(renderer);
    expect(renderer.offset).toBe(0);
    expect(renderer.baseline).toBe(0);
  });
});

describe("normal-tab independence (TS-6)", () => {
  test("各タブが独立 renderer を持つ場合、片方の save/restore が他方に波及しない", () => {
    // 通常タブは各タブが独立 renderer を持つ。mux の helper を片方に適用しても
    // もう一方の renderer のスクロール位置・ベースラインは変化しない。
    const tab1 = makeRenderer(15, 40);
    const tab2 = makeRenderer(3, 9);

    const snap1 = saveMuxPaneState(makeSaveContext(), tab1);
    resetScrollState(tab1);
    applyScrollState(snap1, tab1);

    // tab2 は一切触れていないので保持される
    expect(tab2.offset).toBe(3);
    expect(tab2.baseline).toBe(9);
    // tab1 は往復で元に戻る
    expect(tab1.offset).toBe(15);
    expect(tab1.baseline).toBe(40);
  });
});

describe("round trip A→B→A (TS-5)", () => {
  test("A をスクロールアップ → B に切替 → A に戻ると A の位置が復元される", () => {
    // 共有 renderer を 1 つだけ用意 (mux の構造を模す)
    const renderer = makeRenderer(0, 0);

    // A をスクロールアップ
    renderer.setScrollOffset(12);
    renderer.setScrollPinBaseline(30);

    // A → B: A を退避し、B (未保存) を 0 で初期化
    const snapA = saveMuxPaneState(makeSaveContext(), renderer);
    resetScrollState(renderer);
    expect(renderer.offset).toBe(0);
    expect(renderer.baseline).toBe(0);

    // B で少しスクロール
    renderer.setScrollOffset(5);
    renderer.setScrollPinBaseline(8);

    // B → A: B を退避し、A を復元
    const snapB = saveMuxPaneState(makeSaveContext(), renderer);
    applyScrollState(snapA, renderer);

    expect(renderer.offset).toBe(12);
    expect(renderer.baseline).toBe(30);
    // B のスナップショットは独立して保持されている
    expect(snapB.scrollOffset).toBe(5);
    expect(snapB.scrollPinBaseline).toBe(8);
  });
});
