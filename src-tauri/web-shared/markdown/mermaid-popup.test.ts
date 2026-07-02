/**
 * Tests for the mermaid fullscreen popup.
 *
 * Covers TS-5 through TS-15 from doc/tasks/mermaid-zoom-popup/VERIFICATION.md:
 * - overlay + SVG-clone construction and ARIA
 * - background scroll lock save/restore
 * - focus flow (open → close btn; close → trigger)
 * - zoom clamp lower / upper bounds
 * - reset (`0` key) restores {scale:1, panX:0, panY:0}
 * - ESC keydown closes popup and stops propagation
 * - background click closes; child click does not
 * - wheel deltaY<0 zooms and preventDefault fires
 * - drag mousedown → mousemove → mouseup updates pan
 * - window resize while open recomputes fit factor k
 *
 * Plus TS-16 through TS-20 (sdd.6 FAIL fixes):
 * - button/keyboard zoom step is 0.25 additive; wheel stays ×1.1
 * - Tab / Shift+Tab focus trap cycles the four popup buttons
 * - pan-end click on background does not close; a clean click does
 * - clone sizing normalization sets explicit width/height attrs + max:none
 * - window blur while dragging clears the drag state
 *
 * Plus TS-21 / TS-22 (phase-4 real-device fixes):
 * - arrow keys pan by 40px with scroll-direction semantics + preventDefault
 * - open/close post the reserved esc-guard IPC (and no-op without window.ipc)
 */

import { afterEach, describe, expect, test } from "bun:test";

import {
  __closeActivePopupForTest,
  __readActivePopupState,
  openMermaidPopup,
} from "./mermaid-popup.ts";

const SVG_NS = "http://www.w3.org/2000/svg";

/** Build a source SVG element with a known viewBox and attach it to the DOM. */
function makeSvg(viewBox = "0 0 100 80"): SVGElement {
  const svg = document.createElementNS(SVG_NS, "svg") as SVGElement;
  svg.setAttribute("viewBox", viewBox);
  const rect = document.createElementNS(SVG_NS, "rect");
  rect.setAttribute("x", "0");
  rect.setAttribute("y", "0");
  rect.setAttribute("width", "100");
  rect.setAttribute("height", "80");
  svg.appendChild(rect);
  document.body.appendChild(svg);
  return svg;
}

/** Build a trigger button element (already appended to body) for the popup. */
function makeTrigger(): HTMLButtonElement {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "spread-trigger";
  document.body.appendChild(btn);
  return btn;
}

/** Force window.innerWidth / innerHeight to a known value for fit math. */
function setWindowSize(w: number, h: number): void {
  Object.defineProperty(window, "innerWidth", {
    value: w,
    configurable: true,
  });
  Object.defineProperty(window, "innerHeight", {
    value: h,
    configurable: true,
  });
}

afterEach(() => {
  // Force-close any popup that survived (previous test may have thrown
  // before reaching its own controller.close()). This also clears the
  // module-level singleton guard so the next test starts fresh.
  __closeActivePopupForTest();
  document.body.innerHTML = "";
  document.body.style.overflow = "";
});

