/**
 * Animation controller for managing animated images.
 *
 * Handles Kitty animation frames and GIF playback with visibility-based pause.
 *
 * @module image/animation
 */

import type {
  AnimationState,
  AnimationEvent,
  AnimationFrameData,
  ActiveAnimation,
} from "./types.ts";

/**
 * Callback for frame updates.
 */
export type FrameUpdateCallback = (
  imageId: number,
  bitmap: ImageBitmap
) => void;

/**
 * Callback for animation completion.
 */
export type AnimationCompleteCallback = (imageId: number) => void;

/**
 * Animation controller for managing animated images.
 *
 * Features:
 * - Manages animation frame timing
 * - Supports visibility-based auto-pause
 * - Handles Kitty animation control commands
 */
export class AnimationController {
  /** Active animations by image ID. */
  private animations: Map<number, ActiveAnimation> = new Map();

  /** Frame update callback. */
  private onFrameUpdate: FrameUpdateCallback | null = null;

  /** Animation complete callback. */
  private onAnimationComplete: AnimationCompleteCallback | null = null;

  /**
   * Set the frame update callback.
   *
   * @param callback - Called when a new frame should be displayed
   */
  setFrameUpdateCallback(callback: FrameUpdateCallback): void {
    this.onFrameUpdate = callback;
  }

  /**
   * Set the animation complete callback.
   *
   * @param callback - Called when an animation completes all loops
   */
  setAnimationCompleteCallback(callback: AnimationCompleteCallback): void {
    this.onAnimationComplete = callback;
  }

  /**
   * Handle an animation event from the backend.
   *
   * @param event - Animation event
   */
  async handleEvent(event: AnimationEvent): Promise<void> {
    switch (event.type) {
      case "FrameReady":
        await this.handleFrameReady(event);
        break;
      case "StateChanged":
        this.handleStateChanged(event);
        break;
      case "Completed":
        this.handleCompleted(event);
        break;
    }
  }

  /**
   * Handle frame ready event.
   */
  private async handleFrameReady(event: AnimationEvent & { type: "FrameReady" }): Promise<void> {
    const { image_id, frame_number, delay_ms, rgba_base64, width, height } = event;

    // Get or create animation
    let animation = this.animations.get(image_id);
    if (!animation) {
      animation = this.createAnimation(image_id);
      this.animations.set(image_id, animation);
    }

    // Decode frame data
    const bitmap = await this.decodeToBitmap(rgba_base64, width, height);

    // Store frame
    const frame: AnimationFrameData = {
      frameNumber: frame_number,
      delayMs: delay_ms,
      width,
      height,
      bitmap,
    };
    animation.frames.set(frame_number, frame);

    // If this is the current frame, notify
    if (animation.currentFrame === frame_number && bitmap) {
      this.onFrameUpdate?.(image_id, bitmap);
    }
  }

  /**
   * Handle state changed event.
   */
  private handleStateChanged(event: AnimationEvent & { type: "StateChanged" }): void {
    const { image_id, state } = event;

    const animation = this.animations.get(image_id);
    if (!animation) return;

    const previousState = animation.state;
    animation.state = state;

    // Handle state transitions
    if (state === "Playing" && previousState !== "Playing") {
      this.startPlayback(animation);
    } else if (state !== "Playing" && previousState === "Playing") {
      this.stopPlayback(animation);
    }
  }

  /**
   * Handle animation completed event.
   */
  private handleCompleted(event: AnimationEvent & { type: "Completed" }): void {
    const { image_id } = event;

    const animation = this.animations.get(image_id);
    if (animation) {
      this.stopPlayback(animation);
      animation.state = "Stopped";
    }

    this.onAnimationComplete?.(image_id);
  }

  /**
   * Create a new animation state.
   */
  private createAnimation(imageId: number): ActiveAnimation {
    return {
      imageId,
      frames: new Map(),
      currentFrame: 1,
      state: "Loading",
      loopCount: 0,
      currentLoop: 0,
      timerId: null,
      isVisible: true,
    };
  }

