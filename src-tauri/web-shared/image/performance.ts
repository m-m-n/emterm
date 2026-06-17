/**
 * Performance monitoring for image rendering.
 *
 * Provides metrics collection and analysis for render performance,
 * decode times, and other image-related operations.
 *
 * @module image/performance
 */

/**
 * Metric type identifier.
 */
export type MetricType = string;

/**
 * Performance metrics for a specific operation type.
 */
export interface PerformanceMetrics {
	/** Number of recorded samples. */
	count: number;

	/** Last recorded value. */
	last: number;

	/** Average value. */
	average: number;

	/** Minimum value. */
	min: number;

	/** Maximum value. */
	max: number;

	/** 50th percentile (median). */
	p50: number;

	/** 95th percentile. */
	p95: number;

	/** 99th percentile. */
	p99: number;
}

/**
 * Threshold exceeded callback.
 */
export type ThresholdCallback = (
	metricType: MetricType,
	value: number,
	threshold: number,
) => void;

/**
 * Performance monitor configuration.
 */
export interface PerformanceMonitorOptions {
	/** Whether monitoring is enabled. */
	enabled?: boolean;

	/** Maximum history size per metric type. */
	historySize?: number;
}

/**
 * Default configuration.
 */
const DEFAULT_HISTORY_SIZE = 100;
const FRAME_TIME_THRESHOLD = 16; // 16ms for 60fps

/**
 * Performance monitor for image rendering.
 *
 * Collects and analyzes performance metrics for:
 * - Frame render time
 * - Image decode time
 * - Texture upload time
 * - Cache hit/miss rates
 */
export class PerformanceMonitor {
	/** Whether monitoring is enabled. */
	private enabled: boolean;

	/** Maximum history size. */
	private historySize: number;

	/** Metric history by type. */
	private history: Map<MetricType, number[]> = new Map();

	/** Active measurements (start times). */
	private activeMeasures: Map<string, number> = new Map();

	/** Configured thresholds by metric type. */
	private thresholds: Map<MetricType, number> = new Map();

	/** Threshold exceeded callbacks. */
	private thresholdCallbacks: Set<ThresholdCallback> = new Set();

	/**
	 * Create a new performance monitor.
	 *
	 * @param options - Monitor configuration
	 */
	constructor(options: PerformanceMonitorOptions = {}) {
		this.enabled = options.enabled ?? true;
		this.historySize = options.historySize ?? DEFAULT_HISTORY_SIZE;
	}

	/**
	 * Check if monitoring is enabled.
	 *
	 * @returns True if enabled
	 */
	isEnabled(): boolean {
		return this.enabled;
	}

	/**
	 * Enable monitoring.
	 */
	enable(): void {
		this.enabled = true;
	}

	/**
	 * Disable monitoring.
	 */
	disable(): void {
		this.enabled = false;
	}

	/**
	 * Start a timing measurement.
	 *
	 * @param name - Measurement name
	 */
	startMeasure(name: string): void {
		if (!this.enabled) return;
		this.activeMeasures.set(name, performance.now());
	}

	/**
	 * End a timing measurement and record the result.
	 *
	 * @param name - Measurement name
	 * @returns Duration in milliseconds, or 0 if not started
	 */
	endMeasure(name: string): number {
		if (!this.enabled) return 0;

		const startTime = this.activeMeasures.get(name);
		if (startTime === undefined) return 0;

		this.activeMeasures.delete(name);

		const duration = performance.now() - startTime;
		this.recordMetric(name, duration);

		return duration;
	}

	/**
	 * Record a metric value.
	 *
	 * @param type - Metric type
	 * @param value - Metric value
	 */
	recordMetric(type: MetricType, value: number): void {
		if (!this.enabled) return;

		// Get or create history
		let values = this.history.get(type);
		if (!values) {
			values = [];
			this.history.set(type, values);
		}

		// Add value
		values.push(value);

		// Trim to history size
		while (values.length > this.historySize) {
			values.shift();
		}

		// Check threshold
		const threshold = this.thresholds.get(type);
		if (threshold !== undefined && value > threshold) {
			this.notifyThresholdExceeded(type, value, threshold);
		}
	}

