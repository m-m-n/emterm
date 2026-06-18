import type { Profile } from "../types";
import { t } from "../../i18n/index.ts";
import { renderSubsectionHeader } from "../settings-components";
import { showProfileEditor } from "../../profile/profile-editor";
import { duplicateProfile, ensureSingleDefault } from "../../profile/types";
import type { SectionContext } from "./types";

export function renderProfilesSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  renderSubsectionHeader(panel, t("settings.profiles.title"));

  const profiles = ctx.currentSettings.profiles;

  // Add Profile button
  const addBtn = document.createElement("button");
  addBtn.className = "settings-button profile-add-btn";
  addBtn.textContent = t("settings.profiles.addProfile");
  ctx.addContentListener(addBtn, "click", () => {
    showProfileEditor({
      onSave: (profile: Profile) => {
        ctx.currentSettings.profiles = [
          ...ctx.currentSettings.profiles,
          profile,
        ];
        ctx.saveSetting("profiles", ctx.currentSettings.profiles);
        ctx.reRender();
      },
      onCancel: () => {},
    });
  });
  panel.appendChild(addBtn);

  // Empty state
  if (profiles.length === 0) {
    const empty = document.createElement("p");
    empty.className = "profile-empty-state";
    empty.textContent = t("settings.profiles.noProfiles");
    panel.appendChild(empty);
    return;
  }

  // Profile list
  const list = document.createElement("div");
  list.className = "profile-list";
  list.setAttribute("role", "list");

  for (let i = 0; i < profiles.length; i++) {
    const profile = profiles[i]!;
    const item = document.createElement("div");
    item.className = "profile-list-item";
    item.setAttribute("role", "listitem");
    item.draggable = true;
    item.dataset.index = String(i);

    // Drag handle
    const dragHandle = document.createElement("span");
    dragHandle.className = "profile-drag-handle";
    dragHandle.textContent = "\u283F";
    dragHandle.title = t("settings.profiles.dragHandle");
    item.appendChild(dragHandle);

    // Info area
    const info = document.createElement("div");
    info.className = "profile-item-info";

    const nameEl = document.createElement("span");
    nameEl.className = "profile-item-name";
    nameEl.textContent = profile.name;
    info.appendChild(nameEl);

    if (profile.is_default) {
      const badge = document.createElement("span");
      badge.className = "profile-default-badge";
      badge.textContent = t("settings.profiles.defaultBadge");
      info.appendChild(badge);
    }

    if (profile.ssh_connection_name) {
      const sshEl = document.createElement("span");
      sshEl.className = "profile-item-shell";
      sshEl.textContent = `SSH: ${profile.ssh_connection_name}`;
      info.appendChild(sshEl);
    } else if (profile.shell_path) {
      const shellEl = document.createElement("span");
      shellEl.className = "profile-item-shell";
      shellEl.textContent = profile.shell_path;
      info.appendChild(shellEl);
    }

    item.appendChild(info);

    // Action buttons
    const actions = document.createElement("div");
    actions.className = "profile-item-actions";

    // Default toggle
    const defaultBtn = createActionButton(
      profile.is_default
        ? t("settings.profiles.unsetDefault")
        : t("settings.profiles.setDefault"),
      () => {
        if (profile.is_default) {
          ensureSingleDefault(ctx.currentSettings.profiles, -1);
        } else {
          ensureSingleDefault(ctx.currentSettings.profiles, i);
        }
        ctx.saveSetting("profiles", ctx.currentSettings.profiles);
        ctx.reRender();
      },
    );
    defaultBtn.className += profile.is_default ? " profile-btn-active" : "";
    actions.appendChild(defaultBtn);

    // Edit
    actions.appendChild(
      createActionButton(t("settings.profiles.edit"), () => {
        showProfileEditor({
          profile,
          onSave: (updated: Profile) => {
            ctx.currentSettings.profiles[i] = updated;
            ctx.saveSetting("profiles", [...ctx.currentSettings.profiles]);
            ctx.reRender();
          },
          onCancel: () => {},
        });
      }),
    );

    // Duplicate
    actions.appendChild(
      createActionButton(t("settings.profiles.duplicate"), () => {
        const copy = duplicateProfile(profile);
        ctx.currentSettings.profiles = [
          ...ctx.currentSettings.profiles.slice(0, i + 1),
          copy,
          ...ctx.currentSettings.profiles.slice(i + 1),
        ];
        ctx.saveSetting("profiles", ctx.currentSettings.profiles);
        ctx.reRender();
      }),
    );

    // Delete
    actions.appendChild(
      createActionButton(t("settings.profiles.delete"), () => {
        ctx.currentSettings.profiles = ctx.currentSettings.profiles.filter(
          (_, idx) => idx !== i,
        );
        ctx.saveSetting("profiles", ctx.currentSettings.profiles);
        ctx.reRender();
      }),
    );

    // Launch (open new tab with this profile)
    actions.appendChild(
      createActionButton(t("settings.profiles.launch"), () => {
        document.dispatchEvent(
          new CustomEvent("profile:launch", { detail: profile }),
        );
      }),
    );

    item.appendChild(actions);
    list.appendChild(item);
  }

  // Drag and drop reorder
  setupDragReorder(list, ctx);

  panel.appendChild(list);
}

function createActionButton(
  label: string,
  onClick: () => void,
): HTMLButtonElement {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "profile-action-btn";
  btn.textContent = label;
  btn.addEventListener("click", onClick);
  return btn;
}

function setupDragReorder(list: HTMLElement, ctx: SectionContext): void {
  let dragIndex: number | null = null;

  ctx.addContentListener(list, "dragstart", ((e: DragEvent) => {
    const item = (e.target as HTMLElement).closest(
      ".profile-list-item",
    ) as HTMLElement | null;
    if (!item) return;
    dragIndex = Number(item.dataset.index);
    item.classList.add("dragging");
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
    }
  }) as EventListener);

  ctx.addContentListener(list, "dragover", ((e: DragEvent) => {
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = "move";
    }
  }) as EventListener);

  ctx.addContentListener(list, "dragend", ((e: DragEvent) => {
    const item = (e.target as HTMLElement).closest(
      ".profile-list-item",
    ) as HTMLElement | null;
    if (item) item.classList.remove("dragging");
    dragIndex = null;
  }) as EventListener);

  ctx.addContentListener(list, "drop", ((e: DragEvent) => {
    e.preventDefault();
    if (dragIndex === null) return;

    const target = (e.target as HTMLElement).closest(
      ".profile-list-item",
    ) as HTMLElement | null;
    if (!target) return;

    const dropIndex = Number(target.dataset.index);
    if (dragIndex === dropIndex) return;

    const profiles = [...ctx.currentSettings.profiles];
    const [moved] = profiles.splice(dragIndex, 1);
    if (!moved) return;
    profiles.splice(dropIndex, 0, moved);
    ctx.currentSettings.profiles = profiles;
    ctx.saveSetting("profiles", profiles);
    ctx.reRender();
  }) as EventListener);
}
