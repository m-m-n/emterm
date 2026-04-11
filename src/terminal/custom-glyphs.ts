/**
 * Custom glyph rendering for Unicode Block Elements and Box Drawing characters.
 *
 * Instead of using font glyphs (which can have gaps between cells), we render
 * these characters using canvas fillRect for pixel-perfect results.
 *
 * This approach is used by xterm.js, Alacritty, and other terminal emulators.
 */

/**
 * Block definition using 8x8 grid coordinates.
 * Values are in eighths of the cell size.
 */
export interface BlockDefinition {
	x: number; // X position (0-7)
	y: number; // Y position (0-7)
	w: number; // Width (1-8)
	h: number; // Height (1-8)
}

/**
 * Line definition for box drawing characters.
 */
export interface LineDefinition {
	x1: number; // Start X (0-8, where 4 is center)
	y1: number; // Start Y (0-8, where 4 is center)
	x2: number; // End X
	y2: number; // End Y
	weight: "light" | "heavy"; // Line weight
}

/**
 * Block Elements (U+2580-U+259F)
 * Defined as arrays of rectangles in 8x8 grid coordinates.
 */
export const BLOCK_ELEMENTS: Record<string, BlockDefinition[]> = {
	// Upper and lower blocks
	"\u2580": [{ x: 0, y: 0, w: 8, h: 4 }], // ▀ UPPER HALF BLOCK
	"\u2581": [{ x: 0, y: 7, w: 8, h: 1 }], // ▁ LOWER ONE EIGHTH BLOCK
	"\u2582": [{ x: 0, y: 6, w: 8, h: 2 }], // ▂ LOWER ONE QUARTER BLOCK
	"\u2583": [{ x: 0, y: 5, w: 8, h: 3 }], // ▃ LOWER THREE EIGHTHS BLOCK
	"\u2584": [{ x: 0, y: 4, w: 8, h: 4 }], // ▄ LOWER HALF BLOCK
	"\u2585": [{ x: 0, y: 3, w: 8, h: 5 }], // ▅ LOWER FIVE EIGHTHS BLOCK
	"\u2586": [{ x: 0, y: 2, w: 8, h: 6 }], // ▆ LOWER THREE QUARTERS BLOCK
	"\u2587": [{ x: 0, y: 1, w: 8, h: 7 }], // ▇ LOWER SEVEN EIGHTHS BLOCK
	"\u2588": [{ x: 0, y: 0, w: 8, h: 8 }], // █ FULL BLOCK
	"\u2589": [{ x: 0, y: 0, w: 7, h: 8 }], // ▉ LEFT SEVEN EIGHTHS BLOCK
	"\u258A": [{ x: 0, y: 0, w: 6, h: 8 }], // ▊ LEFT THREE QUARTERS BLOCK
	"\u258B": [{ x: 0, y: 0, w: 5, h: 8 }], // ▋ LEFT FIVE EIGHTHS BLOCK
	"\u258C": [{ x: 0, y: 0, w: 4, h: 8 }], // ▌ LEFT HALF BLOCK
	"\u258D": [{ x: 0, y: 0, w: 3, h: 8 }], // ▍ LEFT THREE EIGHTHS BLOCK
	"\u258E": [{ x: 0, y: 0, w: 2, h: 8 }], // ▎ LEFT ONE QUARTER BLOCK
	"\u258F": [{ x: 0, y: 0, w: 1, h: 8 }], // ▏ LEFT ONE EIGHTH BLOCK
	"\u2590": [{ x: 4, y: 0, w: 4, h: 8 }], // ▐ RIGHT HALF BLOCK

	// Shade characters (rendered as semi-transparent full blocks)
	// These are handled specially in drawBlockElement

	"\u2594": [{ x: 0, y: 0, w: 8, h: 1 }], // ▔ UPPER ONE EIGHTH BLOCK
	"\u2595": [{ x: 7, y: 0, w: 1, h: 8 }], // ▕ RIGHT ONE EIGHTH BLOCK

	// Quadrant blocks
	"\u2596": [{ x: 0, y: 4, w: 4, h: 4 }], // ▖ QUADRANT LOWER LEFT
	"\u2597": [{ x: 4, y: 4, w: 4, h: 4 }], // ▗ QUADRANT LOWER RIGHT
	"\u2598": [{ x: 0, y: 0, w: 4, h: 4 }], // ▘ QUADRANT UPPER LEFT
	"\u2599": [
		// ▙ QUADRANT UPPER LEFT AND LOWER LEFT AND LOWER RIGHT
		{ x: 0, y: 0, w: 4, h: 4 },
		{ x: 0, y: 4, w: 8, h: 4 },
	],
	"\u259A": [
		// ▚ QUADRANT UPPER LEFT AND LOWER RIGHT
		{ x: 0, y: 0, w: 4, h: 4 },
		{ x: 4, y: 4, w: 4, h: 4 },
	],
	"\u259B": [
		// ▛ QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER LEFT
		{ x: 0, y: 0, w: 8, h: 4 },
		{ x: 0, y: 4, w: 4, h: 4 },
	],
	"\u259C": [
		// ▜ QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT
		{ x: 0, y: 0, w: 8, h: 4 },
		{ x: 4, y: 4, w: 4, h: 4 },
	],
	"\u259D": [{ x: 4, y: 0, w: 4, h: 4 }], // ▝ QUADRANT UPPER RIGHT
	"\u259E": [
		// ▞ QUADRANT UPPER RIGHT AND LOWER LEFT
		{ x: 4, y: 0, w: 4, h: 4 },
		{ x: 0, y: 4, w: 4, h: 4 },
	],
	"\u259F": [
		// ▟ QUADRANT UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT
		{ x: 4, y: 0, w: 4, h: 4 },
		{ x: 0, y: 4, w: 8, h: 4 },
	],
};

