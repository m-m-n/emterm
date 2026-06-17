/**
 * Diagnostics history — short-lived ring buffers populated continuously
 * so that, at the moment a WASM crash (or other rare failure) is detected,
 * the surrounding context is already captured.
 *
 * Without this, post-mortem investigation has to grep across thousands
 * of `[DIAG-PTY-HEALTH]` lines preceding the crash; an in-process ring
 * lets the crash log emit a single line summarising the recent past.
 *
 * Two stores:
 *
 * 1. **Heap/recv samples** — fed by the 5 s heartbeat in `diagnostics.ts`.
 *    Tracks wasmHeapMB and PTY chunk recv counters so heap growth or
 *    sudden PTY silence preceding a crash is visible at a glance.
 *
 * 2. **Event timeline** — discrete events (visibility change, resize,
 *    mux enter/exit/switch, recovery attempts). Lets us tell whether
 *    the crash was preceded by, e.g., a resize storm or a visibility
 *    flap, without correlating across separate log lines.
 *
 * Both stores are intentionally tiny (12 / 32 entries) — they are not a
 * substitute for the full log, just enough context to inform the next
 * diagnostic step.
 */

/** One heap/recv sample (taken every heartbeat). */
export interface HeapSample {
  /** performance.now() at sample time. */
  readonly t: number;
  /** WASM linear memory size in MB. -1 if loader unavailable. */
  readonly heapMB: number;
  /** Cumulative PTY chunk count (or -1 if no client). */
  readonly recvCount: number;
  /** Cumulative PTY bytes received (or -1). */
  readonly recvBytes: number;
}

/** One discrete event. */
export interface TimelineEvent {
  /** performance.now() at record time. */
  readonly t: number;
  /** Short kind tag, e.g. "visibility", "resize", "mux-switch". */
  readonly kind: string;
  /** Short detail string (kept compact for log readability). */
  readonly detail: string;
}

/** Maximum samples retained (~ 1 min at 5 s heartbeat). */
const MAX_HEAP_SAMPLES = 12;
/** Maximum events retained. */
const MAX_TIMELINE_EVENTS = 32;

const heapSamples: HeapSample[] = [];
const timelineEvents: TimelineEvent[] = [];

/**
 * Append a heap/recv sample. Called from the diagnostics heartbeat.
 */
export function recordHeapSample(sample: HeapSample): void {
  heapSamples.push(sample);
  if (heapSamples.length > MAX_HEAP_SAMPLES) heapSamples.shift();
}

/**
 * Append a timeline event. Caller passes a short kind + detail.
 *
 * `detail` is logged verbatim so it should already be compact; large
 * payloads should be summarised on the call site.
 */
export function recordEvent(kind: string, detail: string): void {
  timelineEvents.push({ t: performance.now(), kind, detail });
  if (timelineEvents.length > MAX_TIMELINE_EVENTS) timelineEvents.shift();
}

/**
 * Render heap/recv samples as a single compact line for crash logs.
 *
 * Format: `heap=[t-Δms:heapMB/recvCount/recvBytesK, ...]`
 * Time deltas are negative offsets from `now` so the freshest sample
 * is on the right (heuristic: human eye scans left-to-right toward
 * "what just happened").
 */
export function formatHeapHistory(now: number = performance.now()): string {
  if (heapSamples.length === 0) return "heap=[]";
  const parts = heapSamples.map((s) => {
    const ago = Math.round(now - s.t);
    const kb = s.recvBytes >= 0 ? Math.round(s.recvBytes / 1024) : -1;
    return `-${ago}ms:${s.heapMB}MB/${s.recvCount}c/${kb}KB`;
  });
  return `heap=[${parts.join(",")}]`;
}

/**
 * Render timeline events as a single compact line for crash logs.
 *
 * Format: `events=[t-Δms:kind:detail, ...]`
 */
export function formatEventTimeline(now: number = performance.now()): string {
  if (timelineEvents.length === 0) return "events=[]";
  const parts = timelineEvents.map((e) => {
    const ago = Math.round(now - e.t);
    return `-${ago}ms:${e.kind}:${e.detail}`;
  });
  return `events=[${parts.join(",")}]`;
}

/**
 * Combined one-line snapshot: heap history + event timeline.
 *
 * Intended for embedding in a single crash log line.
 */
export function snapshotForCrash(now: number = performance.now()): string {
  return `${formatHeapHistory(now)} ${formatEventTimeline(now)}`;
}

/**
 * Clear both stores. Test-only — production keeps them for the lifetime
 * of the process.
 */
export function resetDiagnosticsHistoryForTests(): void {
  heapSamples.length = 0;
  timelineEvents.length = 0;
}
