/**
 * Keybind Editor
 *
 * Handles keybind input rendering and key capture mode.
 * Captures keyboard shortcuts and saves them to settings.
 */

import { SettingsService } from "./settings-service";
import type { AppSettings } from "./types";
import { t } from "../i18n/index.ts";
import type { AddListenerFn } from "./settings-components";

// ============================================================
// Keybind Capture State
// ============================================================

export interface KeybindCaptureState {
  capturingKeybindButton: HTMLButtonElement | null;
  capturingKeybindKey: string | null;
  capturingOriginalValue: string | null;
}

export interface KeybindEditorContext {
  state: KeybindCaptureState;
  eventListeners: Array<{
    element: EventTarget;
    type: string;
    handler: EventListener;
  }>;
  currentSettings: AppSettings | null;
}

export function createKeybindCaptureState(): KeybindCaptureState {
  return {
    capturingKeybindButton: null,
    capturingKeybindKey: null,
    capturingOriginalValue: null,
  };
}

// ============================================================
// Renderers
// ============================================================

export function renderKeybindInput(
  panel: HTMLElement,
  keybindKey: string,
  label: string,
  currentValue: string,
  addListener: AddListenerFn,
  ctx: KeybindEditorContext,
): void {
  const row = document.createElement("div");
  row.className = "settings-row settings-row-keybind";

  const labelEl = document.createElement("label");
  labelEl.className = "settings-label";
  labelEl.textContent = label;
  row.appendChild(labelEl);

  const button = document.createElement("button");
  button.className = "settings-keybind-input";
  button.dataset.key = keybindKey;
  button.textContent = currentValue;
  row.appendChild(button);

  panel.appendChild(row);

  // Click to enter capture mode
  addListener(button, "click", () => {
    enterKeybindCapture(button, keybindKey, currentValue, ctx);
  });
}

// ============================================================
// Keybind Capture
// ============================================================

export function enterKeybindCapture(
  button: HTMLButtonElement,
  key: string,
  originalValue: string,
  ctx: KeybindEditorContext,
): void {
  // Cancel any existing capture
  if (ctx.state.capturingKeybindButton) {
    exitKeybindCapture(true, ctx);
  }

  ctx.state.capturingKeybindButton = button;
  ctx.state.capturingKeybindKey = key;
  ctx.state.capturingOriginalValue = originalValue;

  button.classList.add("capturing");
  button.textContent = t("settings.keybinds.pressKey");
  button.focus();

  // Capture keydown
  const keydownHandler = (e: Event) => {
    const ke = e as KeyboardEvent;
    e.preventDefault();
    e.stopPropagation();

    // Escape cancels
    if (ke.key === "Escape") {
      exitKeybindCapture(true, ctx);
      return;
    }

    // Ignore bare modifier keys
    if (["Control", "Shift", "Alt", "Meta"].includes(ke.key)) {
      return;
    }

    // Build key combination string
    const parts: string[] = [];
    if (ke.ctrlKey) parts.push("Ctrl");
    if (ke.shiftKey) parts.push("Shift");
    if (ke.altKey) parts.push("Alt");
    if (ke.metaKey) parts.push("Meta");

    // Normalize key name
    let keyName = ke.key;
    if (keyName === " ") keyName = "Space";
    else if (keyName === "+") keyName = "Plus";
    else if (keyName === "-") keyName = "Minus";
    else if (keyName.length === 1) keyName = keyName.toUpperCase();

    parts.push(keyName);
    const combo = parts.join("+");

    // Update button and save
    button.textContent = combo;
    saveKeybind(key, combo, ctx);
    exitKeybindCapture(false, ctx);
  };

  // Use capture phase to intercept before other handlers
  document.addEventListener("keydown", keydownHandler, true);

  // Store cleanup
  ctx.eventListeners.push({
    element: document,
    type: "keydown",
    handler: keydownHandler,
  });
}

export function exitKeybindCapture(cancelled: boolean, ctx: KeybindEditorContext): void {
  if (!ctx.state.capturingKeybindButton) return;

  if (cancelled && ctx.state.capturingOriginalValue !== null) {
    ctx.state.capturingKeybindButton.textContent = ctx.state.capturingOriginalValue;
  }

  ctx.state.capturingKeybindButton.classList.remove("capturing");
  ctx.state.capturingKeybindButton = null;
  ctx.state.capturingKeybindKey = null;
  ctx.state.capturingOriginalValue = null;

  // Remove the capture keydown listener
  const newListeners = ctx.eventListeners.filter((listener) => {
    if (listener.element === document && listener.type === "keydown") {
      listener.element.removeEventListener(listener.type, listener.handler, true);
      return false;
    }
    return true;
  });
  ctx.eventListeners.length = 0;
  ctx.eventListeners.push(...newListeners);
}

async function saveKeybind(key: string, value: string, ctx: KeybindEditorContext): Promise<void> {
  if (!ctx.currentSettings) return;
  (ctx.currentSettings.keybinds as any)[key] = value;
  try {
    await SettingsService.save(ctx.currentSettings);
  } catch (error) {
    console.error("Failed to save keybind:", error);
  }
}
