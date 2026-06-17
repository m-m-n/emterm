/**
 * Placement management functions for the image layer.
 *
 * Extracted from ImageLayer to separate placement logic (place, delete, scroll, query)
 * from rendering and lifecycle concerns.
 *
 * @module image/layer-placement
 */

import type { BitmapCache } from "./cache.ts";
import type {
	ActivePlacement,
	ImageDeleteTarget,
	ImagePlacement,
	ProgressiveImage,
	StoredImage,
} from "./types.ts";
import type { WebGLLayer } from "./webgl-layer.ts";

/**
 * State required by placement functions, provided by ImageLayer.
 */
export interface PlacementContext {
	/** Stored images by ID. */
	readonly images: Map<number, StoredImage>;

	/** Active placements by key (imageId:placementId). */
	readonly placements: Map<string, ActivePlacement>;

	/** Character cell width in pixels. */
	readonly charWidth: number;

	/** Character cell height in pixels. */
	readonly charHeight: number;

	/** Horizontal padding in pixels. */
	readonly paddingX: number;

	/** Vertical padding in pixels. */
	readonly paddingY: number;

	/** WebGL layer (null if Canvas 2D backend). */
	readonly webglLayer: WebGLLayer | null;

	/** Active render backend. */
	readonly activeBackend: "webgl" | "canvas2d";

	/** Bitmap cache (null if disabled). */
	readonly cache: BitmapCache | null;

	/** Progressive loading state by image ID. */
	readonly progressiveImages: Map<number, ProgressiveImage>;
}

/**
 * Place an image at a position, calculating pixel coordinates and display size.
 *
 * Handles aspect-ratio-preserving sizing when only columns or rows are specified.
 * Registers the placement with the WebGL layer if active.
 *
 * @param ctx - Placement context from ImageLayer
 * @param placement - Placement specification
 * @returns true if placement was created, false if image not found
 */
export function placeImage(
	ctx: PlacementContext,
	placement: ImagePlacement,
): boolean {
	const stored = ctx.images.get(placement.image_id);
	if (!stored) {
		console.warn(`Image ${placement.image_id} not found for placement`);
		return false;
	}

	// Calculate pixel position
	const x =
		ctx.paddingX + placement.col * ctx.charWidth + placement.x_offset;
	const y =
		ctx.paddingY + placement.row * ctx.charHeight + placement.y_offset;

	// Calculate display size
	let displayWidth: number;
	let displayHeight: number;

	if (placement.columns > 0 && placement.rows > 0) {
		displayWidth = placement.columns * ctx.charWidth;
		displayHeight = placement.rows * ctx.charHeight;
	} else if (placement.columns > 0) {
		displayWidth = placement.columns * ctx.charWidth;
		displayHeight = (stored.data.height / stored.data.width) * displayWidth;
	} else if (placement.rows > 0) {
		displayHeight = placement.rows * ctx.charHeight;
		displayWidth = (stored.data.width / stored.data.height) * displayHeight;
	} else {
		displayWidth = stored.data.width;
		displayHeight = stored.data.height;
	}

	const key = `${placement.image_id}:${placement.placement_id}`;
	const activePlacement: ActivePlacement = {
		placement,
		x,
		y,
		displayWidth,
		displayHeight,
	};
	ctx.placements.set(key, activePlacement);

	// Add placement to WebGL layer if active
	if (ctx.activeBackend === "webgl" && ctx.webglLayer) {
		ctx.webglLayer.addPlacement({
			textureId: placement.image_id,
			x,
			y,
			width: displayWidth,
			height: displayHeight,
			zIndex: placement.z_index,
			key,
		});
	}

	return true;
}

/**
 * Delete images and/or placements matching the target.
 *
 * Handles all deletion target types: All, AllIncludingHidden, ById,
 * ByPlacement, AtCursor, ByZIndex, ByRow, ByColumn.
 * Also cleans up cache entries and progressive loading state for deleted images.
 *
 * @param ctx - Placement context from ImageLayer
 * @param target - Deletion target specification
 */
