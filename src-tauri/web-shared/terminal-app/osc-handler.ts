/**
 * OSC (Operating System Command) handler functions extracted from TerminalApp.
 * Handles OSC callbacks from the WASM parser, queued OSC event processing,
 * iTerm2 inline image display, and window title updates.
 */

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { TerminalState } from "../terminal/state";
import type { ITerminalRenderer } from "../terminal";
import type { PtyClient } from "../pty/client";
import type { OscColorHandler } from "../terminal/osc-colors";
import type { CursorShapeStack } from "../terminal/osc-cursor-shape";
import type { ImageHandler } from "./handlers/image";
import type { DownloadSessionManager } from "../download";
import { SettingsService } from "../settings/settings-service";
import { handleSemanticPrompt, handleFoldCommand } from "../terminal/handlers/osc_handlers";
import { indexToRgb, DEFAULT_FOREGROUND, DEFAULT_BACKGROUND } from "../terminal/colors";
import { handleOsc52 } from "../terminal/osc-clipboard";
import { parseOsc9, sendNotification } from "../terminal/osc-notification";
import { parseOsc22 } from "../terminal/osc-cursor-shape";
import { parseIterm2Command } from "../terminal/osc-iterm2";

/**
 * Context needed by OSC handler functions.
 */
export interface OscHandlerContext {
  state: TerminalState | null;
  renderer: ITerminalRenderer | null;
  ptyClient: PtyClient | null;
  oscColorHandler: OscColorHandler;
  cursorShapeStack: CursorShapeStack;
  imageHandler: ImageHandler | null;
  downloadManager: DownloadSessionManager | null;
  terminalRoot: HTMLElement | null;
  titleChangeCallback: ((title: string) => void) | null;
  lastWindowTitle: string;
  setLastWindowTitle: (title: string) => void;
  /** Callback for mux attach OSC sequence */
  muxAttachCallback: ((socketPath: string, sessionId: number) => void) | null;
  /** Callback for mux detach OSC sequence */
  muxDetachCallback: (() => void) | null;
  /** Callback for status bar OSC commands */
  statusBarOscCallback: ((command: string, param1?: string, param2?: string) => void) | null;
}

/**
 * Process all queued OSC events.
 * Safe to call after process_pty_data has returned (borrow released).
 */
export function processPendingOscQueue(
  pendingOscQueue: { actionType: number; data: string }[],
  ctx: OscHandlerContext,
): void {
  if (pendingOscQueue.length === 0) return;
  const events = pendingOscQueue.splice(0);
  for (const { actionType, data } of events) {
    handleOscCallback(ctx, actionType, data);
  }
}

/**
 * Handle OSC callback from WASM parser.
 * actionType maps to OSC number (0=SetTitleAndIcon, 2=SetTitle, etc.)
 */
