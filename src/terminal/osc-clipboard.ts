/**
 * OSC 52 clipboard handler.
 *
 * Provides SSH-transparent clipboard read/write via OSC 52 sequences.
 * Supports system clipboard (c), primary selection (p), and combined (cp) targets.
 */

// ── Types ───────────────────────────────────────────────

export type Osc52Action =
  | { type: "write"; target: string; data: string }
  | { type: "query"; target: string }
  | { type: "clear"; target: string };

// ── Parsing ─────────────────────────────────────────────

/**
 * Parse OSC 52 data string into an action.
 * Format: "target;payload" where payload is base64, "?" for query, or empty for clear.
 */
export function parseOsc52(data: string): Osc52Action | null {
  if (!data) return null;

  const sepIdx = data.indexOf(";");
  if (sepIdx < 0) return null;

  const target = data.substring(0, sepIdx);
  const payload = data.substring(sepIdx + 1);

  if (payload === "?") {
    return { type: "query", target };
  }
  if (payload === "") {
    return { type: "clear", target };
  }
  return { type: "write", target, data: payload };
}

// ── Base64 utilities ────────────────────────────────────

/**
 * Encode a string to base64.
 */
export function encodeBase64(text: string): string {
  if (!text) return "";
  const encoder = new TextEncoder();
  const bytes = encoder.encode(text);
  // Convert Uint8Array to binary string
  let binary = "";
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]!);
  }
  return btoa(binary);
}

/**
 * Decode a base64 string. Returns null on invalid input.
 */
export function decodeBase64(encoded: string): string | null {
  if (!encoded) return "";
  try {
    const binary = atob(encoded);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return new TextDecoder().decode(bytes);
  } catch {
    return null;
  }
}

// ── Default settings ────────────────────────────────────

/** Default maximum clipboard payload size in bytes (10 MB). */
const DEFAULT_MAX_SIZE = 10 * 1024 * 1024;

// ── Handler ─────────────────────────────────────────────

export interface OscClipboardConfig {
  /** Whether to respond to clipboard read queries (OSC 52 ?). Default: true. */
  readEnabled: boolean;
  /** Maximum decoded payload size in bytes. Default: 10 MB. */
  maxSize: number;
}

/**
 * Handle OSC 52 clipboard operation.
 *
 * @param data - OSC 52 data string (target;payload)
 * @param config - Clipboard security configuration
 * @param clipboardRead - Async function to read system clipboard text
 * @param clipboardWrite - Async function to write text to system clipboard
 * @param respondFn - Function to send PTY response bytes
 */
export async function handleOsc52(
  data: string,
  config: OscClipboardConfig,
  clipboardRead: () => Promise<string>,
  clipboardWrite: (text: string) => Promise<void>,
  respondFn: (response: string) => void,
): Promise<void> {
  const action = parseOsc52(data);
  if (!action) return;

  switch (action.type) {
    case "write": {
      const decoded = decodeBase64(action.data);
      if (decoded === null) {
        console.debug("[DEBUG][FRONTEND] OSC 52: invalid base64 data");
        return;
      }
      // Check size limit against decoded size
      const byteSize = new TextEncoder().encode(decoded).length;
      if (byteSize > (config.maxSize || DEFAULT_MAX_SIZE)) {
        console.debug(
          `[DEBUG][FRONTEND] OSC 52: payload too large (${byteSize} bytes, limit ${config.maxSize || DEFAULT_MAX_SIZE})`,
        );
        return;
      }
      try {
        await clipboardWrite(decoded);
      } catch (err) {
        console.error("[ERROR][FRONTEND] OSC 52: clipboard write failed:", err);
      }
      break;
    }

    case "query": {
      if (!config.readEnabled) {
        // Silently ignore read queries when disabled
        return;
      }
      try {
        const text = await clipboardRead();
        const encoded = encodeBase64(text);
        const header = `\x1b]52;${action.target};`;
        const trailer = `\x1b\\`;
        const maxPayload = 1024 * 1024; // 1MB pty_write limit
        const maxBase64Len = maxPayload - header.length - trailer.length;
        // Cap base64 data to fit within a single pty_write call.
        // Truncating base64 at a non-boundary may cause decode error on the
        // receiving end, but this is preferable to splitting escape sequences.
        const cappedEncoded = encoded.length > maxBase64Len
          ? encoded.slice(0, maxBase64Len)
          : encoded;
        respondFn(`${header}${cappedEncoded}${trailer}`);
      } catch (err) {
        console.error("[ERROR][FRONTEND] OSC 52: clipboard read failed:", err);
      }
      break;
    }

    case "clear": {
      try {
        await clipboardWrite("");
      } catch (err) {
        console.error("[ERROR][FRONTEND] OSC 52: clipboard clear failed:", err);
      }
      break;
    }
  }
}
