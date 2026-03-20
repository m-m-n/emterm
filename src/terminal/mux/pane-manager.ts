/**
 * Mux pane manager — manages per-pane Canvas + WASM instances.
 *
 * Each mux pane gets its own:
 * - Canvas element for rendering
 * - WASM TerminalCore instance for state/parsing
 * - Rendering pipeline (reuses existing CanvasRenderer)
 */

import { initWasm } from "../wasm/loader";

/**
 * State for a single mux pane on the GUI side.
 */
export interface MuxPaneState {
  paneId: number;
  canvas: HTMLCanvasElement;
  container: HTMLElement;
  // WASM and renderer instances will be added in integration
  // wasmCore: TerminalCore;
  // renderer: ITerminalRenderer;
  active: boolean;
}

/**
 * Manages multiple mux panes within a single mux session.
 */
export class MuxPaneManager {
  private panes: Map<number, MuxPaneState> = new Map();
  private containerElement: HTMLElement;
  private activePaneId: number | null = null;

  constructor(container: HTMLElement) {
    this.containerElement = container;
  }

  /**
   * Create a new pane with Canvas element.
   * Returns the pane state.
   */
  async createPane(paneId: number): Promise<MuxPaneState> {
    // Ensure WASM is initialized
    await initWasm();

    // Create container div for this pane
    const paneContainer = document.createElement("div");
    paneContainer.className = "mux-pane";
    paneContainer.dataset.paneId = String(paneId);
    paneContainer.style.position = "relative";
    paneContainer.style.overflow = "hidden";

    // Create canvas
    const canvas = document.createElement("canvas");
    canvas.className = "mux-pane-canvas";
    canvas.style.width = "100%";
    canvas.style.height = "100%";
    paneContainer.appendChild(canvas);

    this.containerElement.appendChild(paneContainer);

    const state: MuxPaneState = {
      paneId,
      canvas,
      container: paneContainer,
      active: this.panes.size === 0, // First pane is active
    };

    this.panes.set(paneId, state);

    if (state.active) {
      this.activePaneId = paneId;
      paneContainer.classList.add("mux-pane-active");
    }

    return state;
  }

  /** Remove a pane and its DOM elements. */
  removePane(paneId: number): void {
    const pane = this.panes.get(paneId);
    if (!pane) return;

    pane.container.remove();
    this.panes.delete(paneId);

    // If active pane was removed, activate another
    if (this.activePaneId === paneId) {
      const next = this.panes.keys().next();
      this.activePaneId = next.done ? null : next.value;
      if (this.activePaneId !== null) {
        const nextPane = this.panes.get(this.activePaneId);
        if (nextPane) {
          nextPane.active = true;
          nextPane.container.classList.add("mux-pane-active");
        }
      }
    }
  }

  /** Set the active pane. */
  setActivePane(paneId: number): void {
    // Deactivate current
    if (this.activePaneId !== null) {
      const current = this.panes.get(this.activePaneId);
      if (current) {
        current.active = false;
        current.container.classList.remove("mux-pane-active");
      }
    }

    // Activate new
    const pane = this.panes.get(paneId);
    if (pane) {
      pane.active = true;
      pane.container.classList.add("mux-pane-active");
      this.activePaneId = paneId;
    }
  }

  /** Get the active pane state. */
  getActivePane(): MuxPaneState | null {
    if (this.activePaneId === null) return null;
    return this.panes.get(this.activePaneId) ?? null;
  }

  /** Get pane count. */
  get paneCount(): number {
    return this.panes.size;
  }

  /** Destroy all panes and clean up DOM. */
  destroyAll(): void {
    for (const pane of this.panes.values()) {
      pane.container.remove();
    }
    this.panes.clear();
    this.activePaneId = null;
  }
}
