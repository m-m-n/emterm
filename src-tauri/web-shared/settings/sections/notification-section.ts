import { t } from "../../i18n/index.ts";
import { renderSubsectionHeader, renderToggle } from "../settings-components";
import type { SectionContext } from "./types";

export function renderNotificationSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.notification.title");
  panel.appendChild(header);

  // -- General subsection --
  renderSubsectionHeader(panel, t("settings.notification.general"));

  // Desktop Notifications (toggle)
  renderToggle(
    panel,
    {
      key: "notification-enabled",
      label: t("settings.notification.notificationEnabled"),
      value: settings.notification_enabled,
      description: t("settings.notification.notificationEnabledDesc"),
      onSave: (v) => ctx.saveSetting("notification_enabled", v),
    },
    ctx.addContentListener,
  );

  // Tab Activity Indicator (toggle)
  renderToggle(
    panel,
    {
      key: "tab-activity-indicator",
      label: t("settings.notification.tabActivityIndicator"),
      value: settings.tab_activity_indicator,
      description: t("settings.notification.tabActivityIndicatorDesc"),
      onSave: (v) => ctx.saveSetting("tab_activity_indicator", v),
    },
    ctx.addContentListener,
  );

  // -- Triggers subsection --
  renderSubsectionHeader(panel, t("settings.notification.triggers"));

  // Notify on Process Exit (toggle)
  renderToggle(
    panel,
    {
      key: "notify-on-process-exit",
      label: t("settings.notification.notifyOnProcessExit"),
      value: settings.notify_on_process_exit,
      description: t("settings.notification.notifyOnProcessExitDesc"),
      onSave: (v) => ctx.saveSetting("notify_on_process_exit", v),
    },
    ctx.addContentListener,
  );

  // Notify on Output (toggle)
  renderToggle(
    panel,
    {
      key: "notify-on-output",
      label: t("settings.notification.notifyOnOutput"),
      value: settings.notify_on_output,
      description: t("settings.notification.notifyOnOutputDesc"),
      onSave: (v) => ctx.saveSetting("notify_on_output", v),
    },
    ctx.addContentListener,
  );

  // Notify on Bell (toggle)
  renderToggle(
    panel,
    {
      key: "notify-on-bell",
      label: t("settings.notification.notifyOnBell"),
      value: settings.notify_on_bell,
      description: t("settings.notification.notifyOnBellDesc"),
      onSave: (v) => ctx.saveSetting("notify_on_bell", v),
    },
    ctx.addContentListener,
  );

  // Agent Status Notifications (toggle) — task0007
  renderToggle(
    panel,
    {
      key: "agent-status-notifications",
      label: t("settings.notification.agentStatusNotifications"),
      value: settings.agent_status_notifications,
      description: t("settings.notification.agentStatusNotificationsDesc"),
      onSave: (v) => ctx.saveSetting("agent_status_notifications", v),
    },
    ctx.addContentListener,
  );
}