	/**
	 * Get metrics for a specific type.
	 *
	 * @param type - Metric type
	 * @returns Aggregated metrics
	 */
	getMetrics(type: MetricType): PerformanceMetrics {
		const values = this.history.get(type);

		if (!values || values.length === 0) {
			return {
				count: 0,
				last: 0,
				average: 0,
				min: 0,
				max: 0,
				p50: 0,
				p95: 0,
				p99: 0,
			};
		}

		const sorted = [...values].sort((a, b) => a - b);
		const count = sorted.length;
		const sum = sorted.reduce((a, b) => a + b, 0);

		return {
			count,
			last: values[values.length - 1] ?? 0,
			average: sum / count,
			min: sorted[0] ?? 0,
			max: sorted[count - 1] ?? 0,
			p50: this.percentile(sorted, 50),
			p95: this.percentile(sorted, 95),
			p99: this.percentile(sorted, 99),
		};
	}

	/**
	 * Calculate percentile value.
	 */
	private percentile(sortedValues: number[], percentile: number): number {
		if (sortedValues.length === 0) return 0;

		const index = Math.ceil((percentile / 100) * sortedValues.length) - 1;
		return (
			sortedValues[Math.max(0, Math.min(index, sortedValues.length - 1))] ?? 0
		);
	}

	/**
	 * Get all metrics.
	 *
	 * @returns Object with metrics for each type
	 */
	getAllMetrics(): Record<MetricType, PerformanceMetrics> {
		const result: Record<MetricType, PerformanceMetrics> = {};

		for (const type of this.history.keys()) {
			result[type] = this.getMetrics(type);
		}

		return result;
	}

	/**
	 * Check if performance is within acceptable bounds.
	 *
	 * Uses frame time threshold (16ms for 60fps).
	 *
	 * @returns True if performance is good
	 */
	isPerformanceGood(): boolean {
		const frameMetrics = this.getMetrics("frameTime");

		if (frameMetrics.count === 0) return true;

		return frameMetrics.average <= FRAME_TIME_THRESHOLD;
	}

	/**
	 * Reset metrics.
	 *
	 * @param type - Optional metric type to reset (resets all if omitted)
	 */
	reset(type?: MetricType): void {
		if (type) {
			this.history.delete(type);
		} else {
			this.history.clear();
		}
		this.activeMeasures.clear();
	}

	/**
	 * Set a threshold for a metric type.
	 *
	 * @param type - Metric type
	 * @param threshold - Threshold value
	 */
	setThreshold(type: MetricType, threshold: number): void {
		this.thresholds.set(type, threshold);
	}

	/**
	 * Register a threshold exceeded callback.
	 *
	 * @param callback - Callback function
	 * @returns Unsubscribe function
	 */
	onThresholdExceeded(callback: ThresholdCallback): () => void {
		this.thresholdCallbacks.add(callback);
		return () => {
			this.thresholdCallbacks.delete(callback);
		};
	}

	/**
	 * Notify callbacks of threshold exceeded.
	 */
	private notifyThresholdExceeded(
		type: MetricType,
		value: number,
		threshold: number,
	): void {
		for (const callback of this.thresholdCallbacks) {
			try {
				callback(type, value, threshold);
			} catch (error) {
				console.error("Threshold callback error:", error);
			}
		}
	}

	/**
	 * Get debug information as a formatted string.
	 *
	 * @returns Debug string
	 */
	getDebugInfo(): string {
		const lines: string[] = ["Performance Metrics:"];

		for (const [type, values] of this.history) {
			if (values.length === 0) continue;

			const metrics = this.getMetrics(type);
			lines.push(
				`  ${type}: avg=${metrics.average.toFixed(2)}ms ` +
					`min=${metrics.min.toFixed(2)}ms max=${metrics.max.toFixed(2)}ms ` +
					`p95=${metrics.p95.toFixed(2)}ms (n=${metrics.count})`,
			);
		}

		return lines.join("\n");
	}

	/**
	 * Dispose of the monitor.
	 */
	dispose(): void {
		this.history.clear();
		this.activeMeasures.clear();
		this.thresholds.clear();
		this.thresholdCallbacks.clear();
	}
}
