/**
 * Dialog Shell
 *
 * Shared helper for modal dialogs in the child WebView windows. Builds
 * the .dialog-overlay / .dialog-surface / .dialog-title / .dialog-body /
 * .dialog-actions structure, wires Esc / Enter / scrim semantics per
 * kind, applies ARIA attributes, and IME-safely dispatches Enter via
 * `event.isComposing`.
 *
 * Mirrors `dialogs:` in `doc/UI-DESIGN-GUIDELINES.yaml`:
 * - `dialogs.anatomy` → overlay / surface / title / body / actions
 * - `dialogs.layout`  → corner radius / padding / max-width come from
 *                       CSS variables in dialog-shell.css
 * - `dialogs.scrim`   → overlay background-color
 * - `dialogs.actions` → role-specific button colors
 * - `dialogs.keyboard` and `dialogs.focus` → encoded below
 */

export type DialogKind = "input" | "confirm" | "destructive-confirm";

export interface DialogShellOptions {
  /** Title text rendered in the dialog header. */
  title: string;
  /** ARIA label applied to the overlay (`aria-label`). */
  ariaLabel: string;
  /** Dialog kind — drives Enter / focus semantics. */
  kind: DialogKind;
  /**
   * When true, clicking the scrim (overlay outside the surface) triggers
   * the registered cancel callback. Defaults to true.
   */
  scrimClickCancels?: boolean;
}

export type ButtonRole = "primary" | "cancel" | "destructive";

export interface AddButtonOptions {
  label: string;
  role: ButtonRole;
  onClick: () => void;
}

export interface DialogShell {
  overlay: HTMLDivElement;
  surface: HTMLDivElement;
  title: HTMLHeadingElement;
  body: HTMLDivElement;
  actions: HTMLDivElement;
  /**
   * Append a labeled button to the actions row. The helper tracks which
   * button is `primary` (or `destructive`) and which is `cancel` so the
   * keyboard dispatcher can target them by role.
   *
   * The "OK" label is forbidden by `dialogs.labels.rules`; this is
   * enforced via console warning rather than a throw to avoid breaking
   * the WebView surface in production.
   */
  addButton(opts: AddButtonOptions): HTMLButtonElement;
  /** Remove the overlay from the DOM and detach all listeners. */
  close: () => void;
}

/**
 * Build a dialog shell, attach overlay-scoped event handlers, and
 * append the result to `document.body`. The returned object exposes the
 * structural slots so the caller can compose form rows into `body`
 * directly.
 */
export function createDialogShell(opts: DialogShellOptions): DialogShell {
  const scrimClickCancels = opts.scrimClickCancels ?? true;

  // ── DOM scaffolding ────────────────────────────────────────────────
  const overlay = document.createElement("div");
  overlay.className = "dialog-overlay";
  overlay.setAttribute("role", "dialog");
  overlay.setAttribute("aria-modal", "true");
  overlay.setAttribute("aria-label", opts.ariaLabel);

  const surface = document.createElement("div");
  surface.className = "dialog-surface";

  const title = document.createElement("h2");
  title.className = "dialog-title";
  title.textContent = opts.title;

  const body = document.createElement("div");
  body.className = "dialog-body";

  const actions = document.createElement("div");
  actions.className = "dialog-actions";

  surface.appendChild(title);
  surface.appendChild(body);
  surface.appendChild(actions);
  overlay.appendChild(surface);
  document.body.appendChild(overlay);

  // ── Role tracking ──────────────────────────────────────────────────
  let primaryButton: HTMLButtonElement | null = null;
  let primaryCallback: (() => void) | null = null;
  let cancelButton: HTMLButtonElement | null = null;
  let cancelCallback: (() => void) | null = null;

  // ── Keymap (capture phase, IME-safe) ───────────────────────────────
  const onKeydown = (event: KeyboardEvent) => {
    // IME-composing Enter / Escape would otherwise commit composition
    // AND fire the dialog action; respect isComposing so composition
    // ends without stealing the action keys.
    if (event.isComposing) {
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      if (cancelCallback) {
        cancelCallback();
      } else {
        close();
      }
      return;
    }
    if (event.key === "Enter") {
      // Enter inside a <textarea> should still produce a newline, not
      // submit the dialog. Match the standard browser behavior.
      const target = event.target as HTMLElement | null;
      if (target && target.tagName === "TEXTAREA") {
        return;
      }
      switch (opts.kind) {
        case "input":
        case "confirm":
          event.preventDefault();
          primaryCallback?.();
          break;
        case "destructive-confirm":
          event.preventDefault();
          cancelCallback?.();
          break;
      }
    }
  };
  overlay.addEventListener("keydown", onKeydown, true);

  // ── Scrim click → cancel ───────────────────────────────────────────
  const onOverlayClick = (event: MouseEvent) => {
    if (!scrimClickCancels) return;
    if (event.target === overlay) {
      cancelCallback?.();
    }
  };
  overlay.addEventListener("click", onOverlayClick);

  // ── Initial focus (next animation frame) ───────────────────────────
  let initialFocusDone = false;
  function applyInitialFocus(): void {
    if (initialFocusDone) return;
    initialFocusDone = true;
    switch (opts.kind) {
      case "input": {
        const first = body.querySelector<HTMLElement>(
          "input, textarea, select, [tabindex]:not([tabindex='-1'])",
        );
        first?.focus();
        break;
      }
      case "confirm":
        primaryButton?.focus();
        break;
      case "destructive-confirm":
        cancelButton?.focus();
        break;
    }
  }
  requestAnimationFrame(applyInitialFocus);

  // ── Public API ────────────────────────────────────────────────────
  let closed = false;
  function close(): void {
    if (closed) return;
    closed = true;
    overlay.removeEventListener("keydown", onKeydown, true);
    overlay.removeEventListener("click", onOverlayClick);
    overlay.remove();
  }

  function addButton(buttonOpts: AddButtonOptions): HTMLButtonElement {
    if (isGenericOkLabel(buttonOpts.label)) {
      // Forbidden by dialogs.labels.rules; surface as a console warning
      // so devs catch it without breaking the production surface.
      // eslint-disable-next-line no-console
      console.warn(
        `[dialog-shell] primary label must not be a generic OK — got ${JSON.stringify(buttonOpts.label)}`,
      );
    }
    const button = document.createElement("button");
    button.type = "button";
    button.className = `dialog-button dialog-button-${buttonOpts.role}`;
    button.textContent = buttonOpts.label;
    button.addEventListener("click", buttonOpts.onClick);
    actions.appendChild(button);

    if (buttonOpts.role === "primary" || buttonOpts.role === "destructive") {
      primaryButton = button;
      primaryCallback = buttonOpts.onClick;
    } else if (buttonOpts.role === "cancel") {
      cancelButton = button;
      cancelCallback = buttonOpts.onClick;
    }

    // If we already passed the initial focus point (rare — happens when
    // addButton runs synchronously and the test framework drains
    // microtasks before our requestAnimationFrame), re-apply.
    if (initialFocusDone) {
      // no-op — caller can refocus manually.
    }

    return button;
  }

  return {
    overlay,
    surface,
    title,
    body,
    actions,
    addButton,
    close,
  };
}

function isGenericOkLabel(label: string): boolean {
  return label.trim().toLowerCase() === "ok";
}
