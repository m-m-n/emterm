/**
 * Font Picker
 *
 * Font selection overlay that replaces the settings content area
 * with a searchable font list. Renders into the provided content element.
 */

import { FontService } from "./font-service";
import type { FontCategory } from "./types";
import { t } from "../i18n/index.ts";
import type { AddListenerFn } from "./settings-components";

// ============================================================
// Option Types
// ============================================================

export interface FontPickerInputOptions {
  key: string;
  label: string;
  value: string;
  placeholder: string;
  hint: string;
  description?: string;
  category: FontCategory;
  onSelect: (value: string) => void;
}

export interface FontPickerContext {
  contentElement: HTMLElement;
  navElement: HTMLElement | null;
  addContentListener: AddListenerFn;
  detachContentListeners: () => void;
  renderContent: () => void;
}

// ============================================================
// Renderers
// ============================================================

export function renderFontPickerInput(
  panel: HTMLElement,
  opts: FontPickerInputOptions,
  addListener: AddListenerFn,
  onChangeClick: (
    category: FontCategory,
    currentValue: string,
    onSelect: (value: string) => void,
  ) => void,
): void {
  const row = document.createElement("div");
  row.className = "settings-row";

  const label = document.createElement("label");
  label.className = "settings-label";
  label.htmlFor = `settings-${opts.key}`;
  label.textContent = opts.label;
  row.appendChild(label);

  if (opts.description) {
    const desc = document.createElement("span");
    desc.className = "settings-description";
    desc.id = `settings-${opts.key}-desc`;
    desc.textContent = opts.description;
    row.appendChild(desc);
  }

  const inputGroup = document.createElement("div");
  inputGroup.className = "settings-font-picker-group";

  const input = document.createElement("input");
  input.type = "text";
  input.id = `settings-${opts.key}`;
  input.className = "settings-font-picker-input";
  input.value = opts.value;
  input.placeholder = opts.placeholder;
  input.readOnly = true;
  if (opts.description) {
    input.setAttribute("aria-describedby", `settings-${opts.key}-desc`);
  }
  inputGroup.appendChild(input);

  const clearBtn = document.createElement("button");
  clearBtn.className = "settings-font-picker-clear";
  clearBtn.setAttribute("aria-label", t("settings.appearance.fontPickerClear"));
  clearBtn.textContent = "\u00d7";
  clearBtn.type = "button";
  clearBtn.style.display = opts.value ? "" : "none";
  inputGroup.appendChild(clearBtn);

  const changeBtn = document.createElement("button");
  changeBtn.className = "settings-font-picker-button";
  changeBtn.textContent = t("settings.appearance.fontPickerChange");
  changeBtn.type = "button";
  inputGroup.appendChild(changeBtn);

  row.appendChild(inputGroup);

  const hint = document.createElement("span");
  hint.className = "settings-hint";
  hint.textContent = opts.hint;
  row.appendChild(hint);

  panel.appendChild(row);

  // Clear button resets font value
  addListener(clearBtn, "click", () => {
    input.value = "";
    clearBtn.style.display = "none";
    opts.onSelect("");
  });

  // Button click opens font picker
  addListener(changeBtn, "click", () => {
    onChangeClick(opts.category, input.value, (selectedFont) => {
      input.value = selectedFont;
      clearBtn.style.display = selectedFont ? "" : "none";
      opts.onSelect(selectedFont);
    });
  });
}

// ============================================================
// Font Picker Overlay
// ============================================================

