/**
 * Performance measurement utilities for terminal rendering.
 *
 * Provides instrumentation for monitoring render times,
 * throughput, and memory usage.
 */

/**
 * Performance metrics snapshot.
 */
export interface PerformanceMetrics {
  /** Average render time in milliseconds. */
  avgRenderTime: number;
  /** Maximum render time in milliseconds. */
  maxRenderTime: number;
  /** Minimum render time in milliseconds. */
  minRenderTime: number;
  /** Number of render cycles measured. */
  renderCount: number;
  /** Average actions processed per second. */
  actionsPerSecond: number;
  /** Total actions processed. */
  totalActions: number;
  /** Estimated throughput in bytes per second. */
  throughputBps: number;
  /** Number of frames that exceeded 16ms budget. */
  droppedFrames: number;
  /** Percentage of frames within budget. */
  frameSuccessRate: number;
}

/**
 * Timing entry for a single measurement.
 */
interface TimingEntry {
  timestamp: number;
  duration: number;
}

/**
 * Performance monitor for terminal operations.
 */
export class PerformanceMonitor {
  /** Render time history (rolling window). */
  private renderTimes: TimingEntry[] = [];

  /** Action processing times. */
  private actionTimes: TimingEntry[] = [];

  /** Maximum history entries to keep. */
  private readonly maxHistorySize: number = 100;

  /** Frame budget in milliseconds (60fps = 16.67ms). */
  private readonly frameBudget: number = 16;

  /** Total actions processed. */
  private totalActions: number = 0;

  /** Total bytes processed (estimated). */
  private totalBytes: number = 0;

  /** Start time for throughput calculation. */
  private startTime: number = performance.now();

  /** Number of frames that exceeded budget. */
  private droppedFrames: number = 0;

  /** Total frames rendered. */
  private totalFrames: number = 0;

  /** Whether monitoring is enabled. */
  private enabled: boolean = false;

  /**
   * Enable performance monitoring.
   */
  enable(): void {
    this.enabled = true;
    this.reset();
  }

  /**
   * Disable performance monitoring.
   */
  disable(): void {
    this.enabled = false;
  }

  /**
   * Check if monitoring is enabled.
   */
  isEnabled(): boolean {
    return this.enabled;
  }

  /**
   * Record a render operation.
   *
   * @param durationMs - Render duration in milliseconds
   */
  recordRender(durationMs: number): void {
    if (!this.enabled) return;

    const entry: TimingEntry = {
      timestamp: performance.now(),
      duration: durationMs,
    };

    this.renderTimes.push(entry);
    if (this.renderTimes.length > this.maxHistorySize) {
      this.renderTimes.shift();
    }

    this.totalFrames++;
    if (durationMs > this.frameBudget) {
      this.droppedFrames++;
    }
  }

  /**
   * Record action processing.
   *
   * @param actionCount - Number of actions processed
   * @param durationMs - Processing duration in milliseconds
   * @param estimatedBytes - Estimated bytes processed
   */
  recordActions(actionCount: number, durationMs: number, estimatedBytes: number = 0): void {
    if (!this.enabled) return;

    const entry: TimingEntry = {
      timestamp: performance.now(),
      duration: durationMs,
    };

    this.actionTimes.push(entry);
    if (this.actionTimes.length > this.maxHistorySize) {
      this.actionTimes.shift();
    }

    this.totalActions += actionCount;
    this.totalBytes += estimatedBytes;
  }

  /**
   * Measure a function's execution time.
   *
   * @param fn - Function to measure
   * @returns Result of the function
   */
  measure<T>(fn: () => T): { result: T; durationMs: number } {
    const start = performance.now();
    const result = fn();
    const durationMs = performance.now() - start;
    return { result, durationMs };
  }

  /**
   * Measure an async function's execution time.
   *
   * @param fn - Async function to measure
   * @returns Result of the function
   */
  async measureAsync<T>(fn: () => Promise<T>): Promise<{ result: T; durationMs: number }> {
    const start = performance.now();
    const result = await fn();
    const durationMs = performance.now() - start;
    return { result, durationMs };
  }

