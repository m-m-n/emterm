/**
 * Animation controller tests.
 *
 * @module image/animation.test
 */

import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import type {
	ActiveAnimation,
	AnimationEvent,
	AnimationFrameData,
	AnimationState,
} from "./types.ts";

// Mock ImageBitmap
const createMockBitmap = () => ({
	width: 10,
	height: 10,
	close: mock(() => {}),
});

// Mock createImageBitmap
globalThis.createImageBitmap = mock(async () =>
	createMockBitmap(),
) as unknown as typeof createImageBitmap;

// Mock ImageData
class MockImageData {
	data: Uint8ClampedArray;
	width: number;
	height: number;

	constructor(
		data: Uint8ClampedArray | number,
		widthOrHeight?: number,
		height?: number,
	) {
		if (typeof data === "number") {
			this.width = data;
			this.height = widthOrHeight!;
			this.data = new Uint8ClampedArray(data * widthOrHeight! * 4);
		} else {
			this.data = data;
			this.width = widthOrHeight!;
			this.height = height!;
		}
	}
}
globalThis.ImageData = MockImageData as unknown as typeof ImageData;

// Mock atob for base64 decoding
globalThis.atob = (data: string) => {
	return Buffer.from(data, "base64").toString("binary");
};

// Timer mocks
const timeoutCallbacks: Map<number, { callback: () => void; delay: number }> =
	new Map();
let nextTimerId = 1;
let currentTime = 0;

const mockSetTimeout = mock((callback: () => void, delay: number): number => {
	const id = nextTimerId++;
	timeoutCallbacks.set(id, { callback, delay: currentTime + delay });
	return id;
});

const mockClearTimeout = mock((id: number): void => {
	timeoutCallbacks.delete(id);
});

// Setup window mock with timers
globalThis.window = {
	setTimeout: mockSetTimeout,
	clearTimeout: mockClearTimeout,
} as unknown as Window & typeof globalThis;

// Also mock globalThis.clearTimeout since implementation uses clearTimeout directly
globalThis.clearTimeout = mockClearTimeout as unknown as typeof clearTimeout;

// Helper to advance time and execute callbacks
function advanceTimersByTime(ms: number): void {
	currentTime += ms;
	const toExecute: (() => void)[] = [];

	for (const [id, { callback, delay }] of timeoutCallbacks.entries()) {
		if (delay <= currentTime) {
			toExecute.push(callback);
			timeoutCallbacks.delete(id);
		}
	}

	for (const callback of toExecute) {
		callback();
	}
}

function flushTimers(): void {
	let iterations = 0;
	const maxIterations = 100;

	while (timeoutCallbacks.size > 0 && iterations < maxIterations) {
		const minDelay = Math.min(
			...Array.from(timeoutCallbacks.values()).map((t) => t.delay),
		);
		currentTime = minDelay;

		const toExecute: (() => void)[] = [];
		for (const [id, { callback, delay }] of timeoutCallbacks.entries()) {
			if (delay <= currentTime) {
				toExecute.push(callback);
				timeoutCallbacks.delete(id);
			}
		}

		for (const callback of toExecute) {
			callback();
		}
		iterations++;
	}
}

// Import after mocks are set up
import { AnimationController } from "./animation.ts";

// Helper for creating base64 encoded RGBA data
function createBase64Rgba(width: number, height: number): string {
	const size = width * height * 4;
	const data = new Uint8Array(size);
	for (let i = 0; i < size; i += 4) {
		data[i] = 255; // R
		data[i + 1] = 0; // G
		data[i + 2] = 0; // B
		data[i + 3] = 255; // A
	}
	return Buffer.from(data).toString("base64");
}

