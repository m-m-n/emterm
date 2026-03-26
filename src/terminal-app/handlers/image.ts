/**
 * Image processing handler for terminal application.
 * Handles Kitty Graphics Protocol (APC) and SIXEL (DCS) image processing,
 * image event listening, and ImageViewer lifecycle.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { ImageViewer } from "../../image-viewer";
import type { DecodedImage, ImagePlacement } from "../../image/types";
import type { ImageEventPayload } from "../../types/terminal";
import type { PtyClient } from "../../pty/client";
import type { TerminalState } from "../../terminal/state";
import type { ITerminalRenderer } from "../../terminal";
import { handleMuxApc } from "../../terminal/handlers/apc_handlers";

/**
 * Context interface for ImageHandler dependencies.
 * Uses getter functions to allow lazy/nullable access to shared state.
 */
export interface ImageHandlerContext {
  getPtyClient: () => PtyClient | null;
  getState: () => TerminalState | null;
  getRenderer: () => ITerminalRenderer | null;
  getImeHandler: () => { blur(): void; focus(): void } | null;
  getOverlayRoot: () => HTMLElement | null;
}

/**
 * Handles image processing for the terminal application.
 * Manages Kitty Graphics Protocol (APC), SIXEL (DCS), image events,
 * and the ImageViewer overlay.
 */
export class ImageHandler {
  private context: ImageHandlerContext;
  private pendingImages: Map<number, DecodedImage> = new Map();
  private imageEventUnlisten: UnlistenFn | null = null;
  private imageViewer: ImageViewer | null = null;
  private imageInvokeChain: Promise<void> = Promise.resolve();
  private fetchingImages: Map<number, Promise<DecodedImage>> = new Map();
  private pendingPlacements: Map<number, ImagePlacement> = new Map();
  private kittyTransfer: {
    firstChunk: string;
    accumulatedPayload: string;
  } | null = null;
  private pendingApcQueue: Uint8Array[] = [];
  private pendingDcsQueue: Uint8Array[] = [];

  constructor(context: ImageHandlerContext) {
    this.context = context;
  }

  /**
   * Initialize the image handler: create ImageViewer and set up event listener.
   */
  async init(): Promise<void> {
    const overlayRoot = this.context.getOverlayRoot();
    if (overlayRoot) {
      this.imageViewer = new ImageViewer(overlayRoot);
      this.imageViewer.onShow(() => {
        // Blur IME input to prevent EditContext/textarea from intercepting keyboard events.
        // Without this, the IME input mechanism consumes keys like '1', '0', 'f'
        // before DisplayModeController's capture-phase handler can process them.
        this.context.getImeHandler()?.blur();
      });
      this.imageViewer.onHide(() => {
        // Force re-render after image viewer closes (e.g. via Escape key)
        const state = this.context.getState();
        const renderer = this.context.getRenderer();
        if (state && renderer) {
          renderer.forceRender(state);
        }
        // Restore IME focus for terminal input
        this.context.getImeHandler()?.focus();
      });
    }

    await this.setupImageEventListener();
  }

  /**
   * Queue an APC data chunk for deferred processing.
   * Called from WASM callback context where core cannot be accessed.
   *
   * Mux APC messages (emterm-mux; prefix) are handled immediately
   * since they don't interact with the WASM core.
   */
  queueApc(data: Uint8Array): void {
    // Intercept mux APC messages before queuing
    if (handleMuxApc(data)) return;
    this.pendingApcQueue.push(new Uint8Array(data));
  }

  /**
   * Queue a DCS data chunk for deferred processing.
   * Called from WASM callback context where core cannot be accessed.
   */
  queueDcs(data: Uint8Array): void {
    this.pendingDcsQueue.push(new Uint8Array(data));
  }

  /**
   * Process all queued APC events.
   * Safe to call after process_pty_data has returned.
   */
  processPendingApcQueue(): void {
    if (this.pendingApcQueue.length === 0) return;
    const apcEvents = this.pendingApcQueue;
    this.pendingApcQueue = [];
    for (const apcData of apcEvents) {
      this.handleApcCallback(apcData);
    }
  }

  /**
   * Process all queued DCS events.
   * Safe to call after process_pty_data has returned.
   */
  processPendingDcsQueue(): void {
    if (this.pendingDcsQueue.length === 0) return;
    const dcsEvents = this.pendingDcsQueue;
    this.pendingDcsQueue = [];
    for (const dcsData of dcsEvents) {
      this.handleDcsCallback(dcsData);
    }
  }

