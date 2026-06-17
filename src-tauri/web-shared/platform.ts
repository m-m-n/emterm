/**
 * Platform detection helper.
 *
 * Resolves the current operating system identifier once at startup via the
 * existing `get_platform` Tauri command and caches it in module state for
 * subsequent synchronous access. The cache avoids repeated IPC round-trips
 * from hot selection/paste paths.
 *
 * Usage:
 *   1. Call `initPlatform()` once during application bootstrap, before
 *      constructing any component that needs to know the platform.
 *   2. Read `isLinux()` / `isWindows()` synchronously anywhere afterwards.
 *
 * If `initPlatform()` has not yet completed (e.g. an unexpected early call),
 * both predicates return `false` — i.e. they fail closed to the non-Linux
 * code path rather than risking a platform-specific action on the wrong OS.
 */
import { invoke } from "@tauri-apps/api/core";

let cached: string | null = null;
let initPromise: Promise<void> | null = null;

/**
 * Resolve the platform identifier via the backend and cache it.
 *
 * Safe to call multiple times: subsequent calls return the same in-flight
 * Promise (or a resolved Promise if initialization is already complete).
 *
 * On failure, the cache is set to the empty string and a warning is logged.
 * The predicates will return `false` in that state, so the app degrades to
 * non-Linux behavior rather than crashing.
 */
export async function initPlatform(): Promise<void> {
  if (cached !== null) return;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      cached = await invoke<string>("get_platform");
    } catch (error) {
      console.warn("[WARN][FRONTEND] Failed to resolve platform:", error);
      cached = "";
    } finally {
      initPromise = null;
    }
  })();

  return initPromise;
}

/**
 * Return `true` if the current platform is Linux.
 *
 * Returns `false` if `initPlatform()` has not yet resolved.
 */
export function isLinux(): boolean {
  return cached === "linux";
}

/**
 * Return `true` if the current platform is Windows.
 *
 * Returns `false` if `initPlatform()` has not yet resolved.
 */
export function isWindows(): boolean {
  return cached === "windows";
}

/**
 * Internal: reset the cache. Exposed for unit tests only.
 *
 * @internal
 */
export function _resetPlatformCacheForTests(): void {
  cached = null;
  initPromise = null;
}

/**
 * Internal: force the cache to a specific value. Exposed for unit tests only.
 *
 * @internal
 */
export function _setPlatformCacheForTests(value: string | null): void {
  cached = value;
  initPromise = null;
}
