/**
 * SFTP file-drop wiring for TerminalApp.
 *
 * Constructs the `UploadManager` and the `FileDropHandler` and connects
 * their callbacks. The handler distinguishes:
 *
 * - **SSH drops** — uploaded to the remote host via `UploadManager`
 *   using the active connection name and the working directory parsed
 *   out of the prompt.
 * - **Local drops** — formatted as a paste payload and written
 *   directly to the PTY (so the user sees the path as if they typed
 *   it).
 *
 * Extracted from TerminalApp to keep `init()` focused on lifecycle.
 */

import type { TerminalState } from "../terminal/state";
import type { PtyClient } from "../pty/client";
import { UploadManager } from "../sftp/upload-manager";
import { FileDropHandler, formatPathsForPaste, extractRemotePath, type FileDropInfo } from "../sftp/file-drop-handler";

export interface SftpSetupResult {
  uploadManager: UploadManager;
  fileDropHandler: FileDropHandler;
}

export interface SftpSetupContext {
  container: HTMLElement;
  isActiveTab(): boolean;
  getSshConnectionName(): string;
  getState(): TerminalState | null;
  getPtyClient(): PtyClient | null;
}

/**
 * Initialize the upload manager and attach the file-drop handler. Both
 * resources are returned so the host can dispose them in the right
 * order during teardown.
 */
export async function setupSftpFileDrop(
  ctx: SftpSetupContext,
): Promise<SftpSetupResult> {
  const uploadManager = new UploadManager();
  await uploadManager.init();

  const fileDropHandler = new FileDropHandler({
    container: ctx.container,
    isActiveTab: ctx.isActiveTab,
    getSshConnectionName: ctx.getSshConnectionName,
    onSshDrop: (files: FileDropInfo[]) => {
      const destination = extractRemotePath(ctx.getState()?._workingDirectory || "");
      const sshConnectionName = ctx.getSshConnectionName();
      uploadManager.handleSshDrop(files, sshConnectionName, destination);
    },
    onLocalDrop: (paths: string[]) => {
      const text = formatPathsForPaste(paths);
      const ptyClient = ctx.getPtyClient();
      if (text && ptyClient) {
        const bytes = new TextEncoder().encode(text);
        ptyClient.write(bytes);
      }
    },
  });
  await fileDropHandler.attach();

  return { uploadManager, fileDropHandler };
}
