/**
 * Unit tests for move-window-dialog.
 *
 * Uses happy-dom via test-setup.ts to exercise DOM interactions without
 * launching a real browser.
 */

import { afterEach, describe, expect, mock, test } from "bun:test";

// Stub i18n before importing the module under test.
mock.module("../../i18n/index.ts", () => ({
  t: (key: string) => key,
}));

import { showMoveWindowDialog } from "./move-window-dialog";

afterEach(() => {
  // Clean up any leftover overlay from a failed assertion path.
  document.querySelectorAll(".sftp-dialog-overlay").forEach((n) => n.remove());
});

function getInput(): HTMLInputElement {
  const el = document.querySelector<HTMLInputElement>(".sftp-dialog-input");
  if (!el) throw new Error("dialog input not found");
  return el;
}

function getConfirmBtn(): HTMLButtonElement {
  const el = document.querySelector<HTMLButtonElement>(".sftp-dialog-btn-confirm");
  if (!el) throw new Error("confirm button not found");
  return el;
}

function getCancelBtn(): HTMLButtonElement {
  const el = document.querySelector<HTMLButtonElement>(".sftp-dialog-btn-cancel");
  if (!el) throw new Error("cancel button not found");
  return el;
}

function getOverlay(): HTMLElement {
  const el = document.querySelector<HTMLElement>(".sftp-dialog-overlay");
  if (!el) throw new Error("overlay not found");
  return el;
}

/** Dispatch a keydown to the overlay (listener is registered with capture). */
function dispatchKey(
  key: string,
  extras: Partial<KeyboardEventInit & { keyCode: number; isComposing: boolean }> = {},
): void {
  const overlay = getOverlay();
  const ev = new KeyboardEvent("keydown", { key, bubbles: true, ...extras });
  // happy-dom honors property descriptors set after construction.
  if (extras.isComposing !== undefined) {
    Object.defineProperty(ev, "isComposing", { value: extras.isComposing });
  }
  if (extras.keyCode !== undefined) {
    Object.defineProperty(ev, "keyCode", { value: extras.keyCode });
  }
  overlay.dispatchEvent(ev);
}

describe("showMoveWindowDialog", () => {
  test("Enter with valid integer confirms with 1-origin value", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    getInput().value = "2";
    dispatchKey("Enter");
    const result = await p;
    expect(result.confirmed).toBe(true);
    expect(result.value).toBe(2);
  });

  test("Enter with non-integer cancels", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    getInput().value = "abc";
    dispatchKey("Enter");
    const result = await p;
    expect(result.confirmed).toBe(false);
    expect(result.value).toBeUndefined();
  });

  test("Enter with value < 1 cancels", async () => {
    const p = showMoveWindowDialog({ currentIndex: 2, windowCount: 3 });
    getInput().value = "0";
    dispatchKey("Enter");
    const result = await p;
    expect(result.confirmed).toBe(false);
  });

  test("Enter with value > windowCount cancels", async () => {
    const p = showMoveWindowDialog({ currentIndex: 2, windowCount: 3 });
    getInput().value = "999";
    dispatchKey("Enter");
    const result = await p;
    expect(result.confirmed).toBe(false);
  });

  test("Enter with empty input cancels", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    getInput().value = "";
    dispatchKey("Enter");
    const result = await p;
    expect(result.confirmed).toBe(false);
  });

  test("Escape cancels", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    getInput().value = "2";
    dispatchKey("Escape");
    const result = await p;
    expect(result.confirmed).toBe(false);
  });

  test("Cancel button cancels", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    getInput().value = "2";
    getCancelBtn().click();
    const result = await p;
    expect(result.confirmed).toBe(false);
  });

  test("Confirm button with valid input confirms", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    getInput().value = "3";
    getConfirmBtn().click();
    const result = await p;
    expect(result.confirmed).toBe(true);
    expect(result.value).toBe(3);
  });

  test("Confirm button with invalid input cancels", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    getInput().value = "xyz";
    getConfirmBtn().click();
    const result = await p;
    expect(result.confirmed).toBe(false);
  });

  test("Enter during IME composition does not confirm", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    getInput().value = "2";
    // IME commit-Enter: both isComposing=true and keyCode=229 must be ignored.
    dispatchKey("Enter", { isComposing: true });
    dispatchKey("Enter", { keyCode: 229 });
    // Overlay must still be present (promise not yet resolved).
    expect(document.querySelector(".sftp-dialog-overlay")).not.toBeNull();
    // Now a real Enter should confirm.
    dispatchKey("Enter");
    const result = await p;
    expect(result.confirmed).toBe(true);
    expect(result.value).toBe(2);
  });

  test("Whitespace-only input is treated as empty and cancels", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    getInput().value = "   ";
    dispatchKey("Enter");
    const result = await p;
    expect(result.confirmed).toBe(false);
  });

  test("Boundary values are accepted (1 and windowCount)", async () => {
    const p1 = showMoveWindowDialog({ currentIndex: 2, windowCount: 5 });
    getInput().value = "1";
    dispatchKey("Enter");
    const r1 = await p1;
    expect(r1.confirmed).toBe(true);
    expect(r1.value).toBe(1);

    const p2 = showMoveWindowDialog({ currentIndex: 2, windowCount: 5 });
    getInput().value = "5";
    dispatchKey("Enter");
    const r2 = await p2;
    expect(r2.confirmed).toBe(true);
    expect(r2.value).toBe(5);
  });

  test("Floating-point input is rejected", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 5 });
    getInput().value = "2.5";
    dispatchKey("Enter");
    const result = await p;
    expect(result.confirmed).toBe(false);
  });

  test("Negative integer is rejected", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 5 });
    getInput().value = "-1";
    dispatchKey("Enter");
    const result = await p;
    expect(result.confirmed).toBe(false);
  });

  test("Closing removes the overlay from DOM", async () => {
    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    expect(document.querySelectorAll(".sftp-dialog-overlay").length).toBe(1);
    dispatchKey("Escape");
    await p;
    expect(document.querySelectorAll(".sftp-dialog-overlay").length).toBe(0);
  });

  test("previously focused element is restored after close", async () => {
    const trigger = document.createElement("button");
    trigger.textContent = "open";
    document.body.appendChild(trigger);
    trigger.focus();
    expect(document.activeElement).toBe(trigger);

    const p = showMoveWindowDialog({ currentIndex: 1, windowCount: 3 });
    // Focus is moved to input while the dialog is open.
    expect(document.activeElement).not.toBe(trigger);
    dispatchKey("Escape");
    await p;
    expect(document.activeElement).toBe(trigger);

    trigger.remove();
  });
});
