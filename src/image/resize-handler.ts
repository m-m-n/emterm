/**
 * Resize handler with debounce.
 *
 * Provides debounced resize event handling to prevent excessive
 * re-renders during window resize operations.
 *
 * @module image/resize-handler
 */

/**
 * Resize event data.
 */
export interface ResizeEvent {
	/** Width in pixels. */
	width: number;

	/** Height in pixels. */
	height: number;
}

/**
 * Resize callback function type.
 */
export type ResizeCallback = (event: ResizeEvent) => void;

/**
 * Resize handler configuration.
 */
export interface ResizeHandlerOptions {
	/** Debounce time in milliseconds. */
	debounceMs?: number;
}

/**
 * Default debounce time in milliseconds.
 */
const DEFAULT_DEBOUNCE_MS = 100;

/**
 * Resize handler with debounce.
 *
 * Debounces rapid resize events to prevent excessive re-renders.
 * Ensures resize operations complete within target time.
 */
export class ResizeHandler {
	/** Registered callbacks. */
	private callbacks: Set<ResizeCallback> = new Set();

	/** Debounce time in milliseconds. */
	private debounceMs: number;

	/** Pending timeout ID. */
	private timeoutId: number | null = null;

	/** Pending resize event. */
	private pendingEvent: ResizeEvent | null = null;

	/** Last processed dimensions. */
	private lastDimensions: ResizeEvent | null = null;

	/** Whether the handler is disposed. */
	private disposed: boolean = false;

	/**
	 * Create a new resize handler.
	 *
	 * @param options - Handler configuration
	 */
	constructor(options: ResizeHandlerOptions = {}) {
		this.debounceMs = options.debounceMs ?? DEFAULT_DEBOUNCE_MS;
	}

	/**
	 * Register a resize callback.
	 *
	 * @param callback - Callback function
	 * @returns Unsubscribe function
	 */
	onResize(callback: ResizeCallback): () => void {
		this.callbacks.add(callback);
		return () => {
			this.callbacks.delete(callback);
		};
	}

	/**
	 * Handle a resize event (debounced).
	 *
	 * @param event - Resize event data
	 */
	handleResize(event: ResizeEvent): void {
		if (this.disposed) return;

		// Store pending event
		this.pendingEvent = event;

		// Clear existing timeout
		if (this.timeoutId !== null) {
			window.clearTimeout(this.timeoutId);
		}

		// Set new timeout
		this.timeoutId = window.setTimeout(() => {
			this.processPendingResize();
		}, this.debounceMs);
	}

	/**
	 * Process the pending resize event.
	 */
	private processPendingResize(): void {
		this.timeoutId = null;

		if (!this.pendingEvent) return;

		// Check if dimensions actually changed
		if (
			this.lastDimensions &&
			this.lastDimensions.width === this.pendingEvent.width &&
			this.lastDimensions.height === this.pendingEvent.height
		) {
			this.pendingEvent = null;
			return;
		}

		// Update last dimensions
		this.lastDimensions = { ...this.pendingEvent };

		// Notify callbacks
		const event = this.pendingEvent;
		this.pendingEvent = null;

		for (const callback of this.callbacks) {
			try {
				callback(event);
			} catch (error) {
				console.error("Resize callback error:", error);
			}
		}
	}

	/**
	 * Get the debounce time.
	 *
	 * @returns Debounce time in milliseconds
	 */
	getDebounceTime(): number {
		return this.debounceMs;
	}

	/**
	 * Set the debounce time.
	 *
	 * @param ms - Debounce time in milliseconds
	 */
	setDebounceTime(ms: number): void {
		this.debounceMs = ms;
	}

	/**
	 * Cancel any pending resize callback.
	 */
	cancel(): void {
		if (this.timeoutId !== null) {
			window.clearTimeout(this.timeoutId);
			this.timeoutId = null;
		}
		this.pendingEvent = null;
	}

	/**
	 * Immediately process any pending resize.
	 */
	flush(): void {
		if (this.timeoutId !== null) {
			window.clearTimeout(this.timeoutId);
			this.timeoutId = null;
		}

		if (this.pendingEvent) {
			this.processPendingResize();
		}
	}

	/**
	 * Get the last processed dimensions.
	 *
	 * @returns Last dimensions or null
	 */
	getLastDimensions(): ResizeEvent | null {
		return this.lastDimensions ? { ...this.lastDimensions } : null;
	}

	/**
	 * Dispose of the handler.
	 */
	dispose(): void {
		this.disposed = true;
		this.cancel();
		this.callbacks.clear();
		this.lastDimensions = null;
	}
}