describe("openMermaidPopup (TS-5 through TS-15)", () => {
  test("TS-5: appends one overlay to document.body with a cloned SVG inside .mermaid-popup-stage and correct ARIA", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const overlays = document.body.querySelectorAll(".mermaid-popup-overlay");
    expect(overlays.length).toBe(1);
    const overlay = overlays[0] as HTMLElement;
    expect(overlay.getAttribute("role")).toBe("dialog");
    expect(overlay.getAttribute("aria-modal")).toBe("true");

    const stage = overlay.querySelector(".mermaid-popup-stage");
    expect(stage).not.toBeNull();
    const cloned = stage?.querySelector("svg");
    expect(cloned).not.toBeNull();
    // Clone, not the same node
    expect(cloned).not.toBe(svg);
    // Buttons should carry aria-labels for accessibility (NFR5).
    const closeBtn = overlay.querySelector<HTMLElement>(".mermaid-popup-close");
    expect(closeBtn?.getAttribute("aria-label")).toBe("Close popup");

    controller.close();
  });

  test("TS-6: sets document.body.style.overflow to 'hidden' on open and restores on close", () => {
    document.body.style.overflow = "auto";
    const svg = makeSvg();
    const trigger = makeTrigger();

    const controller = openMermaidPopup({ svg, triggerButton: trigger });
    expect(document.body.style.overflow).toBe("hidden");

    controller.close();
    expect(document.body.style.overflow).toBe("auto");
  });

  test("TS-7: focus moves to close button on open and back to trigger on close", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    // Give trigger the initial focus so we can prove it returns.
    trigger.focus();

    const controller = openMermaidPopup({ svg, triggerButton: trigger });
    const closeBtn = document.querySelector<HTMLButtonElement>(
      ".mermaid-popup-close",
    );
    expect(closeBtn).not.toBeNull();
    expect(document.activeElement).toBe(closeBtn);

    controller.close();
    expect(document.activeElement).toBe(trigger);
  });

  test("TS-8: - button (zoom out) at scale=0.25 does not go below 0.25", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    // Push scale down until it clamps at the floor.
    const zoomOut = document.querySelector<HTMLButtonElement>(
      ".mermaid-popup-zoom-out",
    );
    for (let i = 0; i < 40; i++) zoomOut?.click();
    const state = __readActivePopupState();
    expect(state).not.toBeNull();
    expect(state?.scale).toBeCloseTo(0.25, 5);

    // Additional press should stay clamped.
    zoomOut?.click();
    expect(__readActivePopupState()?.scale).toBeCloseTo(0.25, 5);

    controller.close();
  });

  test("TS-9: + button (zoom in) at scale=5.0 does not exceed 5.0", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const zoomIn = document.querySelector<HTMLButtonElement>(
      ".mermaid-popup-zoom-in",
    );
    for (let i = 0; i < 40; i++) zoomIn?.click();
    const state = __readActivePopupState();
    expect(state).not.toBeNull();
    expect(state?.scale).toBeCloseTo(5.0, 5);

    zoomIn?.click();
    expect(__readActivePopupState()?.scale).toBeCloseTo(5.0, 5);

    controller.close();
  });

  test("TS-10: pressing '0' restores {scale:1.0, panX:0, panY:0} after arbitrary zoom + pan", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    // Perturb state: zoom in a couple steps and drag-pan.
    const zoomIn = document.querySelector<HTMLButtonElement>(
      ".mermaid-popup-zoom-in",
    );
    zoomIn?.click();
    zoomIn?.click();

    const stage = document.querySelector<HTMLElement>(".mermaid-popup-stage");
    stage?.dispatchEvent(
      new MouseEvent("mousedown", { button: 0, bubbles: true }),
    );
    // Simulate a drag via manual event props (happy-dom's MouseEvent may not
    // preserve movementX/Y, so we dispatch our own shape).
    const move = new MouseEvent("mousemove", { bubbles: true });
    Object.defineProperty(move, "movementX", { value: 20, configurable: true });
    Object.defineProperty(move, "movementY", { value: 15, configurable: true });
    window.dispatchEvent(move);
    window.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));

    // Now reset with the "0" key on the overlay.
    const overlay = document.querySelector<HTMLElement>(
      ".mermaid-popup-overlay",
    );
    overlay?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "0",
        bubbles: true,
        cancelable: true,
      }),
    );

    const state = __readActivePopupState();
    expect(state?.scale).toBe(1.0);
    expect(state?.panX).toBe(0);
    expect(state?.panY).toBe(0);

    controller.close();
  });

  test("TS-11: ESC keydown on overlay closes the popup and calls stopPropagation()", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    openMermaidPopup({ svg, triggerButton: trigger });

    const overlay = document.querySelector<HTMLElement>(
      ".mermaid-popup-overlay",
    );
    expect(overlay).not.toBeNull();

    let stopped = 0;
    let defaulted = 0;
    const event = new KeyboardEvent("keydown", {
      key: "Escape",
      bubbles: true,
      cancelable: true,
    });
    const origStop = event.stopPropagation.bind(event);
    const origPreventDefault = event.preventDefault.bind(event);
    event.stopPropagation = () => {
      stopped++;
      origStop();
    };
    event.preventDefault = () => {
      defaulted++;
      origPreventDefault();
    };
    overlay?.dispatchEvent(event);

    // Overlay is gone.
    expect(document.querySelector(".mermaid-popup-overlay")).toBeNull();
    // Propagation was stopped.
    expect(stopped).toBeGreaterThan(0);
    expect(defaulted).toBeGreaterThan(0);
  });

  test("TS-12: click on overlay itself closes; click on child stage does NOT close", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const overlay = document.querySelector<HTMLElement>(
      ".mermaid-popup-overlay",
    );
    const stage = document.querySelector<HTMLElement>(".mermaid-popup-stage");

    // Click on inner child — should NOT close.
    const innerClick = new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(innerClick, "target", {
      value: stage,
      configurable: true,
    });
    overlay?.dispatchEvent(innerClick);
    expect(document.querySelector(".mermaid-popup-overlay")).not.toBeNull();

    // Click on overlay (background) — should close.
    const bgClick = new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(bgClick, "target", {
      value: overlay,
      configurable: true,
    });
    overlay?.dispatchEvent(bgClick);
    expect(document.querySelector(".mermaid-popup-overlay")).toBeNull();

    // Redundant close for safety (controller is a no-op after teardown).
    controller.close();
  });

  test("TS-13: wheel deltaY<0 grows scale and calls preventDefault", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const stage = document.querySelector<HTMLElement>(".mermaid-popup-stage");
    const baseline = __readActivePopupState()?.scale ?? 0;

    let defaulted = 0;
    const wheel = new Event("wheel", { bubbles: true, cancelable: true });
    Object.defineProperty(wheel, "deltaY", { value: -100, configurable: true });
    const origPreventDefault = wheel.preventDefault.bind(wheel);
    wheel.preventDefault = () => {
      defaulted++;
      origPreventDefault();
    };
    stage?.dispatchEvent(wheel);

    const state = __readActivePopupState();
    expect(state?.scale).toBeGreaterThan(baseline);
    // Should approximate a 1.1× step from the baseline.
    expect(state?.scale).toBeCloseTo(baseline * 1.1, 5);
    expect(defaulted).toBe(1);

    controller.close();
  });

  test("TS-14: mousedown → mousemove(dx,dy) → mouseup updates panX/panY by summed deltas", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const stage = document.querySelector<HTMLElement>(".mermaid-popup-stage");

    stage?.dispatchEvent(
      new MouseEvent("mousedown", {
        button: 0,
        bubbles: true,
        cancelable: true,
      }),
    );

    // Two moves — panX/Y should accumulate.
    const m1 = new MouseEvent("mousemove", { bubbles: true });
    Object.defineProperty(m1, "movementX", { value: 10, configurable: true });
    Object.defineProperty(m1, "movementY", { value: -5, configurable: true });
    window.dispatchEvent(m1);

    const m2 = new MouseEvent("mousemove", { bubbles: true });
    Object.defineProperty(m2, "movementX", { value: 3, configurable: true });
    Object.defineProperty(m2, "movementY", { value: 4, configurable: true });
    window.dispatchEvent(m2);

    window.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));

    const state = __readActivePopupState();
    expect(state?.panX).toBe(13);
    expect(state?.panY).toBe(-1);

    // A move after mouseup should not accumulate.
    const m3 = new MouseEvent("mousemove", { bubbles: true });
    Object.defineProperty(m3, "movementX", { value: 100, configurable: true });
    Object.defineProperty(m3, "movementY", { value: 100, configurable: true });
    window.dispatchEvent(m3);
    expect(__readActivePopupState()?.panX).toBe(13);
    expect(__readActivePopupState()?.panY).toBe(-1);

    controller.close();
  });

  test("TS-15: window resize while open recomputes the fit factor k", () => {
    setWindowSize(1000, 800);
    const svg = makeSvg("0 0 100 80"); // 10:8 ratio
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const initial = __readActivePopupState()?.fitK ?? 0;
    // At 1000x800 with stageFill=0.8: aw=800, ah=640; min(800/100, 640/80) = 8.
    expect(initial).toBeCloseTo(8, 5);

    // Shrink window and dispatch resize.
    setWindowSize(400, 300);
    window.dispatchEvent(new Event("resize"));

    const after = __readActivePopupState()?.fitK ?? 0;
    // At 400x300: aw=320, ah=240; min(320/100, 240/80) = 3.
    expect(after).toBeCloseTo(3, 5);
    expect(after).not.toBe(initial);

    controller.close();
  });

  test("single-instance guard: a second openMermaidPopup while one is open returns the same controller", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const c1 = openMermaidPopup({ svg, triggerButton: trigger });
    const c2 = openMermaidPopup({ svg, triggerButton: trigger });
    expect(c2).toBe(c1);
    expect(
      document.body.querySelectorAll(".mermaid-popup-overlay").length,
    ).toBe(1);

    c1.close();
  });
});

