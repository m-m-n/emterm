import { t } from "../../i18n/index.ts";
import { renderSubsectionHeader, renderToggle } from "../settings-components";
import type { SectionContext } from "./types";
import { DEFAULT_ACTION_BINDINGS } from "../../terminal/mux/prefix-key";

/** i18n key mapping for each mux action. */
const ACTION_I18N_KEYS: Record<string, string> = {
  detach: "settings.mux.keybind.detach",
  "new-window": "settings.mux.keybind.newWindow",
  "next-window": "settings.mux.keybind.nextWindow",
  "prev-window": "settings.mux.keybind.prevWindow",
  "rename-window": "settings.mux.keybind.renameWindow",
};

export function renderMuxSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const mux = ctx.currentSettings.mux;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.mux.title");
  panel.appendChild(header);

  // -- General subsection --
  renderSubsectionHeader(panel, t("settings.mux.general"));

  // Prefix key (keybind capture for key combo like Ctrl+B)
  renderMuxPrefixInput(panel, mux.prefix, ctx);

  // Mouse toggle
  renderToggle(
    panel,
    {
      key: "mux-mouse",
      label: t("settings.mux.mouse"),
      value: mux.mouse,
      onSave: (v) => {
        ctx.saveSetting("mux", { ...ctx.currentSettings.mux, mouse: v });
      },
    },
    ctx.addContentListener,
  );

  // -- Keybinds subsection --
  renderSubsectionHeader(panel, t("settings.mux.keybinds"));

  const keybinds = mux.keybinds ?? {};
  const grid = document.createElement("div");
  grid.className = "settings-keybind-grid";
  panel.appendChild(grid);

  for (const action of Object.keys(DEFAULT_ACTION_BINDINGS)) {
    const currentKey = keybinds[action] ?? DEFAULT_ACTION_BINDINGS[action]!;
    const i18nKey = ACTION_I18N_KEYS[action]!;

    renderMuxKeybindInput(grid, action, t(i18nKey), currentKey, ctx);
  }
}

/**
 * Renders a keybind capture button for the mux prefix key.
 * Uses the same UI pattern as the keybinds section but saves to mux.prefix.
 */
function renderMuxPrefixInput(
  panel: HTMLElement,
  currentValue: string,
  ctx: SectionContext,
): void {
  const row = document.createElement("div");
  row.className = "settings-row settings-row-keybind";

  const labelEl = document.createElement("label");
  labelEl.className = "settings-label";
  labelEl.textContent = t("settings.mux.prefix");
  row.appendChild(labelEl);

  const button = document.createElement("button");
  button.className = "settings-keybind-input";
  button.textContent = currentValue;
  row.appendChild(button);

  panel.appendChild(row);

  ctx.addContentListener(button, "click", () => {
    button.classList.add("capturing");
    button.textContent = t("settings.keybinds.pressKey");

    let captured = false;
    const keydownHandler = (e: Event) => {
      const ke = e as KeyboardEvent;
      e.preventDefault();
      e.stopPropagation();
      e.stopImmediatePropagation();

      if (ke.key === "Escape") {
        button.textContent = currentValue;
        button.classList.remove("capturing");
        document.removeEventListener("keydown", keydownHandler, true);
        captured = true;
        return;
      }

      // Ignore bare modifier keys
      if (["Control", "Shift", "Alt", "Meta"].includes(ke.key)) {
        return;
      }

      const parts: string[] = [];
      if (ke.ctrlKey) parts.push("Ctrl");
      if (ke.shiftKey) parts.push("Shift");
      if (ke.altKey) parts.push("Alt");
      if (ke.metaKey) parts.push("Meta");

      let keyName = ke.key;
      if (keyName === " ") keyName = "Space";
      else if (keyName === "+") keyName = "Plus";
      else if (keyName === "-") keyName = "Minus";
      else if (keyName.length === 1) keyName = keyName.toUpperCase();

      parts.push(keyName);
      const combo = parts.join("+");

      button.textContent = combo;
      button.classList.remove("capturing");
      document.removeEventListener("keydown", keydownHandler, true);
      captured = true;

      ctx.saveSetting("mux", { ...ctx.currentSettings.mux, prefix: combo });
    };

    document.addEventListener("keydown", keydownHandler, true);

    const cleanup = () => {
      if (!captured) {
        setTimeout(() => {
          if (!captured) {
            button.textContent = currentValue;
            button.classList.remove("capturing");
            document.removeEventListener("keydown", keydownHandler, true);
          }
        }, 100);
      }
    };
    button.addEventListener("blur", cleanup, { once: true });
  });
}

/**
 * Renders a keybind capture button for mux actions.
 * Supports both single characters and key combinations (e.g., Ctrl+N).
 */
function renderMuxKeybindInput(
  panel: HTMLElement,
  action: string,
  label: string,
  currentKey: string,
  ctx: SectionContext,
): void {
  const row = document.createElement("div");
  row.className = "settings-row settings-row-keybind";

  const labelEl = document.createElement("label");
  labelEl.className = "settings-label";
  labelEl.textContent = label;
  row.appendChild(labelEl);

  const button = document.createElement("button");
  button.className = "settings-keybind-input";
  button.textContent = currentKey;
  row.appendChild(button);

  panel.appendChild(row);

  ctx.addContentListener(button, "click", () => {
    button.classList.add("capturing");
    button.textContent = t("settings.keybinds.pressKey");

    let captured = false;
    const keydownHandler = (e: Event) => {
      const ke = e as KeyboardEvent;
      // Prevent browser shortcuts (Ctrl+N, Ctrl+P, etc.) from firing
      e.preventDefault();
      e.stopPropagation();
      e.stopImmediatePropagation();

      if (ke.key === "Escape") {
        button.textContent = currentKey;
        button.classList.remove("capturing");
        document.removeEventListener("keydown", keydownHandler, true);
        captured = true;
        return;
      }

      // Ignore bare modifier keys
      if (["Control", "Shift", "Alt", "Meta"].includes(ke.key)) {
        return;
      }

      const parts: string[] = [];
      if (ke.ctrlKey) parts.push("Ctrl");
      if (ke.shiftKey) parts.push("Shift");
      if (ke.altKey) parts.push("Alt");
      if (ke.metaKey) parts.push("Meta");

      let keyName = ke.key;
      if (keyName === " ") keyName = "Space";
      else if (keyName === "+") keyName = "Plus";
      else if (keyName === "-") keyName = "Minus";
      else if (keyName.length === 1 && parts.length > 0)
        keyName = keyName.toUpperCase();

      parts.push(keyName);
      const combo = parts.join("+");

      button.textContent = combo;
      button.classList.remove("capturing");
      document.removeEventListener("keydown", keydownHandler, true);
      captured = true;

      const newKeybinds = {
        ...ctx.currentSettings.mux.keybinds,
        [action]: combo,
      };
      ctx.saveSetting("mux", {
        ...ctx.currentSettings.mux,
        keybinds: newKeybinds,
      });
    };

    document.addEventListener("keydown", keydownHandler, true);

    // Cleanup on blur only if no key was captured yet
    // (prevents premature removal when focus shifts during key press)
    const cleanup = () => {
      if (!captured) {
        // Delay cleanup to allow keydown to fire first
        setTimeout(() => {
          if (!captured) {
            button.textContent = currentKey;
            button.classList.remove("capturing");
            document.removeEventListener("keydown", keydownHandler, true);
          }
        }, 100);
      }
    };
    button.addEventListener("blur", cleanup, { once: true });
  });
}
