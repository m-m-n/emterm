import { describe, test, expect } from "bun:test";
import { validateSocketPath, parseMuxOsc, decodeStatusUpdateMsg, decodeWelcomeMsg, MuxMessageType } from "./mux-client";

describe("validateSocketPath", () => {
  test("rejects path traversal", () => {
    expect(validateSocketPath("/tmp/emterm/../etc/passwd")).toBe(false);
    expect(validateSocketPath("../../../etc/passwd")).toBe(false);
    expect(validateSocketPath("/tmp/emterm/..\\windows\\system32")).toBe(false);
  });

  test("accepts valid emterm socket paths", () => {
    expect(validateSocketPath("/run/user/1000/emterm/mux-default.sock")).toBe(true);
    expect(validateSocketPath("/tmp/emterm/mux-default.sock")).toBe(true);
  });

  test("rejects paths without emterm", () => {
    expect(validateSocketPath("/tmp/other/mux.sock")).toBe(false);
  });

  test("rejects paths without .sock extension", () => {
    expect(validateSocketPath("/tmp/emterm/something")).toBe(false);
  });
});

describe("parseMuxOsc", () => {
  test("parses attach command", () => {
    const result = parseMuxOsc("emterm", ["mux", "attach", "/tmp/emterm/mux-default.sock", "1"]);
    expect(result).toEqual({
      action: "attach",
      socketPath: "/tmp/emterm/mux-default.sock",
      sessionId: 1,
    });
  });

  test("parses detach command", () => {
    const result = parseMuxOsc("emterm", ["mux", "detach"]);
    expect(result).toEqual({ action: "detach" });
  });

  test("returns null for non-emterm verb", () => {
    expect(parseMuxOsc("other", ["mux", "attach", "/tmp/emterm/s.sock", "0"])).toBeNull();
  });

  test("returns null for non-mux params", () => {
    expect(parseMuxOsc("emterm", ["fold", "start"])).toBeNull();
  });

  test("returns null for invalid attach (missing params)", () => {
    expect(parseMuxOsc("emterm", ["mux", "attach"])).toBeNull();
  });

  test("rejects path traversal in attach", () => {
    const result = parseMuxOsc("emterm", ["mux", "attach", "/tmp/emterm/../etc.sock", "0"]);
    expect(result).toBeNull();
  });

  test("handles non-numeric session ID", () => {
    const result = parseMuxOsc("emterm", ["mux", "attach", "/tmp/emterm/mux.sock", "abc"]);
    expect(result).not.toBeNull();
    expect(result!.sessionId).toBe(0); // defaults to 0
  });

  test("returns null for unknown action", () => {
    expect(parseMuxOsc("emterm", ["mux", "unknown"])).toBeNull();
  });
});

describe("MuxMessageType", () => {
  test("RequestStatusUpdate has correct value", () => {
    expect(MuxMessageType.RequestStatusUpdate).toBe(0x17);
  });

  test("StatusUpdate has correct value", () => {
    expect(MuxMessageType.StatusUpdate).toBe(0x16);
  });
});

