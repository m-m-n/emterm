import { invoke } from "@tauri-apps/api/core";
import { t } from "../../i18n/index.ts";
import { renderSubsectionHeader, renderToggle } from "../settings-components";
import type { SectionContext } from "./types";

export function renderLogSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const { currentSettings: settings } = ctx;
  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.log.title");
  panel.appendChild(header);

  // Log Recording toggle
  renderToggle(
    panel,
    {
      key: "log-recording-enabled",
      label: t("settings.log.recording"),
      value: settings.log_recording_enabled,
      description: t("settings.log.recordingDesc"),
      onSave: (v) => {
        invoke("set_log_recording", { enabled: v });
        ctx.saveSetting("log_recording_enabled", v);
      },
    },
    ctx.addContentListener,
  );

  renderSubsectionHeader(panel, t("settings.log.logFile"));

  // Description
  const desc = document.createElement("p");
  desc.className = "settings-description";
  desc.textContent = t("settings.log.logFileDesc");
  panel.appendChild(desc);

  // Log file path display
  const pathRow = document.createElement("div");
  pathRow.className = "settings-row";
  const pathLabel = document.createElement("span");
  pathLabel.className = "settings-label";
  pathLabel.textContent = t("settings.log.logFilePath");
  pathRow.appendChild(pathLabel);
  const pathValue = document.createElement("code");
  pathValue.className = "settings-log-path";
  pathValue.textContent = "...";
  pathRow.appendChild(pathValue);
  panel.appendChild(pathRow);

  // Log content area
  const logArea = document.createElement("pre");
  logArea.className = "settings-log-content";
  logArea.textContent = t("settings.log.loading");
  panel.appendChild(logArea);

  // Button row
  const btnRow = document.createElement("div");
  btnRow.className = "settings-log-actions";

  const reloadBtn = document.createElement("button");
  reloadBtn.className = "settings-btn";
  reloadBtn.disabled = true;
  reloadBtn.textContent = t("settings.log.reload");
  btnRow.appendChild(reloadBtn);

  const copyBtn = document.createElement("button");
  copyBtn.className = "settings-btn";
  copyBtn.disabled = true;
  copyBtn.textContent = t("settings.log.copy");
  btnRow.appendChild(copyBtn);

  const clearBtn = document.createElement("button");
  clearBtn.className = "settings-btn settings-btn-danger";
  clearBtn.disabled = true;
  clearBtn.textContent = t("settings.log.clear");
  btnRow.appendChild(clearBtn);

  const statusSpan = document.createElement("span");
  statusSpan.className = "settings-log-status";
  btnRow.appendChild(statusSpan);

  panel.appendChild(btnRow);

  // Load log data (tail 500 lines, async)
  const loadLog = async () => {
    reloadBtn.disabled = true;
    copyBtn.disabled = true;
    clearBtn.disabled = true;
    logArea.textContent = t("settings.log.loading");
    try {
      const path = await invoke<string | null>("get_log_path");
      pathValue.textContent = path || t("settings.log.noLogFile");

      const contents = await invoke<string>("get_log_tail", { lines: 500 });
      if (contents.trim()) {
        logArea.textContent = contents;
        logArea.scrollTop = logArea.scrollHeight;
      } else {
        logArea.textContent = t("settings.log.empty");
      }
    } catch (e) {
      logArea.textContent = String(e);
    } finally {
      reloadBtn.disabled = false;
      copyBtn.disabled = false;
      clearBtn.disabled = false;
    }
  };

  loadLog();

  // Reload button
  ctx.addContentListener(reloadBtn, "click", () => {
    loadLog();
  });

  // Copy button
  ctx.addContentListener(copyBtn, "click", async () => {
    try {
      const contents = await invoke<string>("get_log_tail", { lines: 500 });
      await navigator.clipboard.writeText(contents);
      statusSpan.textContent = t("settings.log.copied");
      setTimeout(() => { statusSpan.textContent = ""; }, 2000);
    } catch (e) {
      statusSpan.textContent = String(e);
    }
  });

  // Clear button
  ctx.addContentListener(clearBtn, "click", async () => {
    try {
      await invoke("clear_log");
      logArea.textContent = t("settings.log.empty");
      statusSpan.textContent = t("settings.log.cleared");
      setTimeout(() => { statusSpan.textContent = ""; }, 2000);
    } catch (e) {
      statusSpan.textContent = String(e);
    }
  });
}