/**
 * Shade character opacity values
 */
const SHADE_OPACITY: Record<string, number> = {
	"\u2591": 0.25, // ░ LIGHT SHADE
	"\u2592": 0.5, // ▒ MEDIUM SHADE
	"\u2593": 0.75, // ▓ DARK SHADE
};

/**
 * Box Drawing Characters (U+2500-U+257F)
 * Defined as arrays of line segments.
 */
export const BOX_DRAWING: Record<string, LineDefinition[]> = {
	// Light horizontal and vertical
	"\u2500": [{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" }], // ─
	"\u2501": [{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" }], // ━
	"\u2502": [{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" }], // │
	"\u2503": [{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" }], // ┃

	// Dashed lines (rendered as solid for simplicity)
	"\u2504": [{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" }], // ┄
	"\u2505": [{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" }], // ┅
	"\u2506": [{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" }], // ┆
	"\u2507": [{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" }], // ┇
	"\u2508": [{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" }], // ┈
	"\u2509": [{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" }], // ┉
	"\u250A": [{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" }], // ┊
	"\u250B": [{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" }], // ┋

	// Corner pieces - Light
	"\u250C": [
		// ┌
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u250D": [
		// ┍
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u250E": [
		// ┎
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u250F": [
		// ┏
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2510": [
		// ┐
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u2511": [
		// ┑
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u2512": [
		// ┒
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2513": [
		// ┓
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2514": [
		// └
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],
	"\u2515": [
		// ┕
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],
	"\u2516": [
		// ┖
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u2517": [
		// ┗
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u2518": [
		// ┘
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],
	"\u2519": [
		// ┙
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],
	"\u251A": [
		// ┚
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u251B": [
		// ┛
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
	],

	// T-pieces
	"\u251C": [
		// ├
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
	],
	"\u251D": [
		// ┝
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
	],
	"\u251E": [
		// ┞
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
	],
	"\u251F": [
		// ┟
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
	],
	"\u2520": [
		// ┠
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
	],
	"\u2521": [
		// ┡
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
	],
	"\u2522": [
		// ┢
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
	],
	"\u2523": [
		// ┣
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
	],
	"\u2524": [
		// ┤
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
	],
	"\u2525": [
		// ┥
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u2526": [
		// ┦
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
	],
	"\u2527": [
		// ┧
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
	],
	"\u2528": [
		// ┨
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" },
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
	],
	"\u2529": [
		// ┩
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u252A": [
		// ┪
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u252B": [
		// ┫
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" },
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u252C": [
		// ┬
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u252D": [
		// ┭
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u252E": [
		// ┮
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u252F": [
		// ┯
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u2530": [
		// ┰
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2531": [
		// ┱
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2532": [
		// ┲
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2533": [
		// ┳
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2534": [
		// ┴
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],
	"\u2535": [
		// ┵
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],
	"\u2536": [
		// ┶
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],
	"\u2537": [
		// ┷
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],
	"\u2538": [
		// ┸
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u2539": [
		// ┹
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u253A": [
		// ┺
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u253B": [
		// ┻
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
	],
	"\u253C": [
		// ┼
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
	],
	"\u253D": [
		// ┽
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
	],
	"\u253E": [
		// ┾
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
	],
	"\u253F": [
		// ┿
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
	],
	"\u2540": [
		// ╀
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u2541": [
		// ╁
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2542": [
		// ╂
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2543": [
		// ╃
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u2544": [
		// ╄
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u2545": [
		// ╅
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2546": [
		// ╆
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2547": [
		// ╇
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u2548": [
		// ╈
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u2549": [
		// ╉
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u254A": [
		// ╊
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u254B": [
		// ╋
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "heavy" },
	],

	// Double lines
	"\u2550": [
		// ═
		{ x1: 0, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 8, y2: 5, weight: "light" },
	],
	"\u2551": [
		// ║
		{ x1: 3, y1: 0, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 8, weight: "light" },
	],
	"\u2552": [
		// ╒
		{ x1: 4, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 4, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 4, y1: 3, x2: 4, y2: 8, weight: "light" },
	],
	"\u2553": [
		// ╓
		{ x1: 3, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 3, y1: 4, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 4, x2: 5, y2: 8, weight: "light" },
	],
	"\u2554": [
		// ╔
		{ x1: 3, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 5, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 3, y1: 3, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 5, x2: 5, y2: 8, weight: "light" },
	],
	"\u2555": [
		// ╕
		{ x1: 0, y1: 3, x2: 4, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 4, y2: 5, weight: "light" },
		{ x1: 4, y1: 3, x2: 4, y2: 8, weight: "light" },
	],
	"\u2556": [
		// ╖
		{ x1: 0, y1: 4, x2: 5, y2: 4, weight: "light" },
		{ x1: 3, y1: 4, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 4, x2: 5, y2: 8, weight: "light" },
	],
	"\u2557": [
		// ╗
		{ x1: 0, y1: 3, x2: 5, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 3, y2: 5, weight: "light" },
		{ x1: 3, y1: 5, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 3, x2: 5, y2: 8, weight: "light" },
	],
	"\u2558": [
		// ╘
		{ x1: 4, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 4, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 5, weight: "light" },
	],
	"\u2559": [
		// ╙
		{ x1: 3, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 3, y1: 0, x2: 3, y2: 4, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 4, weight: "light" },
	],
	"\u255A": [
		// ╚
		{ x1: 3, y1: 0, x2: 3, y2: 5, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 3, weight: "light" },
		{ x1: 3, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 5, y1: 3, x2: 8, y2: 3, weight: "light" },
	],
	"\u255B": [
		// ╛
		{ x1: 0, y1: 3, x2: 4, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 4, y2: 5, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 5, weight: "light" },
	],
	"\u255C": [
		// ╜
		{ x1: 0, y1: 4, x2: 5, y2: 4, weight: "light" },
		{ x1: 3, y1: 0, x2: 3, y2: 4, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 4, weight: "light" },
	],
	"\u255D": [
		// ╝
		{ x1: 0, y1: 3, x2: 3, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 5, y2: 5, weight: "light" },
		{ x1: 3, y1: 0, x2: 3, y2: 3, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 5, weight: "light" },
	],
	"\u255E": [
		// ╞
		{ x1: 4, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 4, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
	],
	"\u255F": [
		// ╟
		{ x1: 3, y1: 0, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 8, weight: "light" },
		{ x1: 5, y1: 4, x2: 8, y2: 4, weight: "light" },
	],
	"\u2560": [
		// ╠
		{ x1: 3, y1: 0, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 3, weight: "light" },
		{ x1: 5, y1: 5, x2: 5, y2: 8, weight: "light" },
		{ x1: 5, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 5, y1: 5, x2: 8, y2: 5, weight: "light" },
	],
	"\u2561": [
		// ╡
		{ x1: 0, y1: 3, x2: 4, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 4, y2: 5, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
	],
	"\u2562": [
		// ╢
		{ x1: 3, y1: 0, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 8, weight: "light" },
		{ x1: 0, y1: 4, x2: 3, y2: 4, weight: "light" },
	],
	"\u2563": [
		// ╣
		{ x1: 0, y1: 3, x2: 3, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 3, y2: 5, weight: "light" },
		{ x1: 3, y1: 0, x2: 3, y2: 3, weight: "light" },
		{ x1: 3, y1: 5, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 8, weight: "light" },
	],
	"\u2564": [
		// ╤
		{ x1: 0, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 4, y1: 5, x2: 4, y2: 8, weight: "light" },
	],
	"\u2565": [
		// ╥
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 3, y1: 4, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 4, x2: 5, y2: 8, weight: "light" },
	],
	"\u2566": [
		// ╦
		{ x1: 0, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 3, y2: 5, weight: "light" },
		{ x1: 5, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 3, y1: 5, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 5, x2: 5, y2: 8, weight: "light" },
	],
	"\u2567": [
		// ╧
		{ x1: 0, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 3, weight: "light" },
	],
	"\u2568": [
		// ╨
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 3, y1: 0, x2: 3, y2: 4, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 4, weight: "light" },
	],
	"\u2569": [
		// ╩
		{ x1: 0, y1: 3, x2: 3, y2: 3, weight: "light" },
		{ x1: 5, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 3, y1: 0, x2: 3, y2: 3, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 3, weight: "light" },
	],
	"\u256A": [
		// ╪
		{ x1: 0, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 8, weight: "light" },
	],
	"\u256B": [
		// ╫
		{ x1: 0, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 3, y1: 0, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 8, weight: "light" },
	],
	"\u256C": [
		// ╬
		{ x1: 0, y1: 3, x2: 3, y2: 3, weight: "light" },
		{ x1: 5, y1: 3, x2: 8, y2: 3, weight: "light" },
		{ x1: 0, y1: 5, x2: 3, y2: 5, weight: "light" },
		{ x1: 5, y1: 5, x2: 8, y2: 5, weight: "light" },
		{ x1: 3, y1: 0, x2: 3, y2: 3, weight: "light" },
		{ x1: 3, y1: 5, x2: 3, y2: 8, weight: "light" },
		{ x1: 5, y1: 0, x2: 5, y2: 3, weight: "light" },
		{ x1: 5, y1: 5, x2: 5, y2: 8, weight: "light" },
	],

	// Rounded corners (light)
	"\u256D": [
		// ╭
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u256E": [
		// ╮
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
	"\u256F": [
		// ╯
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],
	"\u2570": [
		// ╰
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
	],

	// Diagonal lines (simplified as straight lines for now)
	"\u2571": [{ x1: 8, y1: 0, x2: 0, y2: 8, weight: "light" }], // ╱
	"\u2572": [{ x1: 0, y1: 0, x2: 8, y2: 8, weight: "light" }], // ╲
	"\u2573": [
		// ╳
		{ x1: 0, y1: 0, x2: 8, y2: 8, weight: "light" },
		{ x1: 8, y1: 0, x2: 0, y2: 8, weight: "light" },
	],

	// Half lines
	"\u2574": [{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" }], // ╴
	"\u2575": [{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" }], // ╵
	"\u2576": [{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" }], // ╶
	"\u2577": [{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" }], // ╷
	"\u2578": [{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" }], // ╸
	"\u2579": [{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" }], // ╹
	"\u257A": [{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" }], // ╺
	"\u257B": [{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" }], // ╻
	"\u257C": [
		// ╼
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "heavy" },
	],
	"\u257D": [
		// ╽
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "light" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "heavy" },
	],
	"\u257E": [
		// ╾
		{ x1: 0, y1: 4, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 8, y2: 4, weight: "light" },
	],
	"\u257F": [
		// ╿
		{ x1: 4, y1: 0, x2: 4, y2: 4, weight: "heavy" },
		{ x1: 4, y1: 4, x2: 4, y2: 8, weight: "light" },
	],
};

/**
 * Powerline symbols (Private Use Area)
 * These are path-based vector shapes.
 */
export interface PowerlineDefinition {
	type: "fill" | "stroke";
	path: (w: number, h: number) => Path2D;
}

export const POWERLINE_SYMBOLS: Record<string, PowerlineDefinition> = {
	// Basic Powerline separators (U+E0B0-U+E0B3)
	"\uE0B0": {
		// Right triangle solid
		type: "fill",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(0, 0);
			p.lineTo(w, h / 2);
			p.lineTo(0, h);
			p.closePath();
			return p;
		},
	},
	"\uE0B1": {
		// Right triangle line
		type: "stroke",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(0, 0);
			p.lineTo(w, h / 2);
			p.lineTo(0, h);
			return p;
		},
	},
	"\uE0B2": {
		// Left triangle solid
		type: "fill",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(w, 0);
			p.lineTo(0, h / 2);
			p.lineTo(w, h);
			p.closePath();
			return p;
		},
	},
	"\uE0B3": {
		// Left triangle line
		type: "stroke",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(w, 0);
			p.lineTo(0, h / 2);
			p.lineTo(w, h);
			return p;
		},
	},
	// Powerline Extra - Semi-circles (U+E0B4-U+E0B7)
	"\uE0B4": {
		// Right semi-circle solid
		type: "fill",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(0, 0);
			p.quadraticCurveTo(w, 0, w, h / 2);
			p.quadraticCurveTo(w, h, 0, h);
			p.closePath();
			return p;
		},
	},
	"\uE0B5": {
		// Right semi-circle line
		type: "stroke",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(0, 0);
			p.quadraticCurveTo(w, 0, w, h / 2);
			p.quadraticCurveTo(w, h, 0, h);
			return p;
		},
	},
	"\uE0B6": {
		// Left semi-circle solid
		type: "fill",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(w, 0);
			p.quadraticCurveTo(0, 0, 0, h / 2);
			p.quadraticCurveTo(0, h, w, h);
			p.closePath();
			return p;
		},
	},
	"\uE0B7": {
		// Left semi-circle line
		type: "stroke",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(w, 0);
			p.quadraticCurveTo(0, 0, 0, h / 2);
			p.quadraticCurveTo(0, h, w, h);
			return p;
		},
	},
	// Powerline Extra - Diagonal (U+E0B8-U+E0BF)
	"\uE0B8": {
		// Lower left triangle solid
		type: "fill",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(0, 0);
			p.lineTo(w, h);
			p.lineTo(0, h);
			p.closePath();
			return p;
		},
	},
	"\uE0B9": {
		// Lower left triangle line
		type: "stroke",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(0, 0);
			p.lineTo(w, h);
			return p;
		},
	},
	"\uE0BA": {
		// Lower right triangle solid
		type: "fill",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(w, 0);
			p.lineTo(0, h);
			p.lineTo(w, h);
			p.closePath();
			return p;
		},
	},
	"\uE0BB": {
		// Lower right triangle line
		type: "stroke",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(w, 0);
			p.lineTo(0, h);
			return p;
		},
	},
	"\uE0BC": {
		// Upper left triangle solid
		type: "fill",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(0, 0);
			p.lineTo(w, 0);
			p.lineTo(0, h);
			p.closePath();
			return p;
		},
	},
	"\uE0BD": {
		// Upper left triangle line
		type: "stroke",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(w, 0);
			p.lineTo(0, h);
			return p;
		},
	},
	"\uE0BE": {
		// Upper right triangle solid
		type: "fill",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(0, 0);
			p.lineTo(w, 0);
			p.lineTo(w, h);
			p.closePath();
			return p;
		},
	},
	"\uE0BF": {
		// Upper right triangle line
		type: "stroke",
		path: (w, h) => {
			const p = new Path2D();
			p.moveTo(0, 0);
			p.lineTo(w, h);
			return p;
		},
	},
};

/**
 * Braille pattern bit positions.
 * Braille patterns (U+2800-U+28FF) are encoded as 8-bit patterns.
 *
 * Dot positions (standard Braille cell):
 *   [1] [4]     bits 0, 3
 *   [2] [5]     bits 1, 4
 *   [3] [6]     bits 2, 5
 *   [7] [8]     bits 6, 7
 */
const BRAILLE_DOT_POSITIONS = [
	{ col: 0, row: 0 }, // bit 0 - dot 1
	{ col: 0, row: 1 }, // bit 1 - dot 2
	{ col: 0, row: 2 }, // bit 2 - dot 3
	{ col: 1, row: 0 }, // bit 3 - dot 4
	{ col: 1, row: 1 }, // bit 4 - dot 5
	{ col: 1, row: 2 }, // bit 5 - dot 6
	{ col: 0, row: 3 }, // bit 6 - dot 7
	{ col: 1, row: 3 }, // bit 7 - dot 8
];

/**
 * Check if a character is a Braille pattern.
 */
export function isBraillePattern(char: string): boolean {
	const code = char.codePointAt(0);
	return code !== undefined && code >= 0x2800 && code <= 0x28ff;
}

/**
 * Draw a Braille pattern as a 2x4 grid of filled blocks.
 */
export function drawBraillePattern(
	ctx: CanvasRenderingContext2D,
	char: string,
	x: number,
	y: number,
	cellWidth: number,
	cellHeight: number
): void {
	const code = char.codePointAt(0);
	if (code === undefined) return;

	const pattern = code - 0x2800;
	const dotWidth = cellWidth / 2;
	const dotHeight = cellHeight / 4;

	for (let i = 0; i < 8; i++) {
		if (pattern & (1 << i)) {
			const pos = BRAILLE_DOT_POSITIONS[i]!;
			ctx.fillRect(
				Math.round(x + pos.col * dotWidth),
				Math.round(y + pos.row * dotHeight),
				Math.ceil(dotWidth),
				Math.ceil(dotHeight)
			);
		}
	}
}

/**
 * Sextant patterns (U+1FB00-U+1FB3B)
 * 2x3 grid semigraphics used in legacy computing (MSX, Teletext, etc.)
 *
 * Grid positions:
 *   [0] [1]     bits 0, 1
 *   [2] [3]     bits 2, 3
 *   [4] [5]     bits 4, 5
 */

/**
 * Check if a character is a Sextant.
 */
export function isSextant(char: string): boolean {
	const code = char.codePointAt(0);
	return code !== undefined && code >= 0x1fb00 && code <= 0x1fb3b;
}

/**
 * Get the sextant pattern for a code point.
 * The encoding is not linear, so we need a lookup table.
 */
function getSextantPattern(code: number): number {
	// U+1FB00-U+1FB3B maps to 2x3 grid patterns
	// The pattern encoding follows a specific order
	const offset = code - 0x1fb00;

	// Sextant encoding: patterns 1-63 (0 would be blank, not included)
	// The order is: patterns that would render the same as existing characters are skipped
	// Pattern mapping (simplified - actual Unicode encoding is complex)
	// For accuracy, we use the actual Unicode pattern order

	// Sextants skip certain patterns that duplicate other characters:
	// - Pattern 0 (blank) = space
	// - Pattern 21 (0b010101) = U+258C (LEFT HALF BLOCK)
	// - Pattern 42 (0b101010) = U+2590 (RIGHT HALF BLOCK)
	// etc.

	// Full pattern lookup (60 characters)
	const sextantPatterns = [
		0b000001, 0b000010, 0b000011, 0b000100, 0b000101, 0b000110, 0b000111,
		0b001000, 0b001001, 0b001010, 0b001011, 0b001100, 0b001101, 0b001110,
		0b001111, 0b010000, 0b010001, 0b010010, 0b010011, 0b010100,
		// 0b010101 skipped (LEFT HALF BLOCK)
		0b010110, 0b010111, 0b011000, 0b011001, 0b011010, 0b011011, 0b011100,
		0b011101, 0b011110, 0b011111, 0b100000, 0b100001, 0b100010, 0b100011,
		0b100100, 0b100101, 0b100110, 0b100111, 0b101000, 0b101001,
		// 0b101010 skipped (RIGHT HALF BLOCK)
		0b101011, 0b101100, 0b101101, 0b101110, 0b101111, 0b110000, 0b110001,
		0b110010, 0b110011, 0b110100, 0b110101, 0b110110, 0b110111, 0b111000,
		0b111001, 0b111010, 0b111011, 0b111100, 0b111101, 0b111110,
		// 0b111111 skipped (FULL BLOCK)
	];

	if (offset >= 0 && offset < sextantPatterns.length) {
		return sextantPatterns[offset] ?? 0;
	}
	return 0;
}

/**
 * Draw a Sextant as a 2x3 grid of filled blocks.
 */
export function drawSextant(
	ctx: CanvasRenderingContext2D,
	char: string,
	x: number,
	y: number,
	cellWidth: number,
	cellHeight: number
): void {
	const code = char.codePointAt(0);
	if (code === undefined) return;

	const pattern = getSextantPattern(code);
	const segWidth = cellWidth / 2;
	const segHeight = cellHeight / 3;

	for (let i = 0; i < 6; i++) {
		if (pattern & (1 << i)) {
			const col = i % 2;
			const row = Math.floor(i / 2);
			ctx.fillRect(
				Math.round(x + col * segWidth),
				Math.round(y + row * segHeight),
				Math.ceil(segWidth),
				Math.ceil(segHeight)
			);
		}
	}
}

/**
 * Check if a character should be custom rendered.
 */
export function isCustomGlyph(char: string): boolean {
	return (
		char in BLOCK_ELEMENTS ||
		char in BOX_DRAWING ||
		char in SHADE_OPACITY ||
		char in POWERLINE_SYMBOLS ||
		isBraillePattern(char) ||
		isSextant(char)
	);
}

/**
 * Draw a custom glyph.
 *
 * @param ctx - Canvas 2D rendering context
 * @param char - The character to draw
 * @param x - X position of the cell (pixels)
 * @param y - Y position of the cell (pixels)
 * @param cellWidth - Width of the cell in pixels
 * @param cellHeight - Height of the cell in pixels
 * @returns true if the character was drawn, false otherwise
 */
export function drawCustomGlyph(
	ctx: CanvasRenderingContext2D,
	char: string,
	x: number,
	y: number,
	cellWidth: number,
	cellHeight: number
): boolean {
	// Check for block elements
	const blockDef = BLOCK_ELEMENTS[char];
	if (blockDef) {
		drawBlockElement(ctx, blockDef, x, y, cellWidth, cellHeight);
		return true;
	}

	// Check for shade characters
	const shadeOpacity = SHADE_OPACITY[char];
	if (shadeOpacity !== undefined) {
		const originalAlpha = ctx.globalAlpha;
		ctx.globalAlpha *= shadeOpacity;
		ctx.fillRect(x, y, cellWidth, cellHeight);
		ctx.globalAlpha = originalAlpha;
		return true;
	}

	// Check for box drawing characters
	const boxDef = BOX_DRAWING[char];
	if (boxDef) {
		drawBoxDrawing(ctx, boxDef, x, y, cellWidth, cellHeight);
		return true;
	}

	// Check for Powerline symbols
	const powerlineDef = POWERLINE_SYMBOLS[char];
	if (powerlineDef) {
		drawPowerlineSymbol(ctx, powerlineDef, x, y, cellWidth, cellHeight);
		return true;
	}

	// Check for Braille patterns (U+2800-U+28FF)
	if (isBraillePattern(char)) {
		drawBraillePattern(ctx, char, x, y, cellWidth, cellHeight);
		return true;
	}

	// Check for Sextants (U+1FB00-U+1FB3B)
	if (isSextant(char)) {
		drawSextant(ctx, char, x, y, cellWidth, cellHeight);
		return true;
	}

	return false;
}

/**
 * Draw a block element using fillRect.
 */
function drawBlockElement(
	ctx: CanvasRenderingContext2D,
	blocks: BlockDefinition[],
	x: number,
	y: number,
	cellWidth: number,
	cellHeight: number
): void {
	const xUnit = cellWidth / 8;
	const yUnit = cellHeight / 8;

	for (const block of blocks) {
		ctx.fillRect(
			Math.round(x + block.x * xUnit),
			Math.round(y + block.y * yUnit),
			Math.ceil(block.w * xUnit),
			Math.ceil(block.h * yUnit)
		);
	}
}

/**
 * Draw box drawing lines.
 */
function drawBoxDrawing(
	ctx: CanvasRenderingContext2D,
	lines: LineDefinition[],
	x: number,
	y: number,
	cellWidth: number,
	cellHeight: number
): void {
	const xUnit = cellWidth / 8;
	const yUnit = cellHeight / 8;
	const lightWidth = Math.max(1, Math.round(cellWidth / 8));
	const heavyWidth = Math.max(2, Math.round(cellWidth / 4));

	for (const line of lines) {
		const lineWidth = line.weight === "heavy" ? heavyWidth : lightWidth;
		const x1 = Math.round(x + line.x1 * xUnit);
		const y1 = Math.round(y + line.y1 * yUnit);
		const x2 = Math.round(x + line.x2 * xUnit);
		const y2 = Math.round(y + line.y2 * yUnit);

		if (x1 === x2) {
			// Vertical line
			const lineX = x1 - Math.floor(lineWidth / 2);
			const minY = Math.min(y1, y2);
			const maxY = Math.max(y1, y2);
			ctx.fillRect(lineX, minY, lineWidth, maxY - minY);
		} else if (y1 === y2) {
			// Horizontal line
			const lineY = y1 - Math.floor(lineWidth / 2);
			const minX = Math.min(x1, x2);
			const maxX = Math.max(x1, x2);
			ctx.fillRect(minX, lineY, maxX - minX, lineWidth);
		} else {
			// Diagonal line - use stroke with current fillStyle as strokeStyle
			ctx.beginPath();
			ctx.strokeStyle = ctx.fillStyle;
			ctx.lineWidth = lineWidth;
			ctx.lineCap = "square";
			ctx.moveTo(x1, y1);
			ctx.lineTo(x2, y2);
			ctx.stroke();
		}
	}
}

/**
 * Draw a Powerline symbol using Path2D.
 */
function drawPowerlineSymbol(
	ctx: CanvasRenderingContext2D,
	def: PowerlineDefinition,
	x: number,
	y: number,
	cellWidth: number,
	cellHeight: number
): void {
	ctx.save();
	ctx.translate(x, y);
	const path = def.path(cellWidth, cellHeight);

	if (def.type === "fill") {
		ctx.fill(path);
	} else {
		ctx.lineWidth = Math.max(1, Math.round(cellWidth / 8));
		ctx.lineCap = "round";
		ctx.lineJoin = "round";
		ctx.stroke(path);
	}

	ctx.restore();
}
