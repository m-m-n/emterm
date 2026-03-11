import { t } from "../../i18n/index.ts";
import { renderSubsectionHeader } from "../settings-components";
import { renderKeybindInput } from "../keybind-editor";
import type { SectionContext } from "./types";

export function renderKeybindsSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const kb = ctx.currentSettings.keybinds;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.keybinds.title");
  panel.appendChild(header);

  // -- Basic subsection --
  renderSubsectionHeader(panel, t("settings.keybinds.basic"));
  const basicGrid = createKeybindGrid(panel);
  renderKeybindInput(
    basicGrid,
    "copy",
    t("settings.keybinds.copy"),
    kb.copy,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    basicGrid,
    "paste",
    t("settings.keybinds.paste"),
    kb.paste,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  // -- Tab Management subsection --
  renderSubsectionHeader(panel, t("settings.keybinds.tabManagement"));
  const tabGrid = createKeybindGrid(panel);
  renderKeybindInput(
    tabGrid,
    "new_tab",
    t("settings.keybinds.newTab"),
    kb.new_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    tabGrid,
    "close_tab",
    t("settings.keybinds.closeTab"),
    kb.close_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    tabGrid,
    "next_tab",
    t("settings.keybinds.nextTab"),
    kb.next_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    tabGrid,
    "prev_tab",
    t("settings.keybinds.prevTab"),
    kb.prev_tab,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
  renderKeybindInput(
    tabGrid,
    "profile_selector",
    t("settings.keybinds.profileSelector"),
    kb.profile_selector,
    ctx.addContentListener,
    ctx.keybindCtx,
  );

  // -- Settings subsection --
  renderSubsectionHeader(panel, t("settings.keybinds.settingsSection"));
  const settingsGrid = createKeybindGrid(panel);
  renderKeybindInput(
    settingsGrid,
    "open_settings",
    t("settings.keybinds.openSettings"),
    kb.open_settings,
    ctx.addContentListener,
    ctx.keybindCtx,
  );
}

/**
 * Creates a keybind grid container and appends it to the panel
 */
function createKeybindGrid(panel: HTMLElement): HTMLElement {
  const grid = document.createElement("div");
  grid.className = "settings-keybind-grid";
  panel.appendChild(grid);
  return grid;
}