describe("AnimationController", () => {
	let controller: AnimationController;

	beforeEach(() => {
		// Reset timer state
		timeoutCallbacks.clear();
		nextTimerId = 1;
		currentTime = 0;
		mockSetTimeout.mockClear();
		mockClearTimeout.mockClear();

		controller = new AnimationController();
	});

	afterEach(() => {
		controller.dispose();
	});

	describe("constructor / initialization", () => {
		test("creates controller with no animations", () => {
			expect(controller.animationCount).toBe(0);
		});

		test("hasAnimation returns false for non-existent animation", () => {
			expect(controller.hasAnimation(999)).toBe(false);
		});

		test("getAnimation returns undefined for non-existent animation", () => {
			expect(controller.getAnimation(999)).toBeUndefined();
		});
	});

	describe("setFrameUpdateCallback", () => {
		test("sets frame update callback", async () => {
			const callback = mock(() => {});
			controller.setFrameUpdateCallback(callback);

			// Add a frame and trigger update
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};

			await controller.handleEvent(event);

			// Frame update should be called for current frame
			expect(callback).toHaveBeenCalled();
		});
	});

	describe("setAnimationCompleteCallback", () => {
		test("sets animation complete callback", async () => {
			const callback = mock(() => {});
			controller.setAnimationCompleteCallback(callback);

			// First add a frame
			const frameEvent: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(frameEvent);

			// Then complete animation
			const completeEvent: AnimationEvent = {
				type: "Completed",
				image_id: 1,
			};
			await controller.handleEvent(completeEvent);

			expect(callback).toHaveBeenCalledWith(1);
		});
	});

	describe("handleEvent - FrameReady", () => {
		test("creates animation when receiving first frame", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};

			await controller.handleEvent(event);

			expect(controller.hasAnimation(1)).toBe(true);
			expect(controller.animationCount).toBe(1);
		});

		test("stores frame data correctly", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 2,
				delay_ms: 150,
				rgba_base64: createBase64Rgba(10, 10),
				width: 10,
				height: 10,
			};

			await controller.handleEvent(event);

			const animation = controller.getAnimation(1);
			expect(animation).toBeDefined();
			expect(animation!.frames.has(2)).toBe(true);

			const frame = animation!.frames.get(2);
			expect(frame?.delayMs).toBe(150);
			expect(frame?.width).toBe(10);
			expect(frame?.height).toBe(10);
		});

		test("adds multiple frames to same animation", async () => {
			for (let i = 1; i <= 3; i++) {
				const event: AnimationEvent = {
					type: "FrameReady",
					image_id: 1,
					frame_number: i,
					delay_ms: 100,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(event);
			}

			const animation = controller.getAnimation(1);
			expect(animation!.frames.size).toBe(3);
		});

		test("notifies callback when current frame is received", async () => {
			const callback = mock(() => {});
			controller.setFrameUpdateCallback(callback);

			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1, // First frame is current frame
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};

			await controller.handleEvent(event);

			expect(callback).toHaveBeenCalledWith(1, expect.anything());
		});
	});

	describe("handleEvent - StateChanged", () => {
		test("updates animation state", async () => {
			// First create animation
			const frameEvent: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(frameEvent);

			// Change state
			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			const animation = controller.getAnimation(1);
			expect(animation!.state).toBe("Playing");
		});

		test("starts playback when state changes to Playing", async () => {
			const frameEvent: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(frameEvent);

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			// Timer should be scheduled
			expect(mockSetTimeout).toHaveBeenCalled();
		});

		test("stops playback when state changes from Playing", async () => {
			// Create and start animation
			const frameEvent: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(frameEvent);

			const playEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(playEvent);

			// Now pause
			const pauseEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Paused",
			};
			await controller.handleEvent(pauseEvent);

			expect(mockClearTimeout).toHaveBeenCalled();
		});

		test("ignores state change for non-existent animation", async () => {
			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 999,
				state: "Playing",
			};

			// Should not throw
			await controller.handleEvent(stateEvent);
		});
	});

	describe("handleEvent - Completed", () => {
		test("stops playback and updates state", async () => {
			const frameEvent: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(frameEvent);

			const completeEvent: AnimationEvent = {
				type: "Completed",
				image_id: 1,
			};
			await controller.handleEvent(completeEvent);

			const animation = controller.getAnimation(1);
			expect(animation!.state).toBe("Stopped");
		});

		test("calls completion callback", async () => {
			const callback = mock(() => {});
			controller.setAnimationCompleteCallback(callback);

			const frameEvent: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(frameEvent);

			const completeEvent: AnimationEvent = {
				type: "Completed",
				image_id: 1,
			};
			await controller.handleEvent(completeEvent);

			expect(callback).toHaveBeenCalledWith(1);
		});
	});

	describe("setCurrentFrame", () => {
		test("updates current frame", async () => {
			// Add multiple frames
			for (let i = 1; i <= 3; i++) {
				const event: AnimationEvent = {
					type: "FrameReady",
					image_id: 1,
					frame_number: i,
					delay_ms: 100,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(event);
			}

			controller.setCurrentFrame(1, 2);

			const animation = controller.getAnimation(1);
			expect(animation!.currentFrame).toBe(2);
		});

		test("notifies callback when frame has bitmap", async () => {
			const callback = mock(() => {});
			controller.setFrameUpdateCallback(callback);

			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			callback.mockClear();
			controller.setCurrentFrame(1, 1);

			expect(callback).toHaveBeenCalledWith(1, expect.anything());
		});

		test("ignores non-existent animation", () => {
			// Should not throw
			controller.setCurrentFrame(999, 1);
		});
	});

	describe("setVisibility", () => {
		test("updates visibility state", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			controller.setVisibility(1, false);

			const animation = controller.getAnimation(1);
			expect(animation!.isVisible).toBe(false);
		});

		test("pauses playback when becoming invisible", async () => {
			// Create and start animation
			const frameEvent: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(frameEvent);

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			// Make invisible
			controller.setVisibility(1, false);

			expect(mockClearTimeout).toHaveBeenCalled();
		});

		test("resumes playback when becoming visible again", async () => {
			// Create animation
			const frameEvent: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(frameEvent);

			// Set to playing
			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			// Make invisible
			controller.setVisibility(1, false);
			mockSetTimeout.mockClear();

			// Make visible again
			controller.setVisibility(1, true);

			// Timer should be scheduled again
			expect(mockSetTimeout).toHaveBeenCalled();
		});

		test("ignores non-existent animation", () => {
			// Should not throw
			controller.setVisibility(999, false);
		});
	});

	describe("setAllVisibility", () => {
		test("updates visibility for all animations", async () => {
			// Create two animations
			for (const imageId of [1, 2]) {
				const event: AnimationEvent = {
					type: "FrameReady",
					image_id: imageId,
					frame_number: 1,
					delay_ms: 100,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(event);
			}

			controller.setAllVisibility(false);

			expect(controller.getAnimation(1)!.isVisible).toBe(false);
			expect(controller.getAnimation(2)!.isVisible).toBe(false);
		});
	});

	describe("getCurrentBitmap", () => {
		test("returns bitmap for current frame", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			const bitmap = controller.getCurrentBitmap(1);
			expect(bitmap).not.toBeNull();
		});

		test("returns null for non-existent animation", () => {
			const bitmap = controller.getCurrentBitmap(999);
			expect(bitmap).toBeNull();
		});

		test("returns null when current frame has no bitmap", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 2, // Not frame 1
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			// Animation starts at frame 1, but only frame 2 has data
			const bitmap = controller.getCurrentBitmap(1);
			expect(bitmap).toBeNull();
		});
	});

	describe("removeAnimation", () => {
		test("removes animation from controller", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			expect(controller.hasAnimation(1)).toBe(true);

			controller.removeAnimation(1);

			expect(controller.hasAnimation(1)).toBe(false);
			expect(controller.animationCount).toBe(0);
		});

		test("stops playback before removal", async () => {
			const frameEvent: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(frameEvent);

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			controller.removeAnimation(1);

			expect(mockClearTimeout).toHaveBeenCalled();
		});

		test("closes bitmaps when removing", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			const animation = controller.getAnimation(1);
			const frame = animation!.frames.get(1);
			const bitmap = frame!.bitmap as { close: ReturnType<typeof mock> };

			controller.removeAnimation(1);

			expect(bitmap.close).toHaveBeenCalled();
		});

		test("ignores non-existent animation", () => {
			// Should not throw
			controller.removeAnimation(999);
		});
	});

	describe("clear", () => {
		test("removes all animations", async () => {
			for (const imageId of [1, 2, 3]) {
				const event: AnimationEvent = {
					type: "FrameReady",
					image_id: imageId,
					frame_number: 1,
					delay_ms: 100,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(event);
			}

			expect(controller.animationCount).toBe(3);

			controller.clear();

			expect(controller.animationCount).toBe(0);
		});

		test("stops all playback", async () => {
			for (const imageId of [1, 2]) {
				const frameEvent: AnimationEvent = {
					type: "FrameReady",
					image_id: imageId,
					frame_number: 1,
					delay_ms: 100,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(frameEvent);

				const stateEvent: AnimationEvent = {
					type: "StateChanged",
					image_id: imageId,
					state: "Playing",
				};
				await controller.handleEvent(stateEvent);
			}

			mockClearTimeout.mockClear();
			controller.clear();

			// Should have cleared both timers
			expect(mockClearTimeout.mock.calls.length).toBeGreaterThanOrEqual(2);
		});
	});

	describe("dispose", () => {
		test("clears all resources", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			controller.dispose();

			expect(controller.animationCount).toBe(0);
		});

		test("clears callbacks", async () => {
			const frameCallback = mock(() => {});
			const completeCallback = mock(() => {});

			controller.setFrameUpdateCallback(frameCallback);
			controller.setAnimationCompleteCallback(completeCallback);

			controller.dispose();

			// Create a new controller to verify callbacks were cleared on the original
			// (The callbacks should be nullified)
		});
	});

	describe("frame timing and playback", () => {
		test("uses frame delay for timing", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 200,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			// Verify setTimeout was called with the correct delay
			expect(mockSetTimeout).toHaveBeenCalledWith(expect.any(Function), 200);
		});

		test("uses default delay when frame delay is missing", async () => {
			// Create animation without frame data for current frame
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 2, // Not frame 1
				delay_ms: 100,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			// Default delay is 40ms
			expect(mockSetTimeout).toHaveBeenCalledWith(expect.any(Function), 40);
		});

		test("advances to next frame after delay", async () => {
			// Add two frames
			for (let i = 1; i <= 2; i++) {
				const event: AnimationEvent = {
					type: "FrameReady",
					image_id: 1,
					frame_number: i,
					delay_ms: 100,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(event);
			}

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			const animation = controller.getAnimation(1);
			expect(animation!.currentFrame).toBe(1);

			// Advance time
			advanceTimersByTime(100);

			expect(animation!.currentFrame).toBe(2);
		});

		test("loops back to first frame in infinite loop", async () => {
			// Add two frames
			for (let i = 1; i <= 2; i++) {
				const event: AnimationEvent = {
					type: "FrameReady",
					image_id: 1,
					frame_number: i,
					delay_ms: 50,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(event);
			}

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			const animation = controller.getAnimation(1);
			// loopCount defaults to 0 (infinite)
			expect(animation!.loopCount).toBe(0);

			// Advance past both frames
			advanceTimersByTime(50); // Frame 1 -> 2
			advanceTimersByTime(50); // Frame 2 -> 1 (loop)

			expect(animation!.currentFrame).toBe(1);
		});

		test("stops after completing specified loop count", async () => {
			// Add two frames
			for (let i = 1; i <= 2; i++) {
				const event: AnimationEvent = {
					type: "FrameReady",
					image_id: 1,
					frame_number: i,
					delay_ms: 50,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(event);
			}

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			const animation = controller.getAnimation(1);
			animation!.loopCount = 1; // Play once

			const completeCallback = mock(() => {});
			controller.setAnimationCompleteCallback(completeCallback);

			// Complete one loop
			advanceTimersByTime(50); // Frame 1 -> 2
			advanceTimersByTime(50); // Frame 2 -> done

			expect(animation!.state).toBe("Stopped");
			expect(completeCallback).toHaveBeenCalledWith(1);
		});

		test("stops advancing when not visible", async () => {
			// Add frames
			for (let i = 1; i <= 3; i++) {
				const event: AnimationEvent = {
					type: "FrameReady",
					image_id: 1,
					frame_number: i,
					delay_ms: 50,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(event);
			}

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			const animation = controller.getAnimation(1);

			// Advance one frame
			advanceTimersByTime(50);
			expect(animation!.currentFrame).toBe(2);

			// Make invisible
			controller.setVisibility(1, false);

			// Advance time - should not advance frame
			advanceTimersByTime(100);
			expect(animation!.currentFrame).toBe(2);
		});

		test("notifies frame update callback on each frame", async () => {
			const callback = mock(() => {});
			controller.setFrameUpdateCallback(callback);

			// Add two frames
			for (let i = 1; i <= 2; i++) {
				const event: AnimationEvent = {
					type: "FrameReady",
					image_id: 1,
					frame_number: i,
					delay_ms: 50,
					rgba_base64: createBase64Rgba(2, 2),
					width: 2,
					height: 2,
				};
				await controller.handleEvent(event);
			}

			callback.mockClear();

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			// Advance to next frame
			advanceTimersByTime(50);

			expect(callback).toHaveBeenCalledWith(1, expect.anything());
		});
	});

	describe("edge cases", () => {
		test("handles empty frames map gracefully", async () => {
			// Create animation
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 50,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			// Clear frames manually (edge case)
			const animation = controller.getAnimation(1);
			animation!.frames.clear();

			// Advance time - should not crash
			advanceTimersByTime(50);

			// Animation should still exist
			expect(controller.hasAnimation(1)).toBe(true);
		});

		test("handles state change while not playing", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 50,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			// Change from Loading to Paused (not Playing)
			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Paused",
			};
			await controller.handleEvent(stateEvent);

			const animation = controller.getAnimation(1);
			expect(animation!.state).toBe("Paused");
			expect(animation!.timerId).toBeNull();
		});

		test("does not start playback when already playing", async () => {
			const event: AnimationEvent = {
				type: "FrameReady",
				image_id: 1,
				frame_number: 1,
				delay_ms: 50,
				rgba_base64: createBase64Rgba(2, 2),
				width: 2,
				height: 2,
			};
			await controller.handleEvent(event);

			const stateEvent: AnimationEvent = {
				type: "StateChanged",
				image_id: 1,
				state: "Playing",
			};
			await controller.handleEvent(stateEvent);

			const callCountAfterFirstPlay = mockSetTimeout.mock.calls.length;

			// Try to play again
			await controller.handleEvent(stateEvent);

			// Should not schedule additional timer (already running)
			expect(mockSetTimeout.mock.calls.length).toBe(callCountAfterFirstPlay);
		});
	});
});