  /**
   * Handle APC callback from WASM parser (Kitty Graphics Protocol).
   *
   * Accumulates Kitty APC chunks and sends them as a single batch invoke
   * when the final chunk (m=0) arrives. This reduces ~600 IPC round-trips
   * to 1 for a typical large image transfer, preventing CLI timeout.
   */
  handleApcCallback(data: Uint8Array): void {
    const state = this.context.getState();
    const ptyClient = this.context.getPtyClient();
    const core = state?.getActiveCore();
    if (!core || !ptyClient) return;

    // APC body is ASCII text: "G<params>;<payload>"
    const body = new TextDecoder().decode(data);

    // Only process Kitty Graphics APC (starts with 'G')
    if (body.length === 0 || body[0] !== "G") {
      return;
    }

    // Split into params and payload at first semicolon
    const semicolonIdx = body.indexOf(";");
    const params = semicolonIdx >= 0 ? body.substring(0, semicolonIdx) : body;
    const payload = semicolonIdx >= 0 ? body.substring(semicolonIdx + 1) : "";
    const isMore = /(?:^|,)m=1(?:,|$)/.test(params);

    if (isMore) {
      // Continuation chunk: accumulate base64 payload
      if (!this.kittyTransfer) {
        // First chunk of a multi-chunk transfer
        this.kittyTransfer = { firstChunk: body, accumulatedPayload: payload };
      } else {
        // Middle chunk: just append payload
        this.kittyTransfer.accumulatedPayload += payload;
      }
      return;
    }

    // Final chunk (m=0 or single-chunk transfer)
    const sessionId = ptyClient.getSessionId();
    const cursorRow = core.get_cursor_row();
    const cursorCol = core.get_cursor_col();

    if (this.kittyTransfer) {
      // Multi-chunk transfer: assemble first chunk + accumulated payload + final payload
      this.kittyTransfer.accumulatedPayload += payload;
      const assembled = this.kittyTransfer;
      this.kittyTransfer = null;

      // Build two-chunk representation: first chunk (with params) + final chunk (with full payload)
      // The first chunk provides format, image_id, etc.
      // We replace its payload with the full accumulated payload.
      const firstSemicolon = assembled.firstChunk.indexOf(";");
      const firstParams = firstSemicolon >= 0
        ? assembled.firstChunk.substring(0, firstSemicolon)
        : assembled.firstChunk;

      // Change m=1 to m=0 in first chunk params (since we're sending all data at once)
      const fixedParams = firstParams.replace(/,m=1(?:,|$)/, (m) =>
        m.endsWith(",") ? ",m=0," : ",m=0",
      );

      // Send as a single chunk with all data
      const fullChunk = fixedParams + ";" + assembled.accumulatedPayload;

      this.imageInvokeChain = this.imageInvokeChain.then(() =>
        invoke("process_kitty_batch", {
          sessionId,
          chunks: [fullChunk],
          cursorRow,
          cursorCol,
        }) as Promise<void>,
      ).catch((error) => {
        console.error("Failed to process Kitty batch:", error);
      });
    } else {
      // Single-chunk transfer: send directly
      this.imageInvokeChain = this.imageInvokeChain.then(() =>
        invoke("process_kitty_batch", {
          sessionId,
          chunks: [body],
          cursorRow,
          cursorCol,
        }) as Promise<void>,
      ).catch((error) => {
        console.error("Failed to process Kitty batch:", error);
      });
    }
  }

  /**
   * Handle DCS callback from WASM parser (SIXEL graphics).
   * Only processes SIXEL DCS (format: [params]q[data]).
   * Non-SIXEL DCS (e.g., DECRQSS "$q...") are ignored.
   */
  handleDcsCallback(data: Uint8Array): void {
    // SIXEL DCS body contains 'q' (0x71) as the command character.
    // Format: optional params (digits/semicolons) followed by 'q' then pixel data.
    // Skip non-SIXEL DCS sequences (e.g., DECRQSS, DECRPSS).
    const Q = 0x71; // 'q'
    let isSixel = false;
    for (let i = 0; i < data.length; i++) {
      const b = data[i]!;
      if (b === Q) {
        isSixel = true;
        break;
      }
      // SIXEL params are digits (0-9) and semicolons only
      if ((b < 0x30 || b > 0x39) && b !== 0x3B) break;
    }
    if (!isSixel) return;

    const state = this.context.getState();
    const ptyClient = this.context.getPtyClient();
    const core = state?.getActiveCore();
    if (!core || !ptyClient) return;

    const sessionId = ptyClient.getSessionId();
    const cursorRow = core.get_cursor_row();
    const cursorCol = core.get_cursor_col();

    // Share the same invoke chain as APC to serialize all image processing
    this.imageInvokeChain = this.imageInvokeChain.then(() =>
      invoke("process_image_data", {
        sessionId,
        protocol: "sixel",
        data: Array.from(data),
        cursorRow,
        cursorCol,
      }) as Promise<void>,
    ).catch((error) => {
      console.error("Failed to process SIXEL image data:", error);
    });
  }

