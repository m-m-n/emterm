import { invoke } from "@tauri-apps/api/core";
import type { SshConnection, SshConfigHost } from "../types";
import { t } from "../../i18n/index.ts";
import {
  renderSubsectionHeader,
  renderNumberInput,
} from "../settings-components";
import { showSshEditor } from "../../ssh/ssh-editor";
import type { SectionContext } from "./types";

export function renderSshSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.ssh.title");
  panel.appendChild(header);

  // -- SSH Command Path --
  renderSubsectionHeader(panel, t("settings.ssh.commandPath"));

  // SSH command path text input with detect button
  const pathRow = document.createElement("div");
  pathRow.className = "settings-item";

  const pathLabel = document.createElement("label");
  pathLabel.className = "settings-item-label";
  pathLabel.textContent = t("settings.ssh.commandPath");
  pathRow.appendChild(pathLabel);

  const pathDesc = document.createElement("span");
  pathDesc.className = "settings-item-description";
  pathDesc.textContent = t("settings.ssh.commandPathDesc");
  pathRow.appendChild(pathDesc);

  const pathInputRow = document.createElement("div");
  pathInputRow.className = "ssh-path-row";

  const pathInput = document.createElement("input");
  pathInput.type = "text";
  pathInput.className = "settings-text-input";
  pathInput.value = settings.ssh_command_path;
  pathInput.placeholder = t("settings.ssh.commandPathPlaceholder");
  pathInputRow.appendChild(pathInput);

  const detectBtn = document.createElement("button");
  detectBtn.type = "button";
  detectBtn.className = "settings-button ssh-detect-btn";
  detectBtn.textContent = t("settings.ssh.detect");
  pathInputRow.appendChild(detectBtn);

  pathRow.appendChild(pathInputRow);

  const pathHint = document.createElement("span");
  pathHint.className = "settings-item-hint";
  pathHint.textContent = t("settings.ssh.commandPathHint");
  pathRow.appendChild(pathHint);

  panel.appendChild(pathRow);

  // Path input save on blur
  ctx.addContentListener(pathInput, "change", () => {
    ctx.saveSetting("ssh_command_path", pathInput.value.trim());
  });

  // Detect button
  ctx.addContentListener(detectBtn, "click", async () => {
    detectBtn.textContent = t("settings.ssh.detecting");
    detectBtn.disabled = true;
    try {
      const path = await invoke<string>("detect_ssh_command");
      if (path) {
        pathInput.value = path;
        ctx.saveSetting("ssh_command_path", path);
        pathHint.textContent = t("settings.ssh.detectSuccess", { path });
        ctx.reRender();
      } else {
        pathHint.textContent = t("settings.ssh.detectNotFound");
      }
    } catch (err) {
      pathHint.textContent = t("settings.ssh.detectNotFound");
      console.warn("SSH detect failed:", err);
    } finally {
      detectBtn.textContent = t("settings.ssh.detect");
      detectBtn.disabled = false;
    }
  });

  // -- eMterm SSH Connections --
  renderSubsectionHeader(panel, t("settings.ssh.connections"));

  // Add Connection button
  const addBtn = document.createElement("button");
  addBtn.className = "settings-button profile-add-btn";
  addBtn.textContent = t("settings.ssh.addConnection");
  ctx.addContentListener(addBtn, "click", () => {
    showSshEditor({
      onSave: (conn: SshConnection) => {
        ctx.currentSettings.ssh_connections = [
          ...ctx.currentSettings.ssh_connections,
          conn,
        ];
        ctx.saveSetting("ssh_connections", ctx.currentSettings.ssh_connections);
        ctx.reRender();
      },
      onCancel: () => {},
    });
  });
  panel.appendChild(addBtn);

  const connections = settings.ssh_connections;

  // Empty state
  if (connections.length === 0) {
    const empty = document.createElement("p");
    empty.className = "ssh-empty-state";
    empty.textContent = t("settings.ssh.noConnections");
    panel.appendChild(empty);
  } else {
    // Connection list
    const list = document.createElement("div");
    list.className = "profile-list";
    list.setAttribute("role", "list");

    for (let i = 0; i < connections.length; i++) {
      const conn = connections[i]!;
      const item = document.createElement("div");
      item.className = "profile-list-item";
      item.setAttribute("role", "listitem");
      item.draggable = true;
      item.dataset.index = String(i);

      // Drag handle
      const dragHandle = document.createElement("span");
      dragHandle.className = "profile-drag-handle";
      dragHandle.textContent = "\u283F";
      dragHandle.title = t("settings.ssh.dragHandle");
      item.appendChild(dragHandle);

      // Info area
      const info = document.createElement("div");
      info.className = "profile-item-info";

      const nameEl = document.createElement("span");
      nameEl.className = "profile-item-name";
      nameEl.textContent = conn.name;
      info.appendChild(nameEl);

      const hostEl = document.createElement("span");
      hostEl.className = "profile-item-shell";
      const hostText = conn.username
        ? `${conn.username}@${conn.hostname}`
        : conn.hostname;
      hostEl.textContent =
        conn.port !== 22 ? `${hostText}:${conn.port}` : hostText;
      info.appendChild(hostEl);

      item.appendChild(info);

      // Action buttons
      const actions = document.createElement("div");
      actions.className = "profile-item-actions";

      // Edit
      actions.appendChild(
        createSshActionButton(t("settings.ssh.edit"), () => {
          showSshEditor({
            connection: conn,
            onSave: (updated: SshConnection) => {
              ctx.currentSettings.ssh_connections[i] = updated;
              ctx.saveSetting("ssh_connections", [
                ...ctx.currentSettings.ssh_connections,
              ]);
              ctx.reRender();
            },
            onCancel: () => {},
          });
        }),
      );

      // Duplicate
      actions.appendChild(
        createSshActionButton(t("settings.ssh.duplicate"), () => {
          const existingNames = new Set(
            ctx.currentSettings.ssh_connections.map((c) => c.name),
          );
          let copyName = `${conn.name} (Copy)`;
          let counter = 2;
          while (existingNames.has(copyName)) {
            copyName = `${conn.name} (Copy ${counter})`;
            counter++;
          }
          const copy: SshConnection = {
            ...conn,
            name: copyName,
          };
          ctx.currentSettings.ssh_connections = [
            ...ctx.currentSettings.ssh_connections.slice(0, i + 1),
            copy,
            ...ctx.currentSettings.ssh_connections.slice(i + 1),
          ];
          ctx.saveSetting(
            "ssh_connections",
            ctx.currentSettings.ssh_connections,
          );
          ctx.reRender();
        }),
      );

      // Delete
      actions.appendChild(
        createSshActionButton(t("settings.ssh.delete"), () => {
          ctx.currentSettings.ssh_connections =
            ctx.currentSettings.ssh_connections.filter((_, idx) => idx !== i);
          ctx.saveSetting(
            "ssh_connections",
            ctx.currentSettings.ssh_connections,
          );
          ctx.reRender();
        }),
      );

      item.appendChild(actions);
      list.appendChild(item);
    }

    // Drag and drop reorder
    setupSshDragReorder(list, ctx);

    panel.appendChild(list);
  } // end of connections.length > 0

  // -- .ssh/config Hosts (read-only) --
  if (settings.ssh_command_path) {
    renderSubsectionHeader(panel, t("settings.ssh.configHosts"));

    const configDesc = document.createElement("p");
    configDesc.className = "settings-item-description ssh-config-desc";
    configDesc.textContent = t("settings.ssh.configHostsDesc");
    panel.appendChild(configDesc);

    const configList = document.createElement("div");
    configList.className = "ssh-config-list";
    configList.setAttribute("role", "list");
    panel.appendChild(configList);

    // Load hosts asynchronously (now returns SshConfigHost[])
    invoke<SshConfigHost[]>("load_ssh_config_hosts")
      .then((hosts) => {
        if (hosts.length === 0) {
          const empty = document.createElement("p");
          empty.className = "ssh-empty-state";
          empty.textContent = t("settings.ssh.configHostsEmpty");
          configList.appendChild(empty);
          return;
        }

        for (const host of hosts) {
          const item = document.createElement("div");
          item.className = "ssh-config-item";
          item.setAttribute("role", "listitem");

          const nameEl = document.createElement("span");
          nameEl.className = "ssh-config-item-name";
          nameEl.textContent = host.host;
          item.appendChild(nameEl);

          // Show details if available
          const detailParts: string[] = [];
          if (host.hostname) detailParts.push(host.hostname);
          if (host.user) detailParts.push(host.user + "@");
          if (host.port !== 22) detailParts.push(":" + host.port);
          if (detailParts.length > 0) {
            const detailEl = document.createElement("span");
            detailEl.className = "ssh-config-item-detail";
            const userPart = host.user ? `${host.user}@` : "";
            const hostPart = host.hostname || host.host;
            const portPart = host.port !== 22 ? `:${host.port}` : "";
            detailEl.textContent = `${userPart}${hostPart}${portPart}`;
            item.appendChild(detailEl);
          }

          // Import button: creates an eMterm SSH connection from this host
          const importBtn = document.createElement("button");
          importBtn.type = "button";
          importBtn.className = "profile-action-btn";
          importBtn.textContent = t("settings.ssh.import");
          ctx.addContentListener(importBtn, "click", () => {
            const newConn: SshConnection = {
              name: host.host,
              hostname: host.hostname || host.host,
              port: host.port,
              username: host.user,
              identity_file: host.identity_file,
              ssh_options: [],
            };
            ctx.currentSettings.ssh_connections = [
              ...ctx.currentSettings.ssh_connections,
              newConn,
            ];
            ctx.saveSetting(
              "ssh_connections",
              ctx.currentSettings.ssh_connections,
            );
            ctx.reRender();
          });
          item.appendChild(importBtn);

          configList.appendChild(item);
        }
      })
      .catch((err) => {
        console.warn("Failed to load SSH config hosts:", err);
        const errEl = document.createElement("p");
        errEl.className = "ssh-empty-state";
        errEl.textContent = t("settings.ssh.configHostsEmpty");
        configList.appendChild(errEl);
      });
  } else {
    const unavailable = document.createElement("p");
    unavailable.className = "ssh-empty-state";
    unavailable.textContent = t("settings.ssh.configHostsUnavailable");
    panel.appendChild(unavailable);
  }

  // -- SFTP Settings --
  renderSubsectionHeader(panel, "SFTP");

  renderNumberInput(
    panel,
    {
      key: "sftp-max-concurrent-uploads",
      label: t("sftp.settings.maxConcurrentUploads"),
      value: settings.sftp_max_concurrent_uploads,
      min: 1,
      max: 16,
      step: 1,
      unit: "",
      hint: t("sftp.settings.maxConcurrentUploadsHint"),
      onInput: () => {},
      onSave: (v) => ctx.saveSetting("sftp_max_concurrent_uploads", v),
    },
    ctx.addContentListener,
  );
}

function createSshActionButton(
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

function setupSshDragReorder(list: HTMLElement, ctx: SectionContext): void {
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

    const conns = [...ctx.currentSettings.ssh_connections];
    const [moved] = conns.splice(dragIndex, 1);
    if (!moved) return;
    conns.splice(dropIndex, 0, moved);
    ctx.currentSettings.ssh_connections = conns;
    ctx.saveSetting("ssh_connections", conns);
    ctx.reRender();
  }) as EventListener);
}
