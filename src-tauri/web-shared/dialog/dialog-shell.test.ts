/**
 * Tests for the WebView dialog shell helper.
 *
 * Covers the contract documented in `dialog-shell.ts`:
 * - Structural slots are returned with the correct classes
 * - ARIA attributes are applied
 * - Esc / Enter dispatch per kind (FR5)
 * - Scrim click → cancel (FR3)
 * - IME-composing Enter is ignored
 */

import { afterEach, describe, expect, test } from "bun:test";

import { createDialogShell } from "./dialog-shell.ts";

afterEach(() => {
  // Wipe any leftover overlays between tests so DOM queries stay deterministic.
  for (const overlay of document.querySelectorAll(".dialog-overlay")) {
    overlay.remove();
  }
});

describe("createDialogShell()", () => {
  test("returns the documented structure with correct classes and ARIA", () => {
    const shell = createDialogShell({
      title: "Title",
      ariaLabel: "Test dialog",
      kind: "input",
    });

    expect(shell.overlay.classList.contains("dialog-overlay")).toBe(true);
    expect(shell.overlay.getAttribute("role")).toBe("dialog");
    expect(shell.overlay.getAttribute("aria-modal")).toBe("true");
    expect(shell.overlay.getAttribute("aria-label")).toBe("Test dialog");

    expect(shell.surface.classList.contains("dialog-surface")).toBe(true);
    expect(shell.title.classList.contains("dialog-title")).toBe(true);
    expect(shell.title.textContent).toBe("Title");
    expect(shell.body.classList.contains("dialog-body")).toBe(true);
    expect(shell.actions.classList.contains("dialog-actions")).toBe(true);

    expect(document.body.contains(shell.overlay)).toBe(true);
    shell.close();
    expect(document.body.contains(shell.overlay)).toBe(false);
  });

  test("addButton applies role classes and tracks primary / cancel", () => {
    const shell = createDialogShell({
      title: "Title",
      ariaLabel: "T",
      kind: "input",
    });
    let primaryCalled = 0;
    let cancelCalled = 0;
    shell.addButton({
      role: "primary",
      label: "Save",
      onClick: () => primaryCalled++,
    });
    shell.addButton({
      role: "cancel",
      label: "Cancel",
      onClick: () => cancelCalled++,
    });

    const primary = shell.actions.querySelector(
      ".dialog-button-primary",
    ) as HTMLButtonElement;
    const cancel = shell.actions.querySelector(
      ".dialog-button-cancel",
    ) as HTMLButtonElement;
    expect(primary).toBeTruthy();
    expect(cancel).toBeTruthy();
    expect(primary.textContent).toBe("Save");
    expect(cancel.textContent).toBe("Cancel");

    primary.click();
    expect(primaryCalled).toBe(1);
    cancel.click();
    expect(cancelCalled).toBe(1);
    shell.close();
  });

  test("Esc keydown triggers cancel callback", () => {
    const shell = createDialogShell({
      title: "T",
      ariaLabel: "T",
      kind: "confirm",
    });
    let cancelled = 0;
    shell.addButton({
      role: "primary",
      label: "Save",
      onClick: () => {},
    });
    shell.addButton({
      role: "cancel",
      label: "Cancel",
      onClick: () => cancelled++,
    });

    shell.overlay.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(cancelled).toBe(1);
    shell.close();
  });

  test("Enter on input kind triggers primary callback", () => {
    const shell = createDialogShell({
      title: "T",
      ariaLabel: "T",
      kind: "input",
    });
    let primary = 0;
    let cancel = 0;
    shell.addButton({
      role: "primary",
      label: "Save",
      onClick: () => primary++,
    });
    shell.addButton({
      role: "cancel",
      label: "Cancel",
      onClick: () => cancel++,
    });

    shell.overlay.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Enter",
        bubbles: true,
        cancelable: true,
      }),
    );
    expect(primary).toBe(1);
    expect(cancel).toBe(0);
    shell.close();
  });

  test("Enter on confirm kind triggers primary callback", () => {
    const shell = createDialogShell({
      title: "T",
      ariaLabel: "T",
      kind: "confirm",
    });
    let primary = 0;
    let cancel = 0;
    shell.addButton({
      role: "primary",
      label: "Upload",
      onClick: () => primary++,
    });
    shell.addButton({
      role: "cancel",
      label: "Cancel",
      onClick: () => cancel++,
    });

    shell.overlay.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(primary).toBe(1);
    expect(cancel).toBe(0);
    shell.close();
  });

  test("Enter on destructive-confirm triggers cancel, not primary", () => {
    const shell = createDialogShell({
      title: "T",
      ariaLabel: "T",
      kind: "destructive-confirm",
    });
    let primary = 0;
    let cancel = 0;
    shell.addButton({
      role: "destructive",
      label: "Overwrite",
      onClick: () => primary++,
    });
    shell.addButton({
      role: "cancel",
      label: "Cancel",
      onClick: () => cancel++,
    });

    shell.overlay.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
    expect(primary).toBe(0);
    expect(cancel).toBe(1);
    shell.close();
  });

  test("scrim click triggers cancel when scrimClickCancels is true (default)", () => {
    const shell = createDialogShell({
      title: "T",
      ariaLabel: "T",
      kind: "input",
    });
    let cancel = 0;
    shell.addButton({
      role: "primary",
      label: "Save",
      onClick: () => {},
    });
    shell.addButton({
      role: "cancel",
      label: "Cancel",
      onClick: () => cancel++,
    });

    // Click directly on overlay (scrim area).
    const click = new Event("click", { bubbles: true, cancelable: true });
    Object.defineProperty(click, "target", {
      value: shell.overlay,
      configurable: true,
    });
    shell.overlay.dispatchEvent(click);
    expect(cancel).toBe(1);

    // Click inside the surface should NOT cancel.
    const innerClick = new Event("click", {
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(innerClick, "target", {
      value: shell.surface,
      configurable: true,
    });
    shell.overlay.dispatchEvent(innerClick);
    expect(cancel).toBe(1);

    shell.close();
  });

  test("scrim click does NOT cancel when scrimClickCancels is false", () => {
    const shell = createDialogShell({
      title: "T",
      ariaLabel: "T",
      kind: "input",
      scrimClickCancels: false,
    });
    let cancel = 0;
    shell.addButton({
      role: "cancel",
      label: "Cancel",
      onClick: () => cancel++,
    });

    const click = new Event("click", { bubbles: true, cancelable: true });
    Object.defineProperty(click, "target", {
      value: shell.overlay,
      configurable: true,
    });
    shell.overlay.dispatchEvent(click);
    expect(cancel).toBe(0);
    shell.close();
  });

  test("IME-composing Enter is ignored", () => {
    const shell = createDialogShell({
      title: "T",
      ariaLabel: "T",
      kind: "input",
    });
    let primary = 0;
    shell.addButton({
      role: "primary",
      label: "Save",
      onClick: () => primary++,
    });
    shell.addButton({
      role: "cancel",
      label: "Cancel",
      onClick: () => {},
    });

    const event = new KeyboardEvent("keydown", {
      key: "Enter",
      bubbles: true,
      cancelable: true,
    });
    Object.defineProperty(event, "isComposing", {
      value: true,
      configurable: true,
    });
    shell.overlay.dispatchEvent(event);
    expect(primary).toBe(0);
    shell.close();
  });

  test("close() removes overlay and detaches listeners", () => {
    const shell = createDialogShell({
      title: "T",
      ariaLabel: "T",
      kind: "confirm",
    });
    let cancel = 0;
    shell.addButton({
      role: "primary",
      label: "Save",
      onClick: () => {},
    });
    shell.addButton({
      role: "cancel",
      label: "Cancel",
      onClick: () => cancel++,
    });
    shell.close();
    expect(document.body.contains(shell.overlay)).toBe(false);
    // After close, dispatching Escape on the (detached) overlay must
    // not invoke cancel because the listener was removed.
    shell.overlay.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    );
    expect(cancel).toBe(0);
  });
});