export function handleOscCallback(
  ctx: OscHandlerContext,
  actionType: number,
  data: string,
): void {
  if (!ctx.state) return;

  switch (actionType) {
    case 0: // SetTitleAndIcon
      ctx.state._title = data;
      ctx.state._iconName = data;
      updateWindowTitle(ctx, data);
      break;
    case 1: // SetIconName
      ctx.state._iconName = data;
      break;
    case 2: // SetTitle
      ctx.state._title = data;
      updateWindowTitle(ctx, data);
      break;
    case 4: { // SetColorPalette
      const writeFn = (resp: string) => {
        ctx.ptyClient?.write(new TextEncoder().encode(resp));
      };
      ctx.oscColorHandler.handleOsc4(data, writeFn, (index) => {
        return indexToRgb(index);
      });
      // Notify renderer of palette change
      ctx.renderer?.forceRender(ctx.state!);
      break;
    }
    case 7: // SetWorkingDirectory
      ctx.state._workingDirectory = data;
      break;
    case 8: { // Hyperlink
      // data format: "params;uri" (semicolon-separated)
      const sepIdx = data.indexOf(";");
      if (sepIdx >= 0) {
        const params = data.substring(0, sepIdx);
        const uri = data.substring(sepIdx + 1);
        if (uri) {
          ctx.state._activeHyperlink = { params, uri };
        } else {
          ctx.state._activeHyperlink = null;
        }
      }
      break;
    }
    case 10: // SetForegroundColor
    case 11: // SetBackgroundColor
    case 12: { // SetCursorColor
      // Query responses (`?`) for OSC 10/11/12 are handled by the Rust
      // reader-thread scanner (src-tauri/src/pty/device_query_scanner.rs)
      // which writes directly to the PTY master fd via libc::write() for
      // zero-latency delivery. Responding from here goes through Tauri IPC
      // and arrives too late: by the time the response hits the kernel the
      // querying CLI has already exited and the shell is back in cooked
      // mode with ECHO on, so the response bytes are echoed as visible
      // `^[]11;rgb:xxxx/xxxx/xxxx^[\` garbage text.
      // SET operations still need to update the in-process color state,
      // so we pass a no-op writeFn and let handleOscDefaultColor() dispatch
      // the non-query branch normally.
      const writeFn = (_resp: string) => {};
      const lookupThemeDefault = (oscNum: number) => {
        switch (oscNum) {
          case 10: return DEFAULT_FOREGROUND;
          case 11: return DEFAULT_BACKGROUND;
          case 12: return DEFAULT_FOREGROUND; // cursor defaults to foreground
          default: return null;
        }
      };
      ctx.oscColorHandler.handleOscDefaultColor(actionType, data, writeFn, lookupThemeDefault);
      ctx.renderer?.forceRender(ctx.state!);
      break;
    }
    case 104: // ResetColorPalette
      ctx.oscColorHandler.handleOsc104(data);
      ctx.renderer?.forceRender(ctx.state!);
      break;
    case 110: // ResetForegroundColor
      ctx.oscColorHandler.resetForeground();
      ctx.renderer?.forceRender(ctx.state!);
      break;
    case 111: // ResetBackgroundColor
      ctx.oscColorHandler.resetBackground();
      ctx.renderer?.forceRender(ctx.state!);
      break;
    case 112: // ResetCursorColor
      ctx.oscColorHandler.resetCursorColor();
      ctx.renderer?.forceRender(ctx.state!);
      break;
    case 9: { // Notification / Progress (OSC 9)
      const action = parseOsc9(data);
      if (!action) break;
      if (action.type === "notification") {
        sendNotification("eMterm", action.message);
      } else {
        // Progress: update state and notify tab bar
        ctx.state._progressState = action.state;
        ctx.state._progressPercentage = action.percentage;
        ctx.titleChangeCallback?.(ctx.state.title || "Terminal");
      }
      break;
    }
    case 22: { // Cursor Shape (OSC 22)
      const action = parseOsc22(data);
      if (!action) break;
      const terminalRoot = ctx.terminalRoot;
      if (action.type === "set") {
        ctx.cursorShapeStack.set(action.shape);
      } else if (action.type === "push") {
        ctx.cursorShapeStack.push(action.shape);
      } else {
        ctx.cursorShapeStack.pop();
      }
      if (terminalRoot) {
        terminalRoot.style.cursor = ctx.cursorShapeStack.current();
      }
      break;
    }
    case 52: { // Clipboard (OSC 52)
      const settings = SettingsService.getCached();
      const config = {
        readEnabled: settings?.clipboard_read_osc52 ?? true,
        maxSize: settings?.clipboard_max_size_osc52 ?? 10 * 1024 * 1024,
      };
      handleOsc52(
        data,
        config,
        async () => {
          const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
          return await readText();
        },
        async (text: string) => {
          const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
          await writeText(text);
        },
        (resp: string) => {
          ctx.ptyClient?.write(new TextEncoder().encode(resp));
        },
      );
      break;
    }
    case 100: { // EmtermExtension (OSC 777)
      // data format: "verb;param1;param2;..."
      const parts = data.split(";");
      const verb = parts[0] || "";
      const params = parts.slice(1);
      // Handle mux commands (emterm;mux;action;...)
      if (verb === "emterm" && params.length > 0 && params[0] === "mux") {
        handleMuxOsc(ctx, params);
      } else if (verb === "emterm" && params.length > 0 && params[0] === "statusbar") {
        // Route to status bar OSC handler
        if (ctx.statusBarOscCallback) {
          const sbParams = params.slice(1); // Remove "statusbar" prefix
          ctx.statusBarOscCallback(sbParams[0] ?? "", sbParams[1], sbParams[2]);
        }
      } else if (verb === "emterm" && params.length > 0 && params[0] === "fold") {
        handleFoldCommand(ctx.state, params.slice(1));
      } else if (verb === "emterm" && params.length > 0 && params[0] === "download") {
        // Route to download manager
        ctx.downloadManager?.handleCommand(verb, params);
      } else if (verb === "emterm" && params.length > 0 && (params[0] === "json" || params[0] === "yaml")) {
        // Route to data viewer manager
        ctx.state.getDataViewerManager().handleCommand(verb, params);
      } else {
        // Route to markdown manager
        ctx.state.getMarkdownManager().handleCommand(verb, params);
      }
      break;
    }
    case 133: { // SemanticPrompt
      // data format: "A" or "D;0" (zone_type[;exit_code])
      const parts = data.split(";");
      const zoneType = parts[0] || "";
      const exitCode = parts.length > 1 ? parseInt(parts[1]!, 10) : null;
      handleSemanticPrompt(ctx.state, zoneType, exitCode);
      break;
    }
    case 101: { // iTerm2 Protocol (OSC 1337)
      const cmd = parseIterm2Command(data);
      if (!cmd) break;
      if (cmd.type === "file") {
        if (cmd.inline && cmd.base64Data) {
          // Inline image display: decode via backend and show
          handleIterm2InlineImage(ctx, cmd.base64Data, cmd.name);
        } else {
          // Download mode: log for now (download infrastructure is backend-driven)
          console.log(`[LOG][FRONTEND] OSC 1337;File download: ${cmd.name || "unnamed"}`);
        }
      } else if (cmd.type === "set_user_var") {
        ctx.state._userVariables.set(cmd.key, cmd.value);
      }
      break;
    }
    // Unknown (255) - ignored
  }
}