  /**
   * Get current performance metrics.
   */
  getMetrics(): PerformanceMetrics {
    const now = performance.now();
    const elapsedSeconds = (now - this.startTime) / 1000;

    // Calculate render time statistics
    let avgRenderTime = 0;
    let maxRenderTime = 0;
    let minRenderTime = Infinity;

    if (this.renderTimes.length > 0) {
      let sum = 0;
      for (const entry of this.renderTimes) {
        sum += entry.duration;
        if (entry.duration > maxRenderTime) maxRenderTime = entry.duration;
        if (entry.duration < minRenderTime) minRenderTime = entry.duration;
      }
      avgRenderTime = sum / this.renderTimes.length;
    }

    if (minRenderTime === Infinity) minRenderTime = 0;

    // Calculate throughput
    const actionsPerSecond = elapsedSeconds > 0 ? this.totalActions / elapsedSeconds : 0;
    const throughputBps = elapsedSeconds > 0 ? this.totalBytes / elapsedSeconds : 0;

    // Frame success rate
    const frameSuccessRate =
      this.totalFrames > 0
        ? ((this.totalFrames - this.droppedFrames) / this.totalFrames) * 100
        : 100;

    return {
      avgRenderTime,
      maxRenderTime,
      minRenderTime,
      renderCount: this.renderTimes.length,
      actionsPerSecond,
      totalActions: this.totalActions,
      throughputBps,
      droppedFrames: this.droppedFrames,
      frameSuccessRate,
    };
  }

  /**
   * Reset all metrics.
   */
  reset(): void {
    this.renderTimes = [];
    this.actionTimes = [];
    this.totalActions = 0;
    this.totalBytes = 0;
    this.startTime = performance.now();
    this.droppedFrames = 0;
    this.totalFrames = 0;
  }

  /**
   * Log current metrics to console.
   */
  logMetrics(): void {
    const metrics = this.getMetrics();
    console.log("=== Terminal Performance Metrics ===");
    console.log(`Render Time: avg=${metrics.avgRenderTime.toFixed(2)}ms, max=${metrics.maxRenderTime.toFixed(2)}ms, min=${metrics.minRenderTime.toFixed(2)}ms`);
    console.log(`Frames: ${metrics.renderCount}, Dropped: ${metrics.droppedFrames} (${(100 - metrics.frameSuccessRate).toFixed(1)}%)`);
    console.log(`Actions: ${metrics.totalActions} total, ${metrics.actionsPerSecond.toFixed(0)}/sec`);
    console.log(`Throughput: ${(metrics.throughputBps / 1024 / 1024).toFixed(2)} MB/s`);
  }
}

/**
 * Global performance monitor instance.
 */
let globalMonitor: PerformanceMonitor | null = null;

/**
 * Get the global performance monitor.
 */
export function getPerformanceMonitor(): PerformanceMonitor {
  if (globalMonitor === null) {
    globalMonitor = new PerformanceMonitor();
  }
  return globalMonitor;
}

/**
 * Render timing helper for use in renderer.
 */
export class RenderTimer {
  private startTime: number = 0;

  /**
   * Start timing a render operation.
   */
  start(): void {
    this.startTime = performance.now();
  }

  /**
   * End timing and return the duration.
   *
   * @returns Duration in milliseconds
   */
  end(): number {
    return performance.now() - this.startTime;
  }

  /**
   * End timing and record to the performance monitor.
   *
   * @param monitor - Performance monitor to record to
   * @returns Duration in milliseconds
   */
  endAndRecord(monitor: PerformanceMonitor): number {
    const duration = this.end();
    monitor.recordRender(duration);
    return duration;
  }
}

/**
 * Simple frame budget checker.
 * Warns if render time exceeds 16ms.
 */
export function checkFrameBudget(durationMs: number, context: string = "render"): void {
  if (durationMs > 16) {
    console.warn(`[Performance] ${context} exceeded frame budget: ${durationMs.toFixed(2)}ms (budget: 16ms)`);
  }
}

/**
 * Throughput measurement utility.
 */
export class ThroughputMeter {
  private bytesProcessed: number = 0;
  private startTime: number = 0;
  private isRunning: boolean = false;

  /**
   * Start measuring throughput.
   */
  start(): void {
    this.bytesProcessed = 0;
    this.startTime = performance.now();
    this.isRunning = true;
  }

  /**
   * Add processed bytes.
   *
   * @param bytes - Number of bytes processed
   */
  add(bytes: number): void {
    if (this.isRunning) {
      this.bytesProcessed += bytes;
    }
  }

  /**
   * Stop measuring and return results.
   *
   * @returns Throughput in bytes per second
   */
  stop(): { bytesProcessed: number; durationMs: number; bytesPerSecond: number } {
    this.isRunning = false;
    const durationMs = performance.now() - this.startTime;
    const durationSec = durationMs / 1000;
    const bytesPerSecond = durationSec > 0 ? this.bytesProcessed / durationSec : 0;

    return {
      bytesProcessed: this.bytesProcessed,
      durationMs,
      bytesPerSecond,
    };
  }

  /**
   * Get current throughput without stopping.
   */
  getCurrentThroughput(): number {
    if (!this.isRunning) return 0;
    const durationSec = (performance.now() - this.startTime) / 1000;
    return durationSec > 0 ? this.bytesProcessed / durationSec : 0;
  }
}