  /**
   * Sets up image event listener for Kitty Graphics and SIXEL support.
   */
  private async setupImageEventListener(): Promise<void> {
    this.imageEventUnlisten = await listen<ImageEventPayload>(
      "image_event",
      (event: { payload: ImageEventPayload }) => {
        const ptyClient = this.context.getPtyClient();
        // Only process events for the current session
        if (
          ptyClient &&
          event.payload.session_id === ptyClient.getSessionId()
        ) {
          this.handleImageEvent(event.payload);
        }
      },
    );
  }

  /**
   * Handles image events from the backend.
   */
  private handleImageEvent(payload: ImageEventPayload): void {
    const eventType = payload.type;

    switch (eventType) {
      case "ImageReady": {
        const image = payload.image as DecodedImage;
        if (!image.rgba_base64) {
          // Large image: data deferred to fetch_image_data command
          const fetchPromise = this.fetchDeferredImageData(image);
          this.fetchingImages.set(image.id, fetchPromise);
        } else {
          this.pendingImages.set(image.id, image);
        }
        break;
      }

      case "Place": {
        // Display the image at the specified position
        const placement = payload.placement;
        if (!placement) {
          console.warn("[WARN][FRONTEND] Place event without placement data");
          break;
        }
        const image = this.pendingImages.get(placement.image_id);
        if (image && this.imageViewer) {
          // For fullscreen display mode
          this.imageViewer.show(image);
        } else {
          // Image data may still be fetching (large image deferred load)
          const fetchPromise = this.fetchingImages.get(placement.image_id);
          if (fetchPromise) {
            this.pendingPlacements.set(placement.image_id, placement);
          } else {
            console.warn(
              `[WARN][FRONTEND] Image not found for placement: ${placement.image_id}`,
            );
          }
        }
        break;
      }

      case "Delete": {
        // Handle image deletion
        const target = payload.target;
        if (!target) {
          console.warn("[WARN][FRONTEND] Delete event without target data");
          break;
        }
        if (target.type === "All" || target.type === "AllIncludingHidden") {
          this.pendingImages.clear();
          this.imageViewer?.hide();
          // Force re-render after closing image viewer to show correct state
          const state = this.context.getState();
          const renderer = this.context.getRenderer();
          if (state && renderer) {
            renderer.forceRender(state);
          }
        } else if (target.type === "ById" && target.id !== undefined) {
          this.pendingImages.delete(target.id);
        }
        break;
      }

      case "QueryResponse": {
        // Handle query response (graphics protocol supported)
        console.info(
          `[INFO][FRONTEND] Graphics supported: ${payload.supported}`,
        );
        break;
      }

      case "Animation": {
        // Handle animation events
        if (this.imageViewer && payload.data) {
          this.imageViewer.handleAnimationEvent(
            payload.data as import("../../image/types").AnimationEvent,
          );
        }
        break;
      }

      case "Response": {
        // Synchronous response is now generated by WASM during
        // process_pty_data(), ensuring the response reaches the PTY
        // while the originating process still has raw mode active.
        // Backend Response events are no longer written to PTY.
        break;
      }

    }
  }

  /**
   * Fetches large image data deferred from the event payload.
   *
   * When rgba_base64 exceeds the backend threshold, it is stored separately
   * and must be retrieved via the fetch_image_data command. After fetching,
   * the image is added to pendingImages and any deferred placement is executed.
   */
  private async fetchDeferredImageData(image: DecodedImage): Promise<DecodedImage> {
    try {
      const sessionId = this.context.getPtyClient()?.getSessionId();
      if (!sessionId) throw new Error("No active PTY session");

      const data = await invoke<string>("fetch_image_data", {
        sessionId,
        imageId: image.id,
      });
      image.rgba_base64 = data;
      this.pendingImages.set(image.id, image);
      this.fetchingImages.delete(image.id);

      // Execute deferred placement if any
      const placement = this.pendingPlacements.get(image.id);
      if (placement && this.imageViewer) {
        this.pendingPlacements.delete(image.id);
        this.imageViewer.show(image);
      }

      return image;
    } catch (error) {
      console.error(`Failed to fetch deferred image data for id=${image.id}:`, error);
      this.fetchingImages.delete(image.id);
      this.pendingPlacements.delete(image.id);
      throw error;
    }
  }

  /**
   * Display a decoded image using the image viewer.
   * Used by OSC 1337;File inline image handler.
   */
  showImage(image: DecodedImage): void {
    if (this.imageViewer) {
      this.imageViewer.show(image);
    }
  }

  /**
   * Clean up all image-related resources.
   */
  dispose(): void {
    if (this.imageEventUnlisten) {
      this.imageEventUnlisten();
      this.imageEventUnlisten = null;
    }
    this.imageViewer?.dispose();
    this.imageViewer = null;
    this.pendingImages.clear();
    this.fetchingImages.clear();
    this.pendingPlacements.clear();
    this.kittyTransfer = null;
    this.pendingApcQueue = [];
    this.pendingDcsQueue = [];
  }
}
