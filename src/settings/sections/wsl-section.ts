import { invoke } from "@tauri-apps/api/core";
import type { WslDistribution } from "../types";
import { t } from "../../i18n/index.ts";
import { renderSubsectionHeader } from "../settings-components";
import type { SectionContext } from "./types";

export function renderWslSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.wsl.title");
  panel.appendChild(header);

  // -- Detected WSL Distributions --
  renderSubsectionHeader(panel, t("settings.wsl.detected"));

  const detectedDesc = document.createElement("p");
  detectedDesc.className = "settings-item-description ssh-config-desc";
  detectedDesc.textContent = t("settings.wsl.detectedDesc");
  panel.appendChild(detectedDesc);

  const detectedList = document.createElement("div");
  detectedList.className = "ssh-config-list";
  detectedList.setAttribute("role", "list");
  panel.appendChild(detectedList);

  // Load detected distributions asynchronously
  invoke<string[]>("detect_wsl_distributions").then((distros) => {
    if (distros.length === 0) {
      const empty = document.createElement("p");
      empty.className = "ssh-empty-state";
      empty.textContent = t("settings.wsl.detectedEmpty");
      detectedList.appendChild(empty);
      return;
    }

    const importedNames = new Set(
      settings.wsl_distributions.map((d) => d.name),
    );

    for (const distro of distros) {
      const item = document.createElement("div");
      item.className = "ssh-config-item";
      item.setAttribute("role", "listitem");

      const nameEl = document.createElement("span");
      nameEl.className = "ssh-config-item-name";
      nameEl.textContent = distro;
      item.appendChild(nameEl);

      const importBtn = document.createElement("button");
      importBtn.type = "button";
      importBtn.className = "profile-action-btn";
      importBtn.textContent = t("settings.wsl.import");

      if (importedNames.has(distro)) {
        importBtn.disabled = true;
        importBtn.title = t("settings.wsl.alreadyImported");
      }

      ctx.addContentListener(importBtn, "click", () => {
        const newDist: WslDistribution = { name: distro };
        ctx.currentSettings.wsl_distributions = [
          ...ctx.currentSettings.wsl_distributions,
          newDist,
        ];
        ctx.saveSetting("wsl_distributions", ctx.currentSettings.wsl_distributions);
        ctx.reRender();
      });
      item.appendChild(importBtn);

      detectedList.appendChild(item);
    }
  }).catch((err) => {
    console.warn("Failed to detect WSL distributions:", err);
    const errEl = document.createElement("p");
    errEl.className = "ssh-empty-state";
    errEl.textContent = t("settings.wsl.detectedEmpty");
    detectedList.appendChild(errEl);
  });

  // -- Imported WSL Distributions --
  renderSubsectionHeader(panel, t("settings.wsl.imported"));

  const distributions = settings.wsl_distributions;

  if (distributions.length === 0) {
    const empty = document.createElement("p");
    empty.className = "ssh-empty-state";
    empty.textContent = t("settings.wsl.noDistributions");
    panel.appendChild(empty);
  } else {
    const list = document.createElement("div");
    list.className = "profile-list";
    list.setAttribute("role", "list");

    for (let i = 0; i < distributions.length; i++) {
      const dist = distributions[i]!;
      const item = document.createElement("div");
      item.className = "profile-list-item";
      item.setAttribute("role", "listitem");

      const info = document.createElement("div");
      info.className = "profile-item-info";

      const nameEl = document.createElement("span");
      nameEl.className = "profile-item-name";
      nameEl.textContent = dist.name;
      info.appendChild(nameEl);

      item.appendChild(info);

      // Delete button
      const actions = document.createElement("div");
      actions.className = "profile-item-actions";

      const deleteBtn = document.createElement("button");
      deleteBtn.type = "button";
      deleteBtn.className = "profile-action-btn";
      deleteBtn.textContent = t("settings.wsl.delete");
      deleteBtn.addEventListener("click", () => {
        ctx.currentSettings.wsl_distributions = ctx.currentSettings.wsl_distributions.filter(
          (_, idx) => idx !== i,
        );
        ctx.saveSetting("wsl_distributions", ctx.currentSettings.wsl_distributions);
        ctx.reRender();
      });
      actions.appendChild(deleteBtn);

      item.appendChild(actions);
      list.appendChild(item);
    }

    panel.appendChild(list);
  }
}
