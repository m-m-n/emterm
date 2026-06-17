/**
 * Tests for CSI device response handlers.
 */

import { describe, expect, test, beforeEach } from "bun:test";
import { TerminalState } from "../state.ts";
import {
  handleDeviceStatusReport,
  handlePrimaryDeviceAttributes,
  handleSecondaryDeviceAttributes,
  handleTertiaryDeviceAttributes,
} from "./csi_device.ts";
import type { TerminalStateAccessor } from "./types.ts";

describe("csi_device handlers", () => {
  let state: TerminalState;

  beforeEach(() => {
    state = new TerminalState(80, 24);
  });

  // Helper to get accessor - TerminalState implements TerminalStateAccessor
  function getAccessor(): TerminalStateAccessor {
    return state;
  }

  describe("handleDeviceStatusReport", () => {
    test("should respond with OK status (DSR 5)", () => {
      handleDeviceStatusReport(getAccessor(), 5);

      const response = state.takePendingResponse();
      expect(response).not.toBeNull();
      // ESC [ 0 n
      expect(response).toEqual(new Uint8Array([0x1b, 0x5b, 0x30, 0x6e]));
    });

    test("should respond with cursor position (DSR 6)", () => {
      // Position cursor
      state.processAction({ type: "Csi", value: { action: "CursorPosition", data: { row: 10, col: 20 } } });

      handleDeviceStatusReport(getAccessor(), 6);

      const response = state.takePendingResponse();
      expect(response).not.toBeNull();
      // ESC [ 10 ; 20 R (1-indexed)
      const responseStr = new TextDecoder().decode(response!);
      expect(responseStr).toBe("\x1b[10;20R");
    });

    test("should handle unknown DSR codes gracefully", () => {
      handleDeviceStatusReport(getAccessor(), 99);

      const response = state.takePendingResponse();
      expect(response).toBeNull();
    });
  });

  describe("handlePrimaryDeviceAttributes", () => {
    test("should respond with DA1 response", () => {
      handlePrimaryDeviceAttributes(getAccessor());

      const response = state.takePendingResponse();
      expect(response).not.toBeNull();
      const responseStr = new TextDecoder().decode(response!);
      // Should contain VT420 identifier
      expect(responseStr).toContain("\x1b[?");
      expect(responseStr).toContain("c");
    });
  });

  describe("handleSecondaryDeviceAttributes", () => {
    test("should respond with DA2 response", () => {
      handleSecondaryDeviceAttributes(getAccessor());

      const response = state.takePendingResponse();
      expect(response).not.toBeNull();
      const responseStr = new TextDecoder().decode(response!);
      // Should contain VT420 identifier (41)
      expect(responseStr).toContain("\x1b[>");
      expect(responseStr).toContain("c");
    });
  });

  describe("handleTertiaryDeviceAttributes", () => {
    test("should not generate a response (currently ignored)", () => {
      handleTertiaryDeviceAttributes(getAccessor());

      const response = state.takePendingResponse();
      // Currently DA3 is ignored
      expect(response).toBeNull();
    });
  });

  describe("multiple DSR requests", () => {
    test("should buffer multiple responses", () => {
      handleDeviceStatusReport(getAccessor(), 5);
      handleDeviceStatusReport(getAccessor(), 6);

      const response = state.takePendingResponse();
      expect(response).not.toBeNull();
      // Should contain both responses concatenated
      const responseStr = new TextDecoder().decode(response!);
      expect(responseStr).toContain("0n"); // Status OK
      expect(responseStr).toContain("R"); // Cursor position report
    });
  });
});
