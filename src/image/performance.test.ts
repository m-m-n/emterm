/**
 * Performance metrics tests.
 *
 * @module image/performance.test
 */

import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";

// Mock performance.now()
let mockTime = 0;
globalThis.performance = {
	now: mock(() => mockTime),
} as unknown as Performance;

// Helper to advance mock time
function advanceTime(ms: number): void {
	mockTime += ms;
}

// Import after mocks
import {
	MetricType,
	PerformanceMetrics,
	PerformanceMonitor,
} from "./performance.ts";

describe("PerformanceMonitor", () => {
	beforeEach(() => {
		mockTime = 0;
	});

	describe("constructor", () => {
		test("creates monitor with default settings", () => {
			const monitor = new PerformanceMonitor();
			expect(monitor.isEnabled()).toBe(true);
			monitor.dispose();
		});

		test("creates monitor with custom settings", () => {
			const monitor = new PerformanceMonitor({
				enabled: false,
				historySize: 50,
			});
			expect(monitor.isEnabled()).toBe(false);
			monitor.dispose();
		});
	});

	describe("startMeasure/endMeasure", () => {
		test("measures duration between start and end", () => {
			const monitor = new PerformanceMonitor();

			monitor.startMeasure("render");
			advanceTime(10);
			const duration = monitor.endMeasure("render");

			expect(duration).toBe(10);
			monitor.dispose();
		});

		test("returns 0 for unstarted measurement", () => {
			const monitor = new PerformanceMonitor();

			const duration = monitor.endMeasure("nonexistent");

			expect(duration).toBe(0);
			monitor.dispose();
		});

		test("records measurement in history", () => {
			const monitor = new PerformanceMonitor();

			monitor.startMeasure("render");
			advanceTime(10);
			monitor.endMeasure("render");

			const metrics = monitor.getMetrics("render");
			expect(metrics.count).toBe(1);
			expect(metrics.last).toBe(10);

			monitor.dispose();
		});
	});

	describe("recordMetric", () => {
		test("records a single metric value", () => {
			const monitor = new PerformanceMonitor();

			monitor.recordMetric("frameTime", 16.5);

			const metrics = monitor.getMetrics("frameTime");
			expect(metrics.last).toBe(16.5);
			expect(metrics.count).toBe(1);

			monitor.dispose();
		});

		test("updates statistics with multiple values", () => {
			const monitor = new PerformanceMonitor();

			monitor.recordMetric("frameTime", 10);
			monitor.recordMetric("frameTime", 20);
			monitor.recordMetric("frameTime", 30);

			const metrics = monitor.getMetrics("frameTime");
			expect(metrics.count).toBe(3);
			expect(metrics.average).toBe(20);
			expect(metrics.min).toBe(10);
			expect(metrics.max).toBe(30);

			monitor.dispose();
		});
	});

	describe("getMetrics", () => {
		test("returns zero metrics for unknown type", () => {
			const monitor = new PerformanceMonitor();

			const metrics = monitor.getMetrics("unknown");

			expect(metrics.count).toBe(0);
			expect(metrics.average).toBe(0);
			expect(metrics.min).toBe(0);
			expect(metrics.max).toBe(0);

			monitor.dispose();
		});

		test("calculates percentiles correctly", () => {
			const monitor = new PerformanceMonitor({ historySize: 100 });

			// Add 100 values: 1, 2, 3, ..., 100
			for (let i = 1; i <= 100; i++) {
				monitor.recordMetric("test", i);
			}

			const metrics = monitor.getMetrics("test");

			expect(metrics.p50).toBe(50);
			expect(metrics.p95).toBe(95);
			expect(metrics.p99).toBe(99);

			monitor.dispose();
		});
	});

	describe("getAllMetrics", () => {
		test("returns metrics for all types", () => {
			const monitor = new PerformanceMonitor();

			monitor.recordMetric("render", 10);
			monitor.recordMetric("decode", 50);
			monitor.recordMetric("upload", 5);

			const all = monitor.getAllMetrics();

			expect(all.render).toBeDefined();
			expect(all.decode).toBeDefined();
			expect(all.upload).toBeDefined();

			monitor.dispose();
		});
	});

	describe("isPerformanceGood", () => {
		test("returns true when frame time is under 16ms", () => {
			const monitor = new PerformanceMonitor();

			monitor.recordMetric("frameTime", 10);
			monitor.recordMetric("frameTime", 12);
			monitor.recordMetric("frameTime", 14);

			expect(monitor.isPerformanceGood()).toBe(true);

			monitor.dispose();
		});

		test("returns false when frame time exceeds 16ms", () => {
			const monitor = new PerformanceMonitor();

			monitor.recordMetric("frameTime", 20);
			monitor.recordMetric("frameTime", 25);
			monitor.recordMetric("frameTime", 30);

			expect(monitor.isPerformanceGood()).toBe(false);

			monitor.dispose();
		});

		test("returns true when no frame time data", () => {
			const monitor = new PerformanceMonitor();

			expect(monitor.isPerformanceGood()).toBe(true);

			monitor.dispose();
		});
	});

	describe("reset", () => {
		test("clears all metrics", () => {
			const monitor = new PerformanceMonitor();

			monitor.recordMetric("render", 10);
			monitor.recordMetric("decode", 50);

			monitor.reset();

			const metrics = monitor.getMetrics("render");
			expect(metrics.count).toBe(0);

			monitor.dispose();
		});

		test("clears specific metric type", () => {
			const monitor = new PerformanceMonitor();

			monitor.recordMetric("render", 10);
			monitor.recordMetric("decode", 50);

			monitor.reset("render");

			expect(monitor.getMetrics("render").count).toBe(0);
			expect(monitor.getMetrics("decode").count).toBe(1);

			monitor.dispose();
		});
	});

	describe("enable/disable", () => {
		test("disabling prevents metric recording", () => {
			const monitor = new PerformanceMonitor();

			monitor.disable();

			monitor.recordMetric("render", 10);
			monitor.startMeasure("test");
			advanceTime(10);
			monitor.endMeasure("test");

			expect(monitor.getMetrics("render").count).toBe(0);
			expect(monitor.getMetrics("test").count).toBe(0);

			monitor.dispose();
		});

		test("enabling resumes metric recording", () => {
			const monitor = new PerformanceMonitor({ enabled: false });

			monitor.enable();

			monitor.recordMetric("render", 10);

			expect(monitor.getMetrics("render").count).toBe(1);

			monitor.dispose();
		});
	});

	describe("onThresholdExceeded", () => {
		test("notifies when threshold is exceeded", () => {
			const monitor = new PerformanceMonitor();
			const callback = mock(() => {});

			monitor.setThreshold("frameTime", 16);
			monitor.onThresholdExceeded(callback);

			monitor.recordMetric("frameTime", 20);

			expect(callback).toHaveBeenCalledWith("frameTime", 20, 16);

			monitor.dispose();
		});

		test("does not notify when under threshold", () => {
			const monitor = new PerformanceMonitor();
			const callback = mock(() => {});

			monitor.setThreshold("frameTime", 16);
			monitor.onThresholdExceeded(callback);

			monitor.recordMetric("frameTime", 10);

			expect(callback).not.toHaveBeenCalled();

			monitor.dispose();
		});
	});

	describe("history limiting", () => {
		test("limits history to configured size", () => {
			const monitor = new PerformanceMonitor({ historySize: 10 });

			for (let i = 0; i < 20; i++) {
				monitor.recordMetric("test", i);
			}

			// Should only keep last 10 values (10-19)
			const metrics = monitor.getMetrics("test");
			expect(metrics.count).toBe(10);
			expect(metrics.min).toBe(10);
			expect(metrics.max).toBe(19);

			monitor.dispose();
		});
	});

	describe("getDebugInfo", () => {
		test("returns formatted debug string", () => {
			const monitor = new PerformanceMonitor();

			monitor.recordMetric("frameTime", 10);
			monitor.recordMetric("render", 5);

			const debug = monitor.getDebugInfo();

			expect(typeof debug).toBe("string");
			expect(debug.length).toBeGreaterThan(0);

			monitor.dispose();
		});
	});

	describe("dispose", () => {
		test("clears all data", () => {
			const monitor = new PerformanceMonitor();

			monitor.recordMetric("test", 10);
			monitor.dispose();

			expect(monitor.getMetrics("test").count).toBe(0);
		});
	});
});
