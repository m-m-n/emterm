/**
 * Bitmap cache for scaled images.
 *
 * Implements LRU caching for ImageBitmaps at common scales to avoid
 * repeated scaling operations during rendering.
 *
 * @module image/cache
 */

/**
 * Cache key type.
 */
export type CacheKey = string;

/**
 * Cache entry information.
 */
interface CacheEntry {
	/** Cached ImageBitmap. */
	bitmap: ImageBitmap;

	/** Image ID (for bulk deletion). */
	imageId: number;

	/** Width of the cached bitmap. */
	width: number;

	/** Height of the cached bitmap. */
	height: number;

	/** Memory size in bytes (estimated). */
	memoryBytes: number;

	/** Last access timestamp. */
	lastAccess: number;
}

/**
 * Cache configuration options.
 */
export interface CacheOptions {
	/** Maximum number of entries. */
	maxEntries?: number;

	/** Maximum memory usage in bytes. */
	maxMemoryBytes?: number;
}

/**
 * Cache statistics.
 */
export interface CacheStats {
	/** Number of entries. */
	entries: number;

	/** Memory usage in bytes. */
	memoryBytes: number;

	/** Cache hits. */
	hits: number;

	/** Cache misses. */
	misses: number;

	/** Hit rate (0-1). */
	hitRate: number;
}

/**
 * Default cache configuration.
 */
const DEFAULT_MAX_ENTRIES = 100;
const DEFAULT_MAX_MEMORY_BYTES = 50 * 1024 * 1024; // 50MB

/**
 * Bitmap cache with LRU eviction.
 *
 * Caches scaled ImageBitmaps to avoid repeated scaling operations.
 * Uses LRU (Least Recently Used) eviction when capacity is exceeded.
 */
export class BitmapCache {
	/** Cache entries by key. */
	private cache: Map<CacheKey, CacheEntry> = new Map();

	/** Maximum entries. */
	private maxEntries: number;

	/** Maximum memory in bytes. */
	private maxMemoryBytes: number;

	/** Current memory usage. */
	private currentMemoryBytes: number = 0;

	/** Cache hit count. */
	private hits: number = 0;

	/** Cache miss count. */
	private misses: number = 0;

	/**
	 * Create a new bitmap cache.
	 *
	 * @param options - Cache configuration
	 */
	constructor(options: CacheOptions = {}) {
		this.maxEntries = options.maxEntries ?? DEFAULT_MAX_ENTRIES;
		this.maxMemoryBytes = options.maxMemoryBytes ?? DEFAULT_MAX_MEMORY_BYTES;
	}

	/**
	 * Generate a cache key for an image at a specific scale.
	 *
	 * @param imageId - Image ID
	 * @param targetWidth - Target display width
	 * @param targetHeight - Target display height
	 * @returns Cache key
	 */
	generateKey(
		imageId: number,
		targetWidth: number,
		targetHeight: number,
	): CacheKey {
		return `${imageId}:${targetWidth}x${targetHeight}`;
	}

	/**
	 * Extract image ID from a cache key.
	 */
	private extractImageId(key: CacheKey): number {
		const colonIndex = key.indexOf(":");
		return colonIndex >= 0 ? parseInt(key.substring(0, colonIndex), 10) : 0;
	}

	/**
	 * Get a cached bitmap.
	 *
	 * @param key - Cache key
	 * @returns Cached bitmap or undefined
	 */
	get(key: CacheKey): ImageBitmap | undefined {
		const entry = this.cache.get(key);
		if (entry) {
			// Update access time
			entry.lastAccess = Date.now();
			this.hits++;
			return entry.bitmap;
		}
		this.misses++;
		return undefined;
	}

	/**
	 * Store a bitmap in the cache.
	 *
	 * @param key - Cache key
	 * @param bitmap - ImageBitmap to cache
	 * @param width - Bitmap width
	 * @param height - Bitmap height
	 */
	set(key: CacheKey, bitmap: ImageBitmap, width: number, height: number): void {
		// Calculate memory size (RGBA = 4 bytes per pixel)
		const memoryBytes = width * height * 4;

		// Evict if necessary
		this.evictIfNeeded(memoryBytes);

		// Remove existing entry if present
		const existing = this.cache.get(key);
		if (existing) {
			existing.bitmap.close();
			this.currentMemoryBytes -= existing.memoryBytes;
			this.cache.delete(key);
		}

		// Extract image ID from key
		const imageId = this.extractImageId(key);

		// Add new entry
		const entry: CacheEntry = {
			bitmap,
			imageId,
			width,
			height,
			memoryBytes,
			lastAccess: Date.now(),
		};

		this.cache.set(key, entry);
		this.currentMemoryBytes += memoryBytes;
	}

	/**
	 * Check if a key exists in the cache.
	 *
	 * @param key - Cache key
	 * @returns True if key exists
	 */
	has(key: CacheKey): boolean {
		return this.cache.has(key);
	}

	/**
	 * Delete a cached bitmap.
	 *
	 * @param key - Cache key
	 */
	delete(key: CacheKey): void {
		const entry = this.cache.get(key);
		if (entry) {
			entry.bitmap.close();
			this.currentMemoryBytes -= entry.memoryBytes;
			this.cache.delete(key);
		}
	}

	/**
	 * Delete all cached bitmaps for an image ID.
	 *
	 * @param imageId - Image ID
	 */
	deleteByImageId(imageId: number): void {
		const keysToDelete: CacheKey[] = [];

		for (const [key, entry] of this.cache) {
			if (entry.imageId === imageId) {
				keysToDelete.push(key);
			}
		}

		for (const key of keysToDelete) {
			this.delete(key);
		}
	}

	/**
	 * Evict entries if needed to make room for new entry.
	 */
	private evictIfNeeded(neededBytes: number): void {
		// Check entry count
		while (this.cache.size >= this.maxEntries) {
			this.evictLRU();
		}

		// Check memory
		while (
			this.currentMemoryBytes + neededBytes > this.maxMemoryBytes &&
			this.cache.size > 0
		) {
			this.evictLRU();
		}
	}

	/**
	 * Evict the least recently used entry.
	 */
	private evictLRU(): void {
		let oldestKey: CacheKey | null = null;
		let oldestTime = Infinity;

		for (const [key, entry] of this.cache) {
			if (entry.lastAccess < oldestTime) {
				oldestTime = entry.lastAccess;
				oldestKey = key;
			}
		}

		if (oldestKey) {
			this.delete(oldestKey);
		}
	}

	/**
	 * Get the number of cached entries.
	 *
	 * @returns Entry count
	 */
	getSize(): number {
		return this.cache.size;
	}

	/**
	 * Get the cache capacity (max entries).
	 *
	 * @returns Maximum entries
	 */
	getCapacity(): number {
		return this.maxEntries;
	}

	/**
	 * Get cache statistics.
	 *
	 * @returns Cache statistics
	 */
	getStats(): CacheStats {
		const total = this.hits + this.misses;
		return {
			entries: this.cache.size,
			memoryBytes: this.currentMemoryBytes,
			hits: this.hits,
			misses: this.misses,
			hitRate: total > 0 ? this.hits / total : 0,
		};
	}

	/**
	 * Clear all cached entries.
	 */
	clear(): void {
		for (const entry of this.cache.values()) {
			entry.bitmap.close();
		}
		this.cache.clear();
		this.currentMemoryBytes = 0;
	}

	/**
	 * Dispose of the cache.
	 */
	dispose(): void {
		this.clear();
		this.hits = 0;
		this.misses = 0;
	}
}
