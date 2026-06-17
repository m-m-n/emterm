/**
 * Upload Manager
 *
 * Orchestrates the full SFTP upload workflow:
 * 1. Receives file drop from FileDropHandler
 * 2. Shows UploadDialog for confirmation
 * 3. Checks for duplicates via backend
 * 4. Shows OverwriteConfirmDialog if needed
 * 5. Initiates uploads via backend commands
 * 6. Displays progress via UploadProgressDisplay
 *
 * Also manages tab close guard for active uploads.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { t } from "../i18n/index.ts";
import type { SshConnection } from "../settings/types";
import { SettingsService } from "../settings/settings-service";
import type { FileDropInfo } from "./file-drop-handler";
import { showUploadDialog } from "./upload-dialog";
import { showOverwriteDialog } from "./overwrite-dialog";
import {
  UploadProgressDisplay,
  type UploadProgressInfo,
} from "./upload-progress";

/**
 * Backend progress event payload shape.
 */
interface SftpUploadProgressPayload {
  session_id: string;
  file_name: string;
  bytes_transferred: number;
  total_bytes: number;
  status: "preparing" | "uploading" | "completed" | "failed" | "cancelled";
  error_message: string | null;
}

/**
 * Connection args passed to backend commands.
 */
interface SftpConnectionArgs {
  hostname: string;
  port: number;
  username: string;
  identity_file: string;
  ssh_options: Array<{ key: string; value: string }>;
}

/**
 * Manages the upload lifecycle for a tab.
 */
export class UploadManager {
  private progressDisplay: UploadProgressDisplay;
  private activeSessions: Set<string> = new Set();
  private unlistenProgress: UnlistenFn | null = null;
  private sessionCounter = 0;

  constructor() {
    this.progressDisplay = new UploadProgressDisplay();
    this.progressDisplay.onCancel((sessionId) => this.cancelUpload(sessionId));
  }

  /**
   * Initialize the upload manager by subscribing to backend progress events.
   */
  async init(): Promise<void> {
    this.unlistenProgress = await listen<SftpUploadProgressPayload>(
      "sftp-upload-progress",
      (event) => {
        const payload = event.payload;
        const info: UploadProgressInfo = {
          sessionId: payload.session_id,
          fileName: payload.file_name,
          bytesTransferred: payload.bytes_transferred,
          totalBytes: payload.total_bytes,
          status: payload.status,
          errorMessage: payload.error_message ?? undefined,
        };
        this.progressDisplay.update(info);

        // Clean up terminal states
        if (
          payload.status === "completed" ||
          payload.status === "failed" ||
          payload.status === "cancelled"
        ) {
          this.activeSessions.delete(payload.session_id);
        }
      },
    );
  }

  /**
   * Handle files dropped on an SSH tab.
   * Runs the full upload workflow: dialog -> duplicate check -> upload.
   */
  async handleSshDrop(
    files: FileDropInfo[],
    sshConnectionName: string,
    defaultDestination: string,
  ): Promise<void> {
    // Step 1: Show upload dialog
    const dialogResult = await showUploadDialog({
      files,
      defaultDestination,
    });

    if (!dialogResult.confirmed) return;

    // Get SSH connection details from settings
    const settings = SettingsService.getCached();
    if (!settings) {
      console.error("Settings not available for SFTP upload");
      return;
    }

    const conn = settings.ssh_connections.find(
      (c) => c.name === sshConnectionName,
    );
    if (!conn) {
      console.error(
        `SSH connection '${sshConnectionName}' not found in settings (available: ${settings.ssh_connections.map((c) => c.name).join(", ")})`,
      );
      return;
    }

    if (!conn.hostname) {
      console.error(
        `SSH connection '${sshConnectionName}' has no hostname configured`,
      );
      return;
    }

    const connectionArgs = this.buildConnectionArgs(conn);
    const destination = dialogResult.destination;

    // Step 2: Check for duplicates
    const fileNames = files.map((f) => f.name);
    try {
      const duplicates = await invoke<string[]>("sftp_check_duplicates", {
        connection: connectionArgs,
        remoteDir: destination,
        fileNames,
      });

      if (duplicates.length > 0) {
        // Step 3: Show overwrite dialog
        const overwriteResult = await showOverwriteDialog({
          conflictingFiles: duplicates,
        });
        if (!overwriteResult.approved) return;
      }
    } catch (error) {
      const errorStr = String(error);
      // Fatal errors: abort upload entirely
      if (
        errorStr.includes("Missing hostname") ||
        errorStr.includes("not found") ||
        errorStr.includes("Invalid")
      ) {
        console.error("SFTP connection error, aborting upload:", error);
        return;
      }
      // Non-fatal errors (e.g., remote directory doesn't exist yet): proceed
      console.warn("Duplicate check failed, proceeding with upload:", error);
    }

    // Step 4: Start uploads
    for (const file of files) {
      const sessionId = this.generateSessionId();
      this.activeSessions.add(sessionId);

      const remotePath = destination
        ? `${destination}/${file.name}`
        : file.name;

      // Fire and forget - progress is tracked via events
      invoke("sftp_upload", {
        sessionId,
        connection: connectionArgs,
        localPath: file.path,
        remotePath,
        isDirectory: file.isDirectory,
      }).catch((error) => {
        console.error(`Upload failed for ${file.name}:`, error);
        this.activeSessions.delete(sessionId);
      });
    }
  }

  /**
   * Cancel a specific upload.
   */
  async cancelUpload(sessionId: string): Promise<void> {
    try {
      await invoke("sftp_cancel_upload", { sessionId });
    } catch (error) {
      console.error("Failed to cancel upload:", error);
    }
    this.activeSessions.delete(sessionId);
  }

  /**
   * Cancel all active uploads (e.g., on tab close).
   */
  async cancelAllUploads(): Promise<void> {
    const sessions = [...this.activeSessions];
    for (const sessionId of sessions) {
      await this.cancelUpload(sessionId);
    }
  }

  /**
   * Check if there are active uploads.
   */
  hasActiveUploads(): boolean {
    return this.activeSessions.size > 0;
  }

  /**
   * Dispose the upload manager.
   */
  dispose(): void {
    this.unlistenProgress?.();
    this.unlistenProgress = null;
    this.progressDisplay.dispose();
    this.activeSessions.clear();
  }

  private buildConnectionArgs(conn: SshConnection): SftpConnectionArgs {
    return {
      hostname: conn.hostname,
      port: conn.port,
      username: conn.username,
      identity_file: conn.identity_file,
      ssh_options: conn.ssh_options.map((o) => ({
        key: o.key,
        value: o.value,
      })),
    };
  }

  private generateSessionId(): string {
    this.sessionCounter++;
    return `sftp-${Date.now()}-${this.sessionCounter}`;
  }
}