export async function showFontPicker(
  category: FontCategory,
  currentValue: string,
  onSelect: (value: string) => void,
  ctx: FontPickerContext,
): Promise<void> {
  if (!ctx.contentElement) return;

  // Detach current content listeners
  ctx.detachContentListeners();

  // Clear content area
  ctx.contentElement.innerHTML = "";

  // Disable navigation tabs
  setNavTabsEnabled(ctx.navElement, false);

  // Get title based on category
  const titleMap: Record<FontCategory, string> = {
    primary: t("settings.appearance.fontPickerPrimaryTitle"),
    secondary: t("settings.appearance.fontPickerSecondaryTitle"),
    ui: t("settings.ui.fontPickerUiTitle"),
    "markdown-body": t("settings.markdownViewer.fontPickerBodyTitle"),
    "markdown-code": t("settings.markdownViewer.fontPickerCodeTitle"),
  };

  // Load fonts
  let fontList: string[] = [];
  try {
    const fonts = await FontService.list();
    switch (category) {
      case "primary":
      case "markdown-code":
        fontList = fonts.monospace_fonts;
        break;
      case "secondary":
      case "ui":
      case "markdown-body":
        fontList = fonts.all_fonts;
        break;
    }
  } catch (err) {
    console.error("Failed to load fonts:", err);
  }

  // Build font picker UI
  const picker = document.createElement("div");
  picker.className = "font-picker";

  // Header
  const header = document.createElement("div");
  header.className = "font-picker-header";

  const backBtn = document.createElement("button");
  backBtn.className = "font-picker-back";
  backBtn.setAttribute("aria-label", t("settings.appearance.fontPickerBack"));
  backBtn.textContent = "\u2190"; // Left arrow
  backBtn.type = "button";
  header.appendChild(backBtn);

  const title = document.createElement("h3");
  title.className = "font-picker-title";
  title.textContent = titleMap[category];
  header.appendChild(title);

  picker.appendChild(header);

  // Search
  const searchContainer = document.createElement("div");
  searchContainer.className = "font-picker-search";

  const searchInput = document.createElement("input");
  searchInput.type = "text";
  searchInput.className = "font-picker-search-input";
  searchInput.placeholder = t("settings.appearance.fontPickerSearch");
  searchInput.setAttribute(
    "aria-label",
    t("settings.appearance.fontPickerSearch"),
  );
  searchContainer.appendChild(searchInput);

  picker.appendChild(searchContainer);

  // Font list container
  const listContainer = document.createElement("div");
  listContainer.className = "font-picker-list";
  listContainer.setAttribute("role", "listbox");
  listContainer.setAttribute("aria-label", titleMap[category]);
  picker.appendChild(listContainer);

  ctx.contentElement.appendChild(picker);

  // Render the font list
  const renderList = (fonts: string[]) => {
    listContainer.innerHTML = "";

    if (fonts.length === 0) {
      const noResults = document.createElement("div");
      noResults.className = "font-picker-no-results";
      noResults.textContent = t("settings.appearance.fontPickerNoResults");
      listContainer.appendChild(noResults);
      return;
    }

    for (const fontName of fonts) {
      const item = document.createElement("div");
      item.className = "font-picker-item";
      item.setAttribute("role", "option");
      item.setAttribute("tabindex", "-1");
      item.style.fontFamily = `'${fontName.replace(/'/g, "\\'")}', sans-serif`;
      item.textContent = fontName;

      const isSelected = fontName === currentValue;
      item.setAttribute("aria-selected", String(isSelected));
      if (isSelected) {
        item.setAttribute("tabindex", "0");
      }

      listContainer.appendChild(item);
    }

    // Scroll selected item into view
    const selectedItem = listContainer.querySelector('[aria-selected="true"]');
    if (selectedItem) {
      selectedItem.scrollIntoView({ block: "center" });
    }
  };

  renderList(fontList);

  const hidePicker = () => {
    hideFontPicker(ctx);
  };

  // Event: back button
  ctx.addContentListener(backBtn, "click", () => {
    hidePicker();
  });

  // Event: search input
  ctx.addContentListener(searchInput, "input", () => {
    const filtered = filterFontList(searchInput.value, fontList);
    renderList(filtered);
  });

  // Event: font list click
  ctx.addContentListener(listContainer, "click", (e: Event) => {
    const target = e.target as HTMLElement;
    if (target.classList.contains("font-picker-item")) {
      const fontName = target.textContent || "";
      onSelect(fontName);
      hidePicker();
    }
  });

  // Event: keyboard navigation on list
  ctx.addContentListener(listContainer, "keydown", (e: Event) => {
    const ke = e as KeyboardEvent;
    const target = ke.target as HTMLElement;
    if (!target.classList.contains("font-picker-item")) return;

    switch (ke.key) {
      case "ArrowDown": {
        ke.preventDefault();
        const next = target.nextElementSibling as HTMLElement | null;
        if (next && next.classList.contains("font-picker-item")) {
          target.setAttribute("tabindex", "-1");
          next.setAttribute("tabindex", "0");
          next.focus();
        }
        break;
      }
      case "ArrowUp": {
        ke.preventDefault();
        const prev = target.previousElementSibling as HTMLElement | null;
        if (prev && prev.classList.contains("font-picker-item")) {
          target.setAttribute("tabindex", "-1");
          prev.setAttribute("tabindex", "0");
          prev.focus();
        }
        break;
      }
      case "Enter": {
        ke.preventDefault();
        const fontName = target.textContent || "";
        onSelect(fontName);
        hidePicker();
        break;
      }
      case "Escape": {
        ke.preventDefault();
        hidePicker();
        break;
      }
    }
  });

  // Event: Escape on search input
  ctx.addContentListener(searchInput, "keydown", (e: Event) => {
    if ((e as KeyboardEvent).key === "Escape") {
      e.preventDefault();
      hidePicker();
    }
  });

  // Focus search input
  searchInput.focus();
}

export function hideFontPicker(ctx: FontPickerContext): void {
  // Re-enable navigation tabs
  setNavTabsEnabled(ctx.navElement, true);

  // Detach font picker listeners
  ctx.detachContentListeners();

  // Restore settings view
  ctx.renderContent();
}

export function setNavTabsEnabled(
  navElement: HTMLElement | null,
  enabled: boolean,
): void {
  if (!navElement) return;
  const tabs = navElement.querySelectorAll(".settings-nav-item");
  for (const tab of tabs) {
    if (enabled) {
      tab.classList.remove("disabled");
      (tab as HTMLButtonElement).disabled = false;
      tab.removeAttribute("aria-disabled");
    } else {
      tab.classList.add("disabled");
      (tab as HTMLButtonElement).disabled = true;
      tab.setAttribute("aria-disabled", "true");
    }
  }
}

export function filterFontList(searchText: string, fonts: string[]): string[] {
  if (!searchText) return fonts;
  const lower = searchText.toLowerCase();
  return fonts.filter((name) => name.toLowerCase().includes(lower));
}
