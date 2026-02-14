/**
 * Performance benchmark: WASM vs TypeScript Unicode width calculation.
 *
 * Measures execution time for both implementations on realistic data.
 */
import { describe, expect, test } from "bun:test";

// WASM implementation
import {
	charWidth as wasmCharWidth,
	stringWidth as wasmStringWidth,
	classifyCodepoints as wasmClassifyCodepoints,
} from "./unicode.ts";

// Original TypeScript implementation
import {
	charWidth as tsCharWidth,
	stringWidth as tsStringWidth,
} from "../unicode.ts";

// Generate realistic PTY data (mixed ASCII/CJK/emoji)
function generateMixedString(length: number): string {
	const chars = [
		// ASCII (most common)
		..."abcdefghijklmnopqrstuvwxyz ",
		..."ABCDEFGHIJKLMNOPQRSTUVWXYZ",
		..."0123456789",
		// CJK
		..."漢字テストあいうえおカキクケコ",
		// Emoji
		..."😀🚀📁🔋⌚☕⭐",
	];
	let result = "";
	for (let i = 0; i < length; i++) {
		result += chars[i % chars.length];
	}
	return result;
}

function benchmarkCharWidth(
	label: string,
	fn: (char: string) => number,
	chars: string[],
	iterations: number,
): number {
	// Warmup
	for (let i = 0; i < 100; i++) {
		for (const ch of chars) fn(ch);
	}

	const times: number[] = [];
	for (let iter = 0; iter < iterations; iter++) {
		const start = performance.now();
		for (const ch of chars) fn(ch);
		times.push(performance.now() - start);
	}

	times.sort((a, b) => a - b);
	return times[Math.floor(times.length / 2)]!; // median
}

function benchmarkStringWidth(
	label: string,
	fn: (str: string) => number,
	str: string,
	iterations: number,
): number {
	// Warmup
	for (let i = 0; i < 100; i++) fn(str);

	const times: number[] = [];
	for (let iter = 0; iter < iterations; iter++) {
		const start = performance.now();
		fn(str);
		times.push(performance.now() - start);
	}

	times.sort((a, b) => a - b);
	return times[Math.floor(times.length / 2)]!; // median
}

describe("Performance benchmark", () => {
	const mixedString = generateMixedString(10000);
	const chars = [...mixedString];
	const iterations = 50;

	test("charWidth: WASM vs TS on 10,000 mixed characters", () => {
		const tsTime = benchmarkCharWidth("TS charWidth", tsCharWidth, chars, iterations);
		const wasmTime = benchmarkCharWidth("WASM charWidth", wasmCharWidth, chars, iterations);
		const speedup = tsTime / wasmTime;

		console.log(`  TS charWidth (median):   ${tsTime.toFixed(3)}ms`);
		console.log(`  WASM charWidth (median): ${wasmTime.toFixed(3)}ms`);
		console.log(`  Speedup: ${speedup.toFixed(2)}x`);

		// Just verify both produce correct results
		expect(wasmCharWidth(chars[0]!)).toBe(tsCharWidth(chars[0]!));
	});

	test("stringWidth: WASM vs TS on 10,000 mixed characters", () => {
		const tsTime = benchmarkStringWidth("TS stringWidth", tsStringWidth, mixedString, iterations);
		const wasmTime = benchmarkStringWidth("WASM stringWidth", wasmStringWidth, mixedString, iterations);
		const speedup = tsTime / wasmTime;

		console.log(`  TS stringWidth (median):   ${tsTime.toFixed(3)}ms`);
		console.log(`  WASM stringWidth (median): ${wasmTime.toFixed(3)}ms`);
		console.log(`  Speedup: ${speedup.toFixed(2)}x`);

		// Verify correctness
		expect(wasmStringWidth(mixedString)).toBe(tsStringWidth(mixedString));
	});

	test("classifyCodepoints: WASM batch on 10,000 mixed characters", () => {
		// Warmup
		for (let i = 0; i < 100; i++) wasmClassifyCodepoints(mixedString);

		const times: number[] = [];
		for (let iter = 0; iter < iterations; iter++) {
			const start = performance.now();
			wasmClassifyCodepoints(mixedString);
			times.push(performance.now() - start);
		}
		times.sort((a, b) => a - b);
		const wasmBatchTime = times[Math.floor(times.length / 2)]!;

		// Compare against TS per-char loop (equivalent work)
		const tsTime = benchmarkCharWidth("TS equivalent", tsCharWidth, chars, iterations);

		const speedup = tsTime / wasmBatchTime;

		console.log(`  TS per-char loop (median):    ${tsTime.toFixed(3)}ms`);
		console.log(`  WASM batch classify (median): ${wasmBatchTime.toFixed(3)}ms`);
		console.log(`  Speedup: ${speedup.toFixed(2)}x`);

		// Verify result length
		const result = wasmClassifyCodepoints(mixedString);
		expect(result.length).toBe(chars.length);
	});
});
