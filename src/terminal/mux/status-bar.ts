/**
 * Mux status bar component.
 *
 * DOM-rendered status bar showing session name, window list, and time.
 * Event-driven updates from daemon (no polling).
 */

/** Status bar position setting. */
export type StatusBarPosition = "top" | "bottom";

/** Status bar state data. */
export interface StatusBarData {
  sessionName: string;
  windowNames: string[];
  activeWindowIndex: number;
}

/** Status bar height in pixels. */
export const STATUS_BAR_HEIGHT = 24;

/**
 * Mux status bar UI component.
 */
export class MuxStatusBar {
  private container: HTMLElement;
  private element: HTMLElement;
  private position: StatusBarPosition;
  private data: StatusBarData;
  private timeInterval: ReturnType<typeof setInterval> | null = null;

  constructor(container: HTMLElement, position: StatusBarPosition = "bottom") {
    this.container = container;
    this.position = position;
    this.data = { sessionName: "", windowNames: [], activeWindowIndex: 0 };

    // Create status bar element
    this.element = document.createElement("div");
    this.element.className = "mux-status-bar";
    this.element.style.height = `${STATUS_BAR_HEIGHT}px`;
    this.element.style.lineHeight = `${STATUS_BAR_HEIGHT}px`;
    this.element.style.position = "absolute";
    this.element.style.left = "0";
    this.element.style.right = "0";
    this.element.style.fontSize = "12px";
    this.element.style.fontFamily = "var(--md-sys-typescale-body-small-font, sans-serif)";
    this.element.style.padding = "0 8px";
    this.element.style.display = "flex";
    this.element.style.justifyContent = "space-between";
    this.element.style.backgroundColor = "var(--md-sys-color-surface-container, #1E1E2E)";
    this.element.style.color = "var(--md-sys-color-on-surface-variant, #CAC4D0)";
    this.element.style.zIndex = "10";

    this.applyPosition();
    this.container.appendChild(this.element);
    this.render();
    this.startClock();
  }

  /** Update status bar data from daemon push. */
  update(data: StatusBarData): void {
    this.data = data;
    this.render();
  }

  /** Change position (top/bottom). */
  setPosition(position: StatusBarPosition): void {
    this.position = position;
    this.applyPosition();
  }

  /** Get the height to account for in pane layout. */
  getHeight(): number {
    return STATUS_BAR_HEIGHT;
  }

  /** Destroy and clean up. */
  destroy(): void {
    if (this.timeInterval !== null) {
      clearInterval(this.timeInterval);
      this.timeInterval = null;
    }
    this.element.remove();
  }

  private applyPosition(): void {
    if (this.position === "top") {
      this.element.style.top = "0";
      this.element.style.bottom = "";
    } else {
      this.element.style.top = "";
      this.element.style.bottom = "0";
    }
  }

  private render(): void {
    // Clear existing children
    while (this.element.firstChild) {
      this.element.removeChild(this.element.firstChild);
    }

    const leftSpan = document.createElement("span");
    this.renderLeft(leftSpan);

    const rightSpan = document.createElement("span");
    rightSpan.textContent = this.renderRight();

    this.element.appendChild(leftSpan);
    this.element.appendChild(rightSpan);
  }

  private renderLeft(container: HTMLElement): void {
    const sessionName = this.data.sessionName || "mux";
    container.appendChild(document.createTextNode(`[${sessionName}] `));

    this.data.windowNames.forEach((name, i) => {
      if (i > 0) {
        container.appendChild(document.createTextNode(" "));
      }

      const label = `${i}:${name}`;
      if (i === this.data.activeWindowIndex) {
        const activeSpan = document.createElement("span");
        activeSpan.style.color = "var(--md-sys-color-primary, #D0BCFF)";
        activeSpan.textContent = `${label}*`;
        container.appendChild(activeSpan);
      } else {
        container.appendChild(document.createTextNode(label));
      }
    });
  }

  private renderRight(): string {
    const now = new Date();
    const h = String(now.getHours()).padStart(2, "0");
    const m = String(now.getMinutes()).padStart(2, "0");
    return `${h}:${m}`;
  }

  private startClock(): void {
    this.timeInterval = setInterval(() => {
      this.render();
    }, 60_000); // Update every minute
  }
}