  /**
   * Decode base64 RGBA data to ImageBitmap.
   */
  private async decodeToBitmap(
    base64: string,
    width: number,
    height: number
  ): Promise<ImageBitmap | null> {
    try {
      const binaryString = atob(base64);
      const bytes = new Uint8ClampedArray(binaryString.length);
      for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
      }

      const imageData = new ImageData(bytes, width, height);
      return await createImageBitmap(imageData);
    } catch (e) {
      console.warn("Failed to decode animation frame:", e);
      return null;
    }
  }

  /**
   * Start animation playback.
   */
  private startPlayback(animation: ActiveAnimation): void {
    if (animation.timerId !== null) {
      return; // Already playing
    }

    if (!animation.isVisible) {
      return; // Don't play if not visible
    }

    this.scheduleNextFrame(animation);
  }

  /**
   * Stop animation playback.
   */
  private stopPlayback(animation: ActiveAnimation): void {
    if (animation.timerId !== null) {
      clearTimeout(animation.timerId);
      animation.timerId = null;
    }
  }

  /**
   * Schedule the next frame to display.
   */
  private scheduleNextFrame(animation: ActiveAnimation): void {
    const currentFrame = animation.frames.get(animation.currentFrame);
    const delay = currentFrame?.delayMs ?? 40;

    animation.timerId = window.setTimeout(() => {
      this.advanceFrame(animation);
    }, delay);
  }

  /**
   * Advance to the next frame.
   */
  private advanceFrame(animation: ActiveAnimation): void {
    animation.timerId = null;

    if (animation.state !== "Playing" || !animation.isVisible) {
      return;
    }

    // Calculate next frame
    const frameCount = animation.frames.size;
    if (frameCount === 0) return;

    const maxFrame = Math.max(...animation.frames.keys());
    let nextFrame = animation.currentFrame + 1;

    if (nextFrame > maxFrame) {
      // Loop handling
      if (animation.loopCount === 0) {
        // Infinite loop
        nextFrame = 1;
      } else {
        animation.currentLoop++;
        if (animation.currentLoop >= animation.loopCount) {
          // Animation complete
          animation.state = "Stopped";
          this.onAnimationComplete?.(animation.imageId);
          return;
        }
        nextFrame = 1;
      }
    }

    animation.currentFrame = nextFrame;

    // Get and display the frame
    const frame = animation.frames.get(nextFrame);
    if (frame?.bitmap) {
      this.onFrameUpdate?.(animation.imageId, frame.bitmap);
    }

    // Schedule next frame
    this.scheduleNextFrame(animation);
  }

  /**
   * Set the current frame for an animation.
   *
   * @param imageId - Image ID
   * @param frameNumber - Frame number to display
   */
  setCurrentFrame(imageId: number, frameNumber: number): void {
    const animation = this.animations.get(imageId);
    if (!animation) return;

    animation.currentFrame = frameNumber;

    const frame = animation.frames.get(frameNumber);
    if (frame?.bitmap) {
      this.onFrameUpdate?.(imageId, frame.bitmap);
    }
  }

  /**
   * Set animation visibility.
   *
   * Pauses animation when not visible, resumes when visible again.
   *
   * @param imageId - Image ID
   * @param isVisible - Whether the animation is visible
   */
  setVisibility(imageId: number, isVisible: boolean): void {
    const animation = this.animations.get(imageId);
    if (!animation) return;

    const wasVisible = animation.isVisible;
    animation.isVisible = isVisible;

    if (animation.state === "Playing") {
      if (isVisible && !wasVisible) {
        // Resume playback
        this.startPlayback(animation);
      } else if (!isVisible && wasVisible) {
        // Pause playback
        this.stopPlayback(animation);
      }
    }
  }

  /**
   * Set visibility for all animations.
   *
   * @param isVisible - Whether animations are visible
   */
  setAllVisibility(isVisible: boolean): void {
    for (const animation of this.animations.values()) {
      this.setVisibility(animation.imageId, isVisible);
    }
  }

  /**
   * Get an animation by image ID.
   *
   * @param imageId - Image ID
   * @returns Animation state or undefined
   */
  getAnimation(imageId: number): ActiveAnimation | undefined {
    return this.animations.get(imageId);
  }

  /**
   * Check if an animation exists.
   *
   * @param imageId - Image ID
   * @returns True if animation exists
   */
  hasAnimation(imageId: number): boolean {
    return this.animations.has(imageId);
  }

  /**
   * Remove an animation.
   *
   * @param imageId - Image ID
   */
  removeAnimation(imageId: number): void {
    const animation = this.animations.get(imageId);
    if (animation) {
      this.stopPlayback(animation);

      // Release bitmaps
      for (const frame of animation.frames.values()) {
        if (frame.bitmap) {
          frame.bitmap.close();
        }
      }

      this.animations.delete(imageId);
    }
  }

  /**
   * Clear all animations.
   */
  clear(): void {
    for (const animation of this.animations.values()) {
      this.stopPlayback(animation);

      // Release bitmaps
      for (const frame of animation.frames.values()) {
        if (frame.bitmap) {
          frame.bitmap.close();
        }
      }
    }

    this.animations.clear();
  }

  /**
   * Get animation count.
   */
  get animationCount(): number {
    return this.animations.size;
  }

  /**
   * Get current bitmap for an animation.
   *
   * @param imageId - Image ID
   * @returns Current frame bitmap or null
   */
  getCurrentBitmap(imageId: number): ImageBitmap | null {
    const animation = this.animations.get(imageId);
    if (!animation) return null;

    const frame = animation.frames.get(animation.currentFrame);
    return frame?.bitmap ?? null;
  }

  /**
   * Dispose of the animation controller.
   */
  dispose(): void {
    this.clear();
    this.onFrameUpdate = null;
    this.onAnimationComplete = null;
  }
}