describe("decodeWelcomeMsg", () => {
  /** Build a Welcome binary message with the Accepted variant (0). */
  function buildWelcomeMsg(
    serverVersion: number,
    sessions: Array<{ id: number; name: string; window_count: number; pane_count: number; active_window_index: number }>,
  ): Uint8Array {
    const parts: Uint8Array[] = [];

    // variant: u32 (0 = Accepted)
    const variantBuf = new Uint8Array(4);
    new DataView(variantBuf.buffer).setUint32(0, 0, true);
    parts.push(variantBuf);

    // server_version: u32
    const versionBuf = new Uint8Array(4);
    new DataView(versionBuf.buffer).setUint32(0, serverVersion, true);
    parts.push(versionBuf);

    // sessions Vec length: u64 LE
    const lenBuf = new Uint8Array(8);
    new DataView(lenBuf.buffer).setBigUint64(0, BigInt(sessions.length), true);
    parts.push(lenBuf);

    for (const s of sessions) {
      // id: u32
      const idBuf = new Uint8Array(4);
      new DataView(idBuf.buffer).setUint32(0, s.id, true);
      parts.push(idBuf);

      // name: String (u64 len + bytes)
      const nameBytes = new TextEncoder().encode(s.name);
      const nameLenBuf = new Uint8Array(8);
      new DataView(nameLenBuf.buffer).setBigUint64(0, BigInt(nameBytes.length), true);
      parts.push(nameLenBuf);
      parts.push(nameBytes);

      // window_count: u32
      const wcBuf = new Uint8Array(4);
      new DataView(wcBuf.buffer).setUint32(0, s.window_count, true);
      parts.push(wcBuf);

      // pane_count: u32
      const pcBuf = new Uint8Array(4);
      new DataView(pcBuf.buffer).setUint32(0, s.pane_count, true);
      parts.push(pcBuf);

      // active_window_index: u32
      const awiBuf = new Uint8Array(4);
      new DataView(awiBuf.buffer).setUint32(0, s.active_window_index, true);
      parts.push(awiBuf);
    }

    const totalLen = parts.reduce((sum, a) => sum + a.length, 0);
    const result = new Uint8Array(totalLen);
    let offset = 0;
    for (const p of parts) {
      result.set(p, offset);
      offset += p.length;
    }
    return result;
  }

  test("decodes session with active_window_index", () => {
    const data = buildWelcomeMsg(1, [
      { id: 1, name: "default", window_count: 3, pane_count: 3, active_window_index: 2 },
    ]);
    const sessions = decodeWelcomeMsg(data);
    expect(sessions).not.toBeNull();
    expect(sessions!.length).toBe(1);
    expect(sessions![0].id).toBe(1);
    expect(sessions![0].name).toBe("default");
    expect(sessions![0].window_count).toBe(3);
    expect(sessions![0].pane_count).toBe(3);
    expect(sessions![0].active_window_index).toBe(2);
  });

  test("decodes multiple sessions with different active_window_index", () => {
    const data = buildWelcomeMsg(1, [
      { id: 1, name: "session-a", window_count: 2, pane_count: 2, active_window_index: 1 },
      { id: 2, name: "session-b", window_count: 5, pane_count: 5, active_window_index: 4 },
    ]);
    const sessions = decodeWelcomeMsg(data);
    expect(sessions).not.toBeNull();
    expect(sessions!.length).toBe(2);
    expect(sessions![0].active_window_index).toBe(1);
    expect(sessions![1].active_window_index).toBe(4);
  });

  test("active_window_index defaults to 0 for single-window session", () => {
    const data = buildWelcomeMsg(1, [
      { id: 1, name: "single", window_count: 1, pane_count: 1, active_window_index: 0 },
    ]);
    const sessions = decodeWelcomeMsg(data);
    expect(sessions).not.toBeNull();
    expect(sessions![0].active_window_index).toBe(0);
  });

  test("returns null for Rejected variant", () => {
    const buf = new Uint8Array(4);
    new DataView(buf.buffer).setUint32(0, 1, true); // variant 1 = Rejected
    expect(decodeWelcomeMsg(buf)).toBeNull();
  });

  test("returns null for truncated data", () => {
    expect(decodeWelcomeMsg(new Uint8Array([0, 0]))).toBeNull();
  });
});

describe("decodeStatusUpdateMsg", () => {
  /** Helper: encode a string as bincode (u64 LE length + UTF-8 bytes). */
  function encodeBincodeString(s: string): Uint8Array {
    const encoded = new TextEncoder().encode(s);
    const buf = new Uint8Array(8 + encoded.length);
    const view = new DataView(buf.buffer);
    view.setBigUint64(0, BigInt(encoded.length), true);
    buf.set(encoded, 8);
    return buf;
  }

  /** Concatenate Uint8Arrays. */
  function concat(...arrays: Uint8Array[]): Uint8Array {
    const totalLen = arrays.reduce((sum, a) => sum + a.length, 0);
    const result = new Uint8Array(totalLen);
    let offset = 0;
    for (const a of arrays) {
      result.set(a, offset);
      offset += a.length;
    }
    return result;
  }

  test("decodes valid StatusUpdateMsg with left and right", () => {
    const data = concat(
      encodeBincodeString("hello left"),
      encodeBincodeString("hello right"),
    );
    const result = decodeStatusUpdateMsg(data);
    expect(result).not.toBeNull();
    expect(result!.left).toBe("hello left");
    expect(result!.right).toBe("hello right");
  });

  test("decodes empty strings", () => {
    const data = concat(
      encodeBincodeString(""),
      encodeBincodeString(""),
    );
    const result = decodeStatusUpdateMsg(data);
    expect(result).not.toBeNull();
    expect(result!.left).toBe("");
    expect(result!.right).toBe("");
  });

  test("decodes unicode strings", () => {
    const data = concat(
      encodeBincodeString("ステータス"),
      encodeBincodeString("右側"),
    );
    const result = decodeStatusUpdateMsg(data);
    expect(result).not.toBeNull();
    expect(result!.left).toBe("ステータス");
    expect(result!.right).toBe("右側");
  });

  test("returns null for empty data", () => {
    expect(decodeStatusUpdateMsg(new Uint8Array())).toBeNull();
  });

  test("returns null for truncated data", () => {
    // Only 4 bytes - not enough for u64 length
    expect(decodeStatusUpdateMsg(new Uint8Array([0, 0, 0, 0]))).toBeNull();
  });

  test("returns null for truncated right field", () => {
    // Left string is valid but right is missing
    const data = encodeBincodeString("left only");
    expect(decodeStatusUpdateMsg(data)).toBeNull();
  });
});