/**
 * Handle iTerm2 inline image (OSC 1337;File with inline=1).
 * Decodes raw image data via Tauri backend and displays it.
 */
export async function handleIterm2InlineImage(
  ctx: OscHandlerContext,
  base64Data: string,
  name: string,
): Promise<void> {
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    // Send raw image data to backend for decoding into RGBA
    const result = await invoke<{ width: number; height: number; rgba_base64: string }>(
      "decode_iterm2_image",
      { base64Data },
    );
    if (result && ctx.imageHandler) {
      // Create a synthetic DecodedImage and display it
      const image = {
        id: Date.now(), // Use timestamp as unique ID
        width: result.width,
        height: result.height,
        rgba_base64: result.rgba_base64,
      };
      ctx.imageHandler.showImage(image);
    }
  } catch (error) {
    console.error(`[ERROR][FRONTEND] Failed to decode iTerm2 image "${name}":`, error);
  }
}

/**
 * Update window title and notify callbacks.
 */
export function updateWindowTitle(ctx: OscHandlerContext, title: string): void {
  if (title === ctx.lastWindowTitle) return;
  ctx.setLastWindowTitle(title);

  const displayTitle = title || "eMterm";
  getCurrentWebviewWindow().setTitle(displayTitle).catch((error) => {
    console.error("Failed to set window title:", error);
  });

  if (ctx.titleChangeCallback) {
    ctx.titleChangeCallback(title || "Terminal");
  }
}

/**
 * Handle mux OSC commands (emterm;mux;action;...).
 * Dispatched from the OSC 777 handler when params[0] === "mux".
 */
function handleMuxOsc(ctx: OscHandlerContext, params: string[]): void {
  const action = params[1];
  if (action === "attach" && params.length >= 4) {
    const socketPath = params[2]!;
    const sessionId = parseInt(params[3]!, 10);

    // Validate socket path
    if (socketPath.includes("../") || socketPath.includes("..\\")) {
      console.error("[ERROR][FRONTEND] Mux attach: path traversal in socket path");
      return;
    }
    if (!socketPath.includes("emterm")) {
      console.error("[ERROR][FRONTEND] Mux attach: socket path not in emterm directory");
      return;
    }

    console.info(
      `[INFO][FRONTEND] Mux attach: socket=${socketPath}, session=${sessionId}`,
    );

    // Emit mux:attach event for TerminalApp to handle mode switch
    if (ctx.muxAttachCallback) {
      ctx.muxAttachCallback(socketPath, isNaN(sessionId) ? 0 : sessionId);
    }
  } else if (action === "detach") {
    console.info("[INFO][FRONTEND] Mux detach");
    if (ctx.muxDetachCallback) {
      ctx.muxDetachCallback();
    }
  } else {
    console.warn("[WARN][FRONTEND] Unknown mux action:", action);
  }
}
