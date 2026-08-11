import { t } from "../../i18n/index.ts";
import { renderToggle } from "../settings-components";
import type { SectionContext } from "./types";

/**
 * Renders the "Agent" settings category: the agent notification master
 * toggle (moved here from Notifications) followed by the per-event-type
 * toggles that gate individual agent state transitions.
 *
 * Order: master -> turn complete (done) -> waiting for input (blocked) ->
 * visible pane.
 */
export function renderAgentSection(
  panel: HTMLElement,
  ctx: SectionContext,
): void {
  const settings = ctx.currentSettings;

  const header = document.createElement("h2");
  header.className = "settings-section-header";
  header.textContent = t("settings.agent.title");
  panel.appendChild(header);

  // Agent Status Notifications (master toggle) — moved from Notifications
  renderToggle(
    panel,
    {
      key: "agent-status-notifications",
      label: t("settings.agent.master"),
      value: settings.agent_status_notifications,
      description: t("settings.agent.masterDesc"),
      onSave: (v) => ctx.saveSetting("agent_status_notifications", v),
    },
    ctx.addContentListener,
  );

  // Turn Complete (done transition) notification toggle
  renderToggle(
    panel,
    {
      key: "agent-notify-on-done",
      label: t("settings.agent.notifyOnDone"),
      value: settings.agent_notify_on_done,
      description: t("settings.agent.notifyOnDoneDesc"),
      onSave: (v) => ctx.saveSetting("agent_notify_on_done", v),
    },
    ctx.addContentListener,
  );

  // Waiting for Input (blocked transition) notification toggle
  renderToggle(
    panel,
    {
      key: "agent-notify-on-blocked",
      label: t("settings.agent.notifyOnBlocked"),
      value: settings.agent_notify_on_blocked,
      description: t("settings.agent.notifyOnBlockedDesc"),
      onSave: (v) => ctx.saveSetting("agent_notify_on_blocked", v),
    },
    ctx.addContentListener,
  );

  // Visible Pane notification toggle
  renderToggle(
    panel,
    {
      key: "agent-notify-visible-pane",
      label: t("settings.agent.notifyVisiblePane"),
      value: settings.agent_notify_visible_pane,
      description: t("settings.agent.notifyVisiblePaneDesc"),
      onSave: (v) => ctx.saveSetting("agent_notify_visible_pane", v),
    },
    ctx.addContentListener,
  );
}
