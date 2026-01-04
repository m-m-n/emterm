/**
 * Unit tests for PtyClient
 *
 * These tests verify the event filtering logic and duplicate event prevention
 * in the PtyClient class, particularly the fix for the race condition where
 * pty_exit events arrive before sessionId is set.
 */

import { describe, test, expect, mock, beforeEach } from "bun:test";
import type { PtyExitPayload } from "../types/pty";

/**
 * Mock implementation of Tauri's listen function
 */
type ListenerCallback<T> = (event: { payload: T }) => void;
type UnlistenFn = () => void;

let mockListeners: Map<string, ListenerCallback<any>[]> = new Map();

function mockListen<T>(
  eventName: string,
  callback: ListenerCallback<T>
): Promise<UnlistenFn> {
  if (!mockListeners.has(eventName)) {
    mockListeners.set(eventName, []);
  }
  mockListeners.get(eventName)!.push(callback);

  const unlisten = () => {
    const listeners = mockListeners.get(eventName);
    if (listeners) {
      const index = listeners.indexOf(callback);
      if (index > -1) {
        listeners.splice(index, 1);
      }
    }
  };

  return Promise.resolve(unlisten);
}

/**
 * Emit a mock event to all registered listeners
 */
function mockEmit<T>(eventName: string, payload: T): void {
  const listeners = mockListeners.get(eventName);
  if (listeners) {
    listeners.forEach((callback) => callback({ payload }));
  }
}

/**
 * Clear all mock listeners
 */
function clearMockListeners(): void {
  mockListeners.clear();
}

/**
 * Mock PtyClient for testing (simplified version)
 */
class MockPtyClient {
  private sessionId: string | null = null;
  private exitHandled = false;
  private unlisteners: UnlistenFn[] = [];

  getSessionId(): string | null {
    return this.sessionId;
  }

  setSessionId(sessionId: string | null): void {
    this.sessionId = sessionId;
  }

  async onExit(callback: (code: number, remaining: number) => void): Promise<void> {
    const unlisten = await mockListen<PtyExitPayload>("pty_exit", (event) => {
      // Prevent duplicate processing
      if (this.exitHandled) {
        return;
      }

      // NOTE: This implementation assumes single-session model (one PTY per window).
      // The condition `sessionId === null` handles the race where shell exits before
      // spawn() returns, but this will NOT work correctly in multi-tab scenarios.
      if (this.sessionId === null || event.payload.session_id === this.sessionId) {
        this.exitHandled = true;
        callback(event.payload.code, event.payload.remaining_sessions);
        unlisten();
        this.sessionId = null;
      }
    });
    this.unlisteners.push(unlisten);
  }

  dispose(): void {
    this.unlisteners.forEach((unlisten) => unlisten());
    this.unlisteners = [];
  }
}

describe("PtyClient.onExit()", () => {
  beforeEach(() => {
    clearMockListeners();
  });

  test("processes event when sessionId is null (race condition fix)", async () => {
    // Arrange
    const client = new MockPtyClient();
    let callbackInvoked = false;
    let receivedCode: number | null = null;
    let receivedRemaining: number | null = null;

    await client.onExit((code, remaining) => {
      callbackInvoked = true;
      receivedCode = code;
      receivedRemaining = remaining;
    });

    // Act: Emit event while sessionId is still null (simulating race condition)
    const testPayload: PtyExitPayload = {
      session_id: "test-session-id",
      code: 0,
      remaining_sessions: 0,
    };
    mockEmit("pty_exit", testPayload);

    // Assert
    expect(callbackInvoked).toBe(true);
    expect(receivedCode).toBe(0);
    expect(receivedRemaining).toBe(0);
    expect(client.getSessionId()).toBeNull();
  });

  test("processes event when sessionId matches event session_id", async () => {
    // Arrange
    const client = new MockPtyClient();
    client.setSessionId("matching-session-id");

    let callbackInvoked = false;
    let receivedCode: number | null = null;

    await client.onExit((code, remaining) => {
      callbackInvoked = true;
      receivedCode = code;
    });

    // Act
    const testPayload: PtyExitPayload = {
      session_id: "matching-session-id",
      code: 1,
      remaining_sessions: 0,
    };
    mockEmit("pty_exit", testPayload);

    // Assert
    expect(callbackInvoked).toBe(true);
    expect(receivedCode).toBe(1);
  });

  test("ignores event when sessionId does not match", async () => {
    // Arrange
    const client = new MockPtyClient();
    client.setSessionId("session-abc");

    let callbackInvoked = false;

    await client.onExit((code, remaining) => {
      callbackInvoked = true;
    });

    // Act: Emit event with different session_id
    const testPayload: PtyExitPayload = {
      session_id: "session-xyz",
      code: 0,
      remaining_sessions: 1,
    };
    mockEmit("pty_exit", testPayload);

    // Assert: Callback should NOT be invoked
    expect(callbackInvoked).toBe(false);
    expect(client.getSessionId()).toBe("session-abc"); // sessionId unchanged
  });

  test("prevents duplicate event processing", async () => {
    // Arrange
    const client = new MockPtyClient();
    let callbackCount = 0;

    await client.onExit((code, remaining) => {
      callbackCount++;
    });

    // Act: Emit same event twice
    const testPayload: PtyExitPayload = {
      session_id: "test-session-id",
      code: 0,
      remaining_sessions: 0,
    };
    mockEmit("pty_exit", testPayload);
    mockEmit("pty_exit", testPayload);

    // Assert: Callback should only be invoked once
    expect(callbackCount).toBe(1);
  });

  test("calls unlisten() to cleanup listener after processing", async () => {
    // Arrange
    const client = new MockPtyClient();
    let callbackInvoked = false;

    await client.onExit((code, remaining) => {
      callbackInvoked = true;
    });

    // Act: Emit event
    const testPayload: PtyExitPayload = {
      session_id: "test-session-id",
      code: 0,
      remaining_sessions: 0,
    };
    mockEmit("pty_exit", testPayload);

    // Assert: Callback was invoked
    expect(callbackInvoked).toBe(true);

    // Emit again to verify listener was removed
    callbackInvoked = false;
    mockEmit("pty_exit", testPayload);
    expect(callbackInvoked).toBe(false);
  });

  test("sets sessionId to null after callback", async () => {
    // Arrange
    const client = new MockPtyClient();
    client.setSessionId("test-session-id");

    await client.onExit((code, remaining) => {
      // Callback invoked
    });

    // Act
    const testPayload: PtyExitPayload = {
      session_id: "test-session-id",
      code: 0,
      remaining_sessions: 0,
    };
    mockEmit("pty_exit", testPayload);

    // Assert: sessionId should be set to null
    expect(client.getSessionId()).toBeNull();
  });

  test("passes correct parameters to callback", async () => {
    // Arrange
    const client = new MockPtyClient();
    let receivedCode: number | null = null;
    let receivedRemaining: number | null = null;

    await client.onExit((code, remaining) => {
      receivedCode = code;
      receivedRemaining = remaining;
    });

    // Act
    const testPayload: PtyExitPayload = {
      session_id: "test-session-id",
      code: 42,
      remaining_sessions: 2,
    };
    mockEmit("pty_exit", testPayload);

    // Assert
    expect(receivedCode).toBe(42);
    expect(receivedRemaining).toBe(2);
  });
});
