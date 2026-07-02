/**
 * Mermaid fullscreen popup controller.
 *
 * Owns the lifecycle of the zoom / pan popup that overlays the entire
 * child WebView when the user clicks the Spread button on a mermaid block.
 *
 * The popup:
 * - clones the source SVG (leaves the original untouched)
 * - fits it to the stage on open and on `resize`
 * - supports zoom (+ / -/ wheel / buttons), pan (drag), reset (0 / button)
 * - closes on ESC, background click, or the × button
 * - restores focus and body scroll on close
 *
 * @module markdown/mermaid-popup
 */

import { t } from "../i18n/index.ts";

/** Options accepted by {@link openMermaidPopup}. */
export interface MermaidPopupOptions {
  /** SVG element currently in the DOM whose visual should be popped up. */
  svg: SVGElement;
  /** Button that triggered the popup — receives focus back on close. */
  triggerButton: HTMLElement;
}

/** Controller returned by {@link openMermaidPopup}. */
export interface MermaidPopupController {
  /** Close the popup, tearing down all DOM + listeners. */
  close: () => void;
}

/** Absolute clamp range on the user-selected scale factor. */
const MIN_SCALE = 0.25;
const MAX_SCALE = 5.0;
/** Additive step for + / - buttons and keyboard shortcuts (FR3). */
const ZOOM_STEP = 0.25;
/** Multiplicative step for wheel notches (FR3). */
const STEP_FACTOR = 1.1;
/** Fraction of the viewport each dimension of the stage occupies. */
const STAGE_FILL = 0.8;
/** Pixels panned per arrow-key press (FR4 arrow-key pan). */
const PAN_STEP = 40;

/**
 * Reserved host-control IPC messages. The native `webview_host` layer
 * consumes these (never forwarding them to user-level IPC) to suppress its
 * window-level Esc / q / Q close handler while the popup is open, so a single
 * ESC closes only the popup and not the whole Markdown viewer (FR6).
 */
const ESC_GUARD_ON = "__emterm_host:esc-guard:on";
const ESC_GUARD_OFF = "__emterm_host:esc-guard:off";

/**
 * Best-effort post to the host IPC channel. `window.ipc` exists inside the
 * wry-hosted viewer but not in unit tests / plain browsers, so both the
 * optional chaining and the try/catch keep this a safe no-op when absent.
 */
function postHostMessage(msg: string): void {
  try {
    (
      window as unknown as { ipc?: { postMessage?: (m: string) => void } }
    ).ipc?.postMessage?.(msg);
  } catch {
    // window.ipc unavailable (e.g. happy-dom): silently skip.
  }
}

/**
 * Module-level singleton guard: only one popup may exist at a time.
 * When a second `openMermaidPopup` call arrives while another is live,
 * we re-focus the existing close button and return the same controller.
 */
let activePopup: MermaidPopupController | null = null;

/**
 * Open the mermaid fullscreen popup.
 *
 * @returns A controller with `.close()` that reverts every DOM / listener /
 *   focus / body-overflow side effect performed on open.
 */