describe("openMermaidPopup fixes (TS-16 through TS-20)", () => {
  test("TS-16: + / - buttons and keys step scale by 0.25 (additive); wheel keeps ×1.1", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const zoomIn = document.querySelector<HTMLButtonElement>(
      ".mermaid-popup-zoom-in",
    );
    const zoomOut = document.querySelector<HTMLButtonElement>(
      ".mermaid-popup-zoom-out",
    );

    // Baseline starts at exactly 1.0.
    expect(__readActivePopupState()?.scale).toBeCloseTo(1.0, 5);

    // + button: 1.0 → 1.25 (additive step of 0.25).
    zoomIn?.click();
    expect(__readActivePopupState()?.scale).toBeCloseTo(1.25, 5);

    // - button: 1.25 → 1.0.
    zoomOut?.click();
    expect(__readActivePopupState()?.scale).toBeCloseTo(1.0, 5);

    // keyboard "+": 1.0 → 1.25.
    const overlay = document.querySelector<HTMLElement>(
      ".mermaid-popup-overlay",
    );
    overlay?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "+",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(__readActivePopupState()?.scale).toBeCloseTo(1.25, 5);

    // keyboard "-": 1.25 → 1.0.
    overlay?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "-",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(__readActivePopupState()?.scale).toBeCloseTo(1.0, 5);

    // wheel keeps the multiplicative 1.1 step: 1.0 → 1.1.
    const stage = document.querySelector<HTMLElement>(".mermaid-popup-stage");
    const wheel = new Event("wheel", { bubbles: true, cancelable: true });
    Object.defineProperty(wheel, "deltaY", { value: -100, configurable: true });
    stage?.dispatchEvent(wheel);
    expect(__readActivePopupState()?.scale).toBeCloseTo(1.1, 5);

    controller.close();
  });

  test("TS-17: Tab from last button wraps to first; Shift+Tab from first wraps to last", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const overlay = document.querySelector<HTMLElement>(
      ".mermaid-popup-overlay",
    );
    const closeBtn = document.querySelector<HTMLButtonElement>(
      ".mermaid-popup-close",
    );
    const zoomOut = document.querySelector<HTMLButtonElement>(
      ".mermaid-popup-zoom-out",
    );

    // Focus the last button (zoom-out); Tab must wrap to the first (close).
    zoomOut?.focus();
    expect(document.activeElement).toBe(zoomOut);
    overlay?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).toBe(closeBtn);

    // Focus the first button (close); Shift+Tab must wrap to the last (zoom-out).
    closeBtn?.focus();
    overlay?.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Tab",
        shiftKey: true,
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(document.activeElement).toBe(zoomOut);

    controller.close();
  });

  test("TS-18: pan-end mouseup on background does not close; a clean background click closes", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const overlay = document.querySelector<HTMLElement>(
      ".mermaid-popup-overlay",
    );
    const stage = document.querySelector<HTMLElement>(".mermaid-popup-stage");

    // Drag: mousedown → mousemove (actual movement) → mouseup.
    stage?.dispatchEvent(
      new MouseEvent("mousedown", { button: 0, bubbles: true }),
    );
    const move = new MouseEvent("mousemove", { bubbles: true });
    Object.defineProperty(move, "movementX", { value: 30, configurable: true });
    Object.defineProperty(move, "movementY", { value: 10, configurable: true });
    window.dispatchEvent(move);
    window.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));

    // The synthesized click at the end of the drag lands on the overlay bg.
    const dragEndClick = new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(dragEndClick, "target", {
      value: overlay,
      configurable: true,
    });
    overlay?.dispatchEvent(dragEndClick);
    // Must NOT close (pan-end guard consumes the click).
    expect(document.querySelector(".mermaid-popup-overlay")).not.toBeNull();

    // A clean background click (no intervening drag movement) closes.
    const cleanClick = new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(cleanClick, "target", {
      value: overlay,
      configurable: true,
    });
    overlay?.dispatchEvent(cleanClick);
    expect(document.querySelector(".mermaid-popup-overlay")).toBeNull();

    controller.close();
  });

  test("TS-19: cloned SVG has explicit viewBox-derived width/height attributes and inline max sizes of none", () => {
    const svg = document.createElementNS(SVG_NS, "svg") as SVGElement;
    svg.setAttribute("viewBox", "0 0 200 100");
    // Mermaid's useMaxWidth:true output: width="100%" + inline max-width.
    svg.setAttribute("width", "100%");
    svg.setAttribute("height", "100%");
    svg.style.maxWidth = "200px";
    svg.style.maxHeight = "100px";
    svg.style.width = "100%";
    svg.style.height = "auto";
    document.body.appendChild(svg);
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const clone = document.querySelector<SVGElement>(
      ".mermaid-popup-stage svg",
    );
    expect(clone).not.toBeNull();
    // Explicit intrinsic-size (px) attributes so the untransformed base box
    // equals the viewBox in BOTH dimensions and the fit factor is not
    // width-based (tall diagrams would otherwise clip).
    expect(clone?.getAttribute("width")).toBe("200");
    expect(clone?.getAttribute("height")).toBe("100");
    // Inline width/height cleared; inline max sizes forced to none so a
    // stylesheet / inherited max-width cannot re-clamp the clone.
    expect(clone?.style.width).toBe("");
    expect(clone?.style.height).toBe("");
    expect(clone?.style.maxWidth).toBe("none");
    expect(clone?.style.maxHeight).toBe("none");
    // The source SVG must be left untouched.
    expect(svg.getAttribute("width")).toBe("100%");

    controller.close();
  });

  test("TS-20: window blur while dragging clears drag state; later mousemove does not pan", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const stage = document.querySelector<HTMLElement>(".mermaid-popup-stage");

    // Start a drag and pan once.
    stage?.dispatchEvent(
      new MouseEvent("mousedown", { button: 0, bubbles: true }),
    );
    const m1 = new MouseEvent("mousemove", { bubbles: true });
    Object.defineProperty(m1, "movementX", { value: 10, configurable: true });
    Object.defineProperty(m1, "movementY", { value: 10, configurable: true });
    window.dispatchEvent(m1);
    expect(__readActivePopupState()?.panX).toBe(10);
    expect(__readActivePopupState()?.panY).toBe(10);
    expect(stage?.classList.contains("mermaid-popup-dragging")).toBe(true);

    // Blur mid-drag clears the drag state (and the dragging cursor class).
    window.dispatchEvent(new Event("blur"));
    expect(stage?.classList.contains("mermaid-popup-dragging")).toBe(false);

    // A subsequent mousemove must NOT pan.
    const m2 = new MouseEvent("mousemove", { bubbles: true });
    Object.defineProperty(m2, "movementX", { value: 50, configurable: true });
    Object.defineProperty(m2, "movementY", { value: 50, configurable: true });
    window.dispatchEvent(m2);
    expect(__readActivePopupState()?.panX).toBe(10);
    expect(__readActivePopupState()?.panY).toBe(10);

    controller.close();
  });
});

