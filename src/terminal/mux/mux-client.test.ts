import { describe, test, expect } from "bun:test";
import { validateSocketPath, parseMuxOsc, decodeStatusUpdateMsg, MuxMessageType } from "./mux-client";

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
