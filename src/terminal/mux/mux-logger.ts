/**
 * Mux client logger -- writes timestamped log lines to mux-client.log via Tauri command.
 *
 * Log file location: same directory as mux-daemon.log and mux-bridge.log
 * ($XDG_RUNTIME_DIR/emterm/mux-client.log).
 */

import { invoke } from "@tauri-apps/api/core";

function timestamp(): string {
  return new Date().toISOString();
}

function writeLine(level: string, msg: string): void {
  const line = `${timestamp()} ${level}[MUX-CLIENT] ${msg}`;
  invoke("mux_client_log", { line }).catch(() => {
    // Fallback to console if invoke fails (e.g. during shutdown)
  });
}

export const muxLog = {
  info(msg: string): void {
    writeLine("INFO", msg);
  },
  warn(msg: string): void {
    writeLine("WARN", msg);
  },
  error(msg: string): void {
    writeLine("ERROR", msg);
  },
  debug(msg: string): void {
    writeLine("DEBUG", msg);
  },
};