describe("openMermaidPopup arrow-key pan (TS-21)", () => {
  test("TS-21: arrow keys pan by 40px with scroll-direction semantics and preventDefault", () => {
    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });

    const overlay = document.querySelector<HTMLElement>(
      ".mermaid-popup-overlay",
    );

    const press = (key: string): boolean => {
      const ev = new KeyboardEvent("keydown", {
        key,
        bubbles: true,
        cancelable: true,
      });
      overlay?.dispatchEvent(ev);
      return ev.defaultPrevented;
    };

    // ArrowRight reveals content to the right: panX -= 40.
    expect(press("ArrowRight")).toBe(true);
    expect(__readActivePopupState()?.panX).toBe(-40);

    // ArrowLeft moves back: panX += 40 → 0.
    expect(press("ArrowLeft")).toBe(true);
    expect(__readActivePopupState()?.panX).toBe(0);

    // ArrowDown reveals content below: panY -= 40.
    expect(press("ArrowDown")).toBe(true);
    expect(__readActivePopupState()?.panY).toBe(-40);

    // ArrowUp moves back: panY += 40 → 0.
    expect(press("ArrowUp")).toBe(true);
    expect(__readActivePopupState()?.panY).toBe(0);

    // panX is untouched by vertical arrows and vice versa.
    expect(__readActivePopupState()?.panX).toBe(0);

    controller.close();
  });
});

describe("openMermaidPopup native ESC guard IPC (TS-22)", () => {
  afterEach(() => {
    // Remove the mock so unrelated tests see no window.ipc.
    delete (window as unknown as { ipc?: unknown }).ipc;
  });

  test("TS-22: opening posts esc-guard:on and closing posts esc-guard:off via window.ipc", () => {
    const posted: string[] = [];
    (window as unknown as { ipc: { postMessage: (m: string) => void } }).ipc = {
      postMessage: (m: string) => posted.push(m),
    };

    const svg = makeSvg();
    const trigger = makeTrigger();
    const controller = openMermaidPopup({ svg, triggerButton: trigger });
    expect(posted).toContain("__emterm_host:esc-guard:on");
    expect(posted).not.toContain("__emterm_host:esc-guard:off");

    controller.close();
    expect(posted).toContain("__emterm_host:esc-guard:off");
  });

  test("TS-22: open/close without window.ipc does not throw", () => {
    delete (window as unknown as { ipc?: unknown }).ipc;
    const svg = makeSvg();
    const trigger = makeTrigger();
    expect(() => {
      const controller = openMermaidPopup({ svg, triggerButton: trigger });
      controller.close();
    }).not.toThrow();
  });
});