export function deleteImages(
	ctx: PlacementContext,
	target: ImageDeleteTarget,
): void {
	const deletedImageIds: number[] = [];

	switch (target.type) {
		case "All":
			ctx.placements.clear();
			if (ctx.webglLayer) ctx.webglLayer.clearPlacements();
			break;

		case "AllIncludingHidden":
			for (const imageId of ctx.images.keys()) {
				deletedImageIds.push(imageId);
			}
			ctx.placements.clear();
			ctx.images.clear();
			if (ctx.webglLayer) {
				ctx.webglLayer.clearPlacements();
				for (const id of deletedImageIds) {
					ctx.webglLayer.deleteTexture(id);
				}
			}
			break;

		case "ById":
			for (const [key, active] of ctx.placements) {
				if (active.placement.image_id === target.id) {
					ctx.placements.delete(key);
					if (ctx.webglLayer) ctx.webglLayer.removePlacement(key);
				}
			}
			ctx.images.delete(target.id);
			if (ctx.webglLayer) ctx.webglLayer.deleteTexture(target.id);
			deletedImageIds.push(target.id);
			break;

		case "ByPlacement":
			{
				const key = `${target.image_id}:${target.placement_id}`;
				ctx.placements.delete(key);
				if (ctx.webglLayer) ctx.webglLayer.removePlacement(key);
			}
			break;

		case "AtCursor":
			for (const [key, active] of ctx.placements) {
				if (
					active.placement.row === target.row &&
					active.placement.col === target.col
				) {
					ctx.placements.delete(key);
					if (ctx.webglLayer) ctx.webglLayer.removePlacement(key);
				}
			}
			break;

		case "ByZIndex":
			for (const [key, active] of ctx.placements) {
				if (active.placement.z_index === target.z_index) {
					ctx.placements.delete(key);
					if (ctx.webglLayer) ctx.webglLayer.removePlacement(key);
				}
			}
			break;

		case "ByRow":
			for (const [key, active] of ctx.placements) {
				if (active.placement.row === target.row) {
					ctx.placements.delete(key);
					if (ctx.webglLayer) ctx.webglLayer.removePlacement(key);
				}
			}
			break;

		case "ByColumn":
			for (const [key, active] of ctx.placements) {
				if (active.placement.col === target.col) {
					ctx.placements.delete(key);
					if (ctx.webglLayer) ctx.webglLayer.removePlacement(key);
				}
			}
			break;
	}

	// Clear cache for deleted images
	if (ctx.cache) {
		for (const id of deletedImageIds) {
			ctx.cache.deleteByImageId(id);
		}
	}

	// Clean up progressive loading state
	for (const id of deletedImageIds) {
		ctx.progressiveImages.delete(id);
	}
}

/**
 * Adjust placement positions after line-based scroll.
 *
 * Shifts all placement rows by delta. Placements that scroll above row 0
 * are removed. Updates WebGL layer placements if active.
 *
 * @param ctx - Placement context from ImageLayer
 * @param delta - Number of lines scrolled (positive = down, negative = up)
 */
export function scrollPlacements(
	ctx: PlacementContext,
	delta: number,
): void {
	const keysToDelete: string[] = [];

	for (const [key, active] of ctx.placements) {
		const newRow = active.placement.row + delta;
		if (newRow < 0) {
			keysToDelete.push(key);
		} else {
			active.placement.row = newRow;
			active.y =
				ctx.paddingY + newRow * ctx.charHeight + active.placement.y_offset;
		}
	}

	for (const key of keysToDelete) {
		ctx.placements.delete(key);
		if (ctx.webglLayer) ctx.webglLayer.removePlacement(key);
	}

	// Update WebGL placements
	if (ctx.webglLayer) {
		ctx.webglLayer.clearPlacements();
		for (const [key, active] of ctx.placements) {
			ctx.webglLayer.addPlacement({
				textureId: active.placement.image_id,
				x: active.x,
				y: active.y,
				width: active.displayWidth,
				height: active.displayHeight,
				zIndex: active.placement.z_index,
				key,
			});
		}
	}
}

/**
 * Get placements at a specific cell position.
 *
 * @param placements - Active placements map
 * @param row - Row index (0-based)
 * @param col - Column index (0-based)
 * @returns Array of placements at the given position
 */
export function getPlacementsAtPosition(
	placements: Map<string, ActivePlacement>,
	row: number,
	col: number,
): ActivePlacement[] {
	const result: ActivePlacement[] = [];
	for (const active of placements.values()) {
		if (active.placement.row === row && active.placement.col === col) {
			result.push(active);
		}
	}
	return result;
}

/**
 * Get all placements for a given image ID.
 *
 * @param placements - Active placements map
 * @param imageId - Image ID to search for
 * @returns Array of placements for the image
 */
export function getPlacementsForImage(
	placements: Map<string, ActivePlacement>,
	imageId: number,
): ActivePlacement[] {
	const result: ActivePlacement[] = [];
	for (const active of placements.values()) {
		if (active.placement.image_id === imageId) {
			result.push(active);
		}
	}
	return result;
}