export function openMermaidPopup(
  opts: MermaidPopupOptions,
): MermaidPopupController {
  if (activePopup) {
    return activePopup;
  }

  const { svg, triggerButton } = opts;

  // ---- Build overlay DOM ------------------------------------------------
  const overlay = document.createElement("div");
  overlay.className = "mermaid-popup-overlay";
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("aria-modal", "true");
  overlay.tabIndex = -1;

  const stage = document.createElement("div");
  stage.className = "mermaid-popup-stage";
  overlay.appendChild(stage);

  /** Determine the intrinsic width / height of the source SVG. */
  const intrinsicSize = (): { w: number; h: number } => {
    const viewBox = svg.getAttribute("viewBox");
    if (viewBox) {
      const parts = viewBox.split(/\s+/).map((s) => parseFloat(s));
      if (parts.length === 4 && parts.every((n) => Number.isFinite(n))) {
        const w = parts[2] ?? 0;
        const h = parts[3] ?? 0;
        if (w > 0 && h > 0) return { w, h };
      }
    }
    const rect = svg.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      return { w: rect.width, h: rect.height };
    }
    return { w: 400, h: 300 };
  };

  // Clone the SVG so the original toolbar-hosted diagram is untouched.
  const clone = svg.cloneNode(true) as SVGElement;
  // Clone sizing normalization (FR2): Mermaid renders with useMaxWidth:true,
  // so the source SVG carries width="100%" plus an inline max-width that a
  // stylesheet cannot override. Merely *removing* the sizing attributes is
  // insufficient — an SVG without width/height resolves its base box from the
  // available layout width (not the viewBox), which makes the fit factor `k`
  // width-based and clips tall diagrams. So set EXPLICIT width/height
  // ATTRIBUTES (px) from the viewBox-derived intrinsic size, clear the inline
  // width/height styles, and force inline max sizes to "none", so the clone's
  // untransformed base box equals the intrinsic size in BOTH dimensions and
  // `k` truly fits width and height.
  const intrinsic = intrinsicSize();
  clone.setAttribute("width", String(intrinsic.w));
  clone.setAttribute("height", String(intrinsic.h));
  clone.style.width = "";
  clone.style.height = "";
  clone.style.maxWidth = "none";
  clone.style.maxHeight = "none";
  clone.style.transformOrigin = "center center";
  stage.appendChild(clone);

  // Close (×) button, top-right.
  const closeBtn = document.createElement("button");
  closeBtn.className = "mermaid-popup-close";
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", t("markdown.mermaidPopupClose"));
  closeBtn.textContent = "×";
  overlay.appendChild(closeBtn);

  // Zoom controls (+ / 0 / -), bottom-right.
  const controls = document.createElement("div");
  controls.className = "mermaid-popup-controls";

  const zoomInBtn = document.createElement("button");
  zoomInBtn.className = "mermaid-popup-btn mermaid-popup-zoom-in";
  zoomInBtn.type = "button";
  zoomInBtn.setAttribute("aria-label", t("markdown.mermaidPopupZoomIn"));
  zoomInBtn.textContent = "+";

  const resetBtn = document.createElement("button");
  resetBtn.className = "mermaid-popup-btn mermaid-popup-reset";
  resetBtn.type = "button";
  resetBtn.setAttribute("aria-label", t("markdown.mermaidPopupReset"));
  resetBtn.textContent = "0";

  const zoomOutBtn = document.createElement("button");
  zoomOutBtn.className = "mermaid-popup-btn mermaid-popup-zoom-out";
  zoomOutBtn.type = "button";
  zoomOutBtn.setAttribute("aria-label", t("markdown.mermaidPopupZoomOut"));
  zoomOutBtn.textContent = "−";

  controls.appendChild(zoomInBtn);
  controls.appendChild(resetBtn);
  controls.appendChild(zoomOutBtn);
  overlay.appendChild(controls);

  // ---- Zoom / pan state -------------------------------------------------
  let scale = 1.0;
  let panX = 0;
  let panY = 0;
  let fitK = 1.0;

  const stageArea = (): { w: number; h: number } => {
    const w =
      (typeof window !== "undefined" && window.innerWidth
        ? window.innerWidth
        : 800) * STAGE_FILL;
    const h =
      (typeof window !== "undefined" && window.innerHeight
        ? window.innerHeight
        : 600) * STAGE_FILL;
    return { w, h };
  };

  const recomputeFit = (): void => {
    const { w: sw, h: sh } = intrinsicSize();
    const { w: aw, h: ah } = stageArea();
    fitK = Math.min(aw / sw, ah / sh);
    if (!Number.isFinite(fitK) || fitK <= 0) fitK = 1.0;
  };

  const applyTransform = (): void => {
    const effective = scale * fitK;
    clone.style.transform = `translate(${panX}px, ${panY}px) scale(${effective})`;
  };

  const clamp = (v: number): number =>
    Math.max(MIN_SCALE, Math.min(MAX_SCALE, v));

  // Buttons + keyboard: additive 0.25 step (FR3).
  const zoomInStep = (): void => {
    scale = clamp(scale + ZOOM_STEP);
    applyTransform();
  };
  const zoomOutStep = (): void => {
    scale = clamp(scale - ZOOM_STEP);
    applyTransform();
  };
  // Wheel: multiplicative 1.1 step (FR3).
  const zoomInWheel = (): void => {
    scale = clamp(scale * STEP_FACTOR);
    applyTransform();
  };
  const zoomOutWheel = (): void => {
    scale = clamp(scale / STEP_FACTOR);
    applyTransform();
  };
  const resetView = (): void => {
    scale = 1.0;
    panX = 0;
    panY = 0;
    applyTransform();
  };

  recomputeFit();
  applyTransform();

  // ---- Listeners --------------------------------------------------------
  zoomInBtn.addEventListener("click", zoomInStep);
  zoomOutBtn.addEventListener("click", zoomOutStep);
  resetBtn.addEventListener("click", resetView);

  const onWheel = (ev: WheelEvent): void => {
    ev.preventDefault();
    if (ev.deltaY < 0) {
      zoomInWheel();
    } else if (ev.deltaY > 0) {
      zoomOutWheel();
    }
  };
  stage.addEventListener("wheel", onWheel, { passive: false });

  // Drag pan (left mouse button only).
  let dragging = false;
  // Pan-end guard (FR6): set when a drag actually moves the diagram so the
  // synthesized click at the end of the drag does not close the popup.
  let didPan = false;
  const onMouseDown = (ev: MouseEvent): void => {
    if (ev.button !== 0) return;
    dragging = true;
    didPan = false;
    stage.classList.add("mermaid-popup-dragging");
  };
  const onMouseMove = (ev: MouseEvent): void => {
    if (!dragging) return;
    didPan = true;
    panX += ev.movementX;
    panY += ev.movementY;
    applyTransform();
  };
  const onMouseUp = (): void => {
    dragging = false;
    stage.classList.remove("mermaid-popup-dragging");
  };
  // Stuck-drag guard (FR4): a window blur mid-drag clears the drag state.
  const onBlur = (): void => {
    dragging = false;
    stage.classList.remove("mermaid-popup-dragging");
  };
  stage.addEventListener("mousedown", onMouseDown);
  window.addEventListener("mousemove", onMouseMove);
  window.addEventListener("mouseup", onMouseUp);
  window.addEventListener("blur", onBlur);

  // Overlay-level keyboard handling (captured so ESC does not reach the
  // Markdown viewer behind us).
  // Focus trap order (FR9): DOM order of the four popup buttons.
  const focusOrder: HTMLElement[] = [closeBtn, zoomInBtn, resetBtn, zoomOutBtn];
  const trapFocus = (backward: boolean): void => {
    const current = focusOrder.indexOf(document.activeElement as HTMLElement);
    let next: number;
    if (backward) {
      next = current <= 0 ? focusOrder.length - 1 : current - 1;
    } else {
      next = current === focusOrder.length - 1 ? 0 : current + 1;
    }
    focusOrder[next]?.focus();
  };

  const onKeydown = (ev: KeyboardEvent): void => {
    switch (ev.key) {
      case "Escape":
        ev.stopPropagation();
        ev.preventDefault();
        controller.close();
        return;
      case "Tab":
        ev.preventDefault();
        trapFocus(ev.shiftKey);
        return;
      case "+":
      case "=":
        ev.preventDefault();
        zoomInStep();
        return;
      case "-":
      case "_":
        ev.preventDefault();
        zoomOutStep();
        return;
      case "0":
        ev.preventDefault();
        resetView();
        return;
      // Arrow-key pan (FR4): scroll-direction semantics — ArrowRight reveals
      // content to the right (pan the diagram left), etc.
      case "ArrowRight":
        ev.preventDefault();
        panX -= PAN_STEP;
        applyTransform();
        return;
      case "ArrowLeft":
        ev.preventDefault();
        panX += PAN_STEP;
        applyTransform();
        return;
      case "ArrowDown":
        ev.preventDefault();
        panY -= PAN_STEP;
        applyTransform();
        return;
      case "ArrowUp":
        ev.preventDefault();
        panY += PAN_STEP;
        applyTransform();
        return;
      default:
        return;
    }
  };
  overlay.addEventListener("keydown", onKeydown, { capture: true });

  // Background click (target === overlay) closes.
  const onOverlayClick = (ev: MouseEvent): void => {
    // Pan-end guard (FR6): consume the click synthesized at the end of a
    // pan-drag so it does not close the popup. A clean click still closes.
    if (didPan) {
      didPan = false;
      return;
    }
    if (ev.target === overlay) {
      controller.close();
    }
  };
  overlay.addEventListener("click", onOverlayClick);

  closeBtn.addEventListener("click", () => controller.close());

  const onResize = (): void => {
    recomputeFit();
    applyTransform();
  };
  window.addEventListener("resize", onResize);

  // ---- Mount ------------------------------------------------------------
  const previousOverflow = document.body.style.overflow;
  document.body.style.overflow = "hidden";
  document.body.appendChild(overlay);
  closeBtn.focus();
  // FR6 native ESC guard: ask the host to suppress its window-level Esc / q / Q
  // close handler while the popup is open (DOM stopPropagation cannot reach it).
  postHostMessage(ESC_GUARD_ON);

  // ---- Controller -------------------------------------------------------
  const controller: MermaidPopupController = {
    close: () => {
      if (activePopup !== controller) return;
      // FR6 native ESC guard: re-enable the host's window-level close handler.
      postHostMessage(ESC_GUARD_OFF);
      stage.removeEventListener("wheel", onWheel);
      stage.removeEventListener("mousedown", onMouseDown);
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
      window.removeEventListener("blur", onBlur);
      overlay.removeEventListener("keydown", onKeydown, { capture: true });
      overlay.removeEventListener("click", onOverlayClick);
      window.removeEventListener("resize", onResize);

      if (overlay.parentNode) {
        overlay.parentNode.removeChild(overlay);
      }
      document.body.style.overflow = previousOverflow;
      activePopup = null;
      activeStateReader = null;
      try {
        triggerButton.focus();
      } catch {
        // Element may have been removed from the DOM in the meantime;
        // focus restoration is best-effort.
      }
    },
  };
  activePopup = controller;
  activeStateReader = () => ({ scale, panX, panY, fitK });
  return controller;
}

/** Snapshot of the popup's zoom / pan state (test helper). */
export interface MermaidPopupInternals {
  scale: number;
  panX: number;
  panY: number;
  fitK: number;
}

/** Reader wired by {@link openMermaidPopup} so unit tests can inspect state. */
let activeStateReader: (() => MermaidPopupInternals) | null = null;

/**
 * TEST-ONLY: return the live pan/zoom snapshot, or null if no popup is open.
 * Intentionally exported so `mermaid-popup.test.ts` can assert on state
 * transitions without exposing every closure variable through the public
 * controller interface.
 */
export function __readActivePopupState(): MermaidPopupInternals | null {
  return activeStateReader ? activeStateReader() : null;
}

/**
 * TEST-ONLY: force-close any active popup so the module-level singleton is
 * reset between test cases (even if the test threw before its own close()).
 */
export function __closeActivePopupForTest(): void {
  activePopup?.close();
}
