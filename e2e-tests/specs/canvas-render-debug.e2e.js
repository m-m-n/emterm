/**
 * Canvas Render Debug E2E Test
 *
 * Investigates rendering issues:
 * 1. Black lines (1px gaps) between rows
 * 2. Black margins at right and bottom edges
 *
 * Captures screenshots and renderer internal state for analysis.
 */

describe("Canvas Render Debug", () => {
	it("should capture initial render state and check for gaps", async () => {
		// Wait for terminal to fully initialize
		await browser.pause(3000);

		// Get terminal container and canvas info
		const renderInfo = await browser.execute(() => {
			const tabContent = document.querySelector(".tab-content");
			const terminalRoot = document.querySelector(".terminal-root");
			const canvas = document.querySelector("canvas");

			if (!canvas) return { error: "No canvas element found" };

			const containerRect = tabContent
				? tabContent.getBoundingClientRect()
				: null;
			const terminalRootRect = terminalRoot
				? terminalRoot.getBoundingClientRect()
				: null;
			const canvasRect = canvas.getBoundingClientRect();
			const computedStyle = tabContent
				? getComputedStyle(tabContent)
				: null;

			return {
				container: containerRect
					? {
							width: containerRect.width,
							height: containerRect.height,
						}
					: null,
				terminalRoot: terminalRootRect
					? {
							width: terminalRootRect.width,
							height: terminalRootRect.height,
						}
					: null,
				canvas: {
					cssWidth: canvasRect.width,
					cssHeight: canvasRect.height,
					internalWidth: canvas.width,
					internalHeight: canvas.height,
					styleWidth: canvas.style.width,
					styleHeight: canvas.style.height,
				},
				padding: computedStyle
					? {
							left: computedStyle.paddingLeft,
							right: computedStyle.paddingRight,
							top: computedStyle.paddingTop,
							bottom: computedStyle.paddingBottom,
						}
					: null,
				cssVarPadding:
					getComputedStyle(document.documentElement).getPropertyValue(
						"--terminal-padding",
					),
				cssVarBackground:
					getComputedStyle(document.documentElement).getPropertyValue(
						"--terminal-background",
					),
				containerBg: computedStyle
					? computedStyle.backgroundColor
					: null,
			};
		});

		console.log(
			"=== Render Container Info ===",
			JSON.stringify(renderInfo, null, 2),
		);

		// Get renderer internal metrics via window.terminalRenderer
		const rendererMetrics = await browser.execute(() => {
			const renderer = window.terminalRenderer;
			if (!renderer) return { error: "No terminalRenderer on window" };

			// Read private fields via bracket notation
			const charWidth = renderer.getCharWidth ? renderer.getCharWidth() : renderer["charWidth"];
			const charHeight = renderer.getCharHeight ? renderer.getCharHeight() : renderer["charHeight"];
			const cols = renderer["cols"];
			const rows = renderer["rows"];

			return {
				charWidth,
				charHeight,
				fontSize: renderer["fontSize"],
				fontFamily: renderer["fontFamily"],
				lineHeightMultiplier: renderer["lineHeightMultiplier"],
				fontDescent: renderer["fontDescent"],
				cols,
				rows,
				dpr: renderer["dpr"],
				opacity: renderer["opacity"],
				gridWidth: charWidth * cols,
				gridHeight: charHeight * rows,
			};
		});

		console.log(
			"=== Renderer Metrics ===",
			JSON.stringify(rendererMetrics, null, 2),
		);

		// Calculate expected gaps
		if (rendererMetrics && !rendererMetrics.error && renderInfo && !renderInfo.error) {
			const canvasWidth = renderInfo.canvas.cssWidth;
			const canvasHeight = renderInfo.canvas.cssHeight;
			const gridWidth = rendererMetrics.gridWidth;
			const gridHeight = rendererMetrics.gridHeight;

			console.log("=== Gap Analysis ===");
			console.log(`Canvas CSS size: ${canvasWidth} x ${canvasHeight}`);
			console.log(`Grid area: ${gridWidth} x ${gridHeight}`);
			console.log(`Right margin (gap): ${canvasWidth - gridWidth}px`);
			console.log(`Bottom margin (gap): ${canvasHeight - gridHeight}px`);
			console.log(
				`charHeight is integer: ${Number.isInteger(rendererMetrics.charHeight)}`,
			);
			console.log(
				`charHeight fractional part: ${rendererMetrics.charHeight % 1}`,
			);
			console.log(
				`charWidth fractional part: ${rendererMetrics.charWidth % 1}`,
			);
		}

		// Take initial screenshot
		await browser.saveScreenshot(
			"./screenshots/canvas-render-debug-01-initial.png",
		);
	});

	it("should analyze pixel data around row boundaries", async () => {
		await browser.pause(1000);

		// Sample pixel colors at row boundaries to detect black lines
		const pixelAnalysis = await browser.execute(() => {
			const canvas = document.querySelector("canvas");
			if (!canvas) return { error: "No canvas" };

			const ctx = canvas.getContext("2d");
			if (!ctx) return { error: "No context" };

			const renderer = window.terminalRenderer;
			if (!renderer) return { error: "No terminalRenderer" };

			const charHeight = renderer.getCharHeight ? renderer.getCharHeight() : renderer["charHeight"];
			const charWidth = renderer.getCharWidth ? renderer.getCharWidth() : renderer["charWidth"];
			const dpr = renderer["dpr"] || window.devicePixelRatio || 1;
			const cols = renderer["cols"];
			const rows = renderer["rows"];

			// Sample pixels at row boundaries (checking for gaps between rows)
			const rowBoundaryPixels = [];
			for (let row = 0; row < Math.min(8, rows); row++) {
				const y = row * charHeight;
				const nextY = (row + 1) * charHeight;

				// Sample at the end of this row and start of next row
				// Use DPR-scaled coordinates for getImageData
				const endOfRowY = Math.floor(nextY * dpr) - 1;
				const startOfNextRowY = Math.ceil(nextY * dpr);

				const colMid = Math.floor((cols / 2) * charWidth * dpr);

				// Get pixel at end of current row
				let endPixel = null;
				try {
					const d1 = ctx.getImageData(colMid, endOfRowY, 1, 1).data;
					endPixel = { r: d1[0], g: d1[1], b: d1[2], a: d1[3] };
				} catch (e) {
					endPixel = { error: e.message };
				}

				// Get pixel at start of next row
				let startPixel = null;
				try {
					const d2 = ctx.getImageData(
						colMid,
						startOfNextRowY,
						1,
						1,
					).data;
					startPixel = { r: d2[0], g: d2[1], b: d2[2], a: d2[3] };
				} catch (e) {
					startPixel = { error: e.message };
				}

				// Also sample the exact boundary pixel
				const exactBoundaryY = Math.round(nextY * dpr);
				let boundaryPixel = null;
				try {
					const d3 = ctx.getImageData(colMid, exactBoundaryY, 1, 1).data;
					boundaryPixel = { r: d3[0], g: d3[1], b: d3[2], a: d3[3] };
				} catch (e) {
					boundaryPixel = { error: e.message };
				}

				rowBoundaryPixels.push({
					row,
					cssRowEndY: nextY,
					cssRowEndY_isInteger: Number.isInteger(nextY),
					scaledEndY: endOfRowY,
					scaledStartNextY: startOfNextRowY,
					scaledExactBoundary: exactBoundaryY,
					endOfRowPixel: endPixel,
					startOfNextRowPixel: startPixel,
					exactBoundaryPixel: boundaryPixel,
				});
			}

			// Sample pixels at right edge and bottom edge
			const rightEdgePixels = [];
			const gridWidthPx = cols * charWidth;
			for (let row = 0; row < Math.min(3, rows); row++) {
				const y = Math.floor((row * charHeight + charHeight / 2) * dpr);
				const lastGridX = Math.floor(gridWidthPx * dpr) - 1;
				const afterGridX = Math.ceil(gridWidthPx * dpr);
				const canvasEdgeX = canvas.width - 1;

				let lastGridPixel = null;
				try {
					const d = ctx.getImageData(lastGridX, y, 1, 1).data;
					lastGridPixel = { r: d[0], g: d[1], b: d[2], a: d[3] };
				} catch (e) {
					lastGridPixel = { error: e.message };
				}

				let afterGridPixel = null;
				if (afterGridX < canvas.width) {
					try {
						const d = ctx.getImageData(afterGridX, y, 1, 1).data;
						afterGridPixel = { r: d[0], g: d[1], b: d[2], a: d[3] };
					} catch (e) {
						afterGridPixel = { error: e.message };
					}
				} else {
					afterGridPixel = { note: "beyond canvas" };
				}

				let canvasEdgePixel = null;
				try {
					const d = ctx.getImageData(canvasEdgeX, y, 1, 1).data;
					canvasEdgePixel = { r: d[0], g: d[1], b: d[2], a: d[3] };
				} catch (e) {
					canvasEdgePixel = { error: e.message };
				}

				rightEdgePixels.push({
					row,
					gridEndX: gridWidthPx,
					canvasWidthCSS: canvas.width / dpr,
					gapPx: (canvas.width / dpr) - gridWidthPx,
					lastGridPixel,
					afterGridPixel,
					canvasEdgePixel,
				});
			}

			// Sample bottom edge pixels
			const bottomEdgePixels = [];
			const gridHeightPx = rows * charHeight;
			for (let col = 0; col < Math.min(3, cols); col++) {
				const x = Math.floor((col * charWidth + charWidth / 2) * dpr);
				const lastGridY = Math.floor(gridHeightPx * dpr) - 1;
				const afterGridY = Math.ceil(gridHeightPx * dpr);
				const canvasEdgeY = canvas.height - 1;

				let lastGridPixel = null;
				try {
					const d = ctx.getImageData(x, lastGridY, 1, 1).data;
					lastGridPixel = { r: d[0], g: d[1], b: d[2], a: d[3] };
				} catch (e) {
					lastGridPixel = { error: e.message };
				}

				let afterGridPixel = null;
				if (afterGridY < canvas.height) {
					try {
						const d = ctx.getImageData(x, afterGridY, 1, 1).data;
						afterGridPixel = {
							r: d[0],
							g: d[1],
							b: d[2],
							a: d[3],
						};
					} catch (e) {
						afterGridPixel = { error: e.message };
					}
				} else {
					afterGridPixel = { note: "beyond canvas" };
				}

				let canvasEdgePixel = null;
				try {
					const d = ctx.getImageData(x, canvasEdgeY, 1, 1).data;
					canvasEdgePixel = { r: d[0], g: d[1], b: d[2], a: d[3] };
				} catch (e) {
					canvasEdgePixel = { error: e.message };
				}

				bottomEdgePixels.push({
					col,
					gridEndY: gridHeightPx,
					canvasHeightCSS: canvas.height / dpr,
					gapPx: (canvas.height / dpr) - gridHeightPx,
					lastGridPixel,
					afterGridPixel,
					canvasEdgePixel,
				});
			}

			return {
				dpr,
				charHeight,
				charWidth,
				charHeightIsInteger: Number.isInteger(charHeight),
				canvasSize: { width: canvas.width, height: canvas.height },
				gridSize: { width: gridWidthPx, height: gridHeightPx },
				rowBoundaryPixels,
				rightEdgePixels,
				bottomEdgePixels,
			};
		});

		console.log(
			"=== Pixel Analysis ===",
			JSON.stringify(pixelAnalysis, null, 2),
		);

		// Summarize findings
		if (pixelAnalysis && !pixelAnalysis.error) {
			console.log("\n=== Summary ===");
			console.log(`DPR: ${pixelAnalysis.dpr}`);
			console.log(
				`charHeight: ${pixelAnalysis.charHeight} (integer: ${pixelAnalysis.charHeightIsInteger})`,
			);
			console.log(`charWidth: ${pixelAnalysis.charWidth}`);
			console.log(`Grid: ${pixelAnalysis.gridSize.width} x ${pixelAnalysis.gridSize.height}`);
			console.log(`Canvas: ${pixelAnalysis.canvasSize.width} x ${pixelAnalysis.canvasSize.height}`);

			// Check for black pixels at row boundaries
			let hasBlackRowGaps = false;
			for (const rb of pixelAnalysis.rowBoundaryPixels) {
				const endP = rb.endOfRowPixel;
				const startP = rb.startOfNextRowPixel;
				const boundP = rb.exactBoundaryPixel;
				if (endP && !endP.error && endP.r === 0 && endP.g === 0 && endP.b === 0) {
					console.log(
						`Row ${rb.row}: END pixel is BLACK (0,0,0) at scaled Y=${rb.scaledEndY}`,
					);
					hasBlackRowGaps = true;
				}
				if (startP && !startP.error && startP.r === 0 && startP.g === 0 && startP.b === 0) {
					console.log(
						`Row ${rb.row}: START-NEXT pixel is BLACK (0,0,0) at scaled Y=${rb.scaledStartNextY}`,
					);
					hasBlackRowGaps = true;
				}
				if (boundP && !boundP.error && boundP.r === 0 && boundP.g === 0 && boundP.b === 0) {
					console.log(
						`Row ${rb.row}: EXACT BOUNDARY pixel is BLACK (0,0,0) at scaled Y=${rb.scaledExactBoundary}`,
					);
					hasBlackRowGaps = true;
				}
			}
			if (!hasBlackRowGaps) {
				console.log("No black gaps detected between rows (row boundary pixels are non-black)");
			}

			// Check right edge
			let hasBlackRightEdge = false;
			for (const re of pixelAnalysis.rightEdgePixels) {
				const afterP = re.afterGridPixel;
				const edgeP = re.canvasEdgePixel;
				if (afterP && !afterP.error && !afterP.note && afterP.r === 0 && afterP.g === 0 && afterP.b === 0) {
					console.log(
						`Row ${re.row}: RIGHT MARGIN pixel is BLACK at grid edge X=${re.gridEndX}, gap=${re.gapPx}px`,
					);
					hasBlackRightEdge = true;
				}
				if (edgeP && !edgeP.error && edgeP.r === 0 && edgeP.g === 0 && edgeP.b === 0) {
					console.log(
						`Row ${re.row}: CANVAS EDGE pixel is BLACK`,
					);
					hasBlackRightEdge = true;
				}
			}
			if (!hasBlackRightEdge) {
				console.log("No black right margin detected");
			}

			// Check bottom edge
			let hasBlackBottomEdge = false;
			for (const be of pixelAnalysis.bottomEdgePixels) {
				const afterP = be.afterGridPixel;
				const edgeP = be.canvasEdgePixel;
				if (afterP && !afterP.error && !afterP.note && afterP.r === 0 && afterP.g === 0 && afterP.b === 0) {
					console.log(
						`Col ${be.col}: BOTTOM MARGIN pixel is BLACK at grid edge Y=${be.gridEndY}, gap=${be.gapPx}px`,
					);
					hasBlackBottomEdge = true;
				}
				if (edgeP && !edgeP.error && edgeP.r === 0 && edgeP.g === 0 && edgeP.b === 0) {
					console.log(
						`Col ${be.col}: CANVAS BOTTOM EDGE pixel is BLACK`,
					);
					hasBlackBottomEdge = true;
				}
			}
			if (!hasBlackBottomEdge) {
				console.log("No black bottom margin detected");
			}
		}

		await browser.saveScreenshot(
			"./screenshots/canvas-render-debug-02-analysis.png",
		);
	});

	it("should check renderLine fills entire row width and analyze root cause", async () => {
		await browser.pause(500);

		// Check what renderLine actually draws vs the full canvas width
		const fillAnalysis = await browser.execute(() => {
			const canvas = document.querySelector("canvas");
			if (!canvas) return { error: "No canvas" };

			const renderer = window.terminalRenderer;
			if (!renderer) return { error: "No terminalRenderer" };

			const charWidth = renderer.getCharWidth ? renderer.getCharWidth() : renderer["charWidth"];
			const charHeight = renderer.getCharHeight ? renderer.getCharHeight() : renderer["charHeight"];
			const cols = renderer["cols"];
			const rows = renderer["rows"];
			const dpr = renderer["dpr"] || 1;

			// The renderLine method clears with:
			// this.ctx.fillRect(0, y, this.cols * this.charWidth, this.charHeight);
			// This means it only fills up to (cols * charWidth), NOT the full canvas width

			const gridWidth = cols * charWidth;
			const gridHeight = rows * charHeight;
			const canvasCSSWidth = canvas.width / dpr;
			const canvasCSSHeight = canvas.height / dpr;

			// The forceRender method clears the entire canvas:
			// this.ctx.fillRect(0, 0, rect.width, rect.height);
			// But then renderLine only fills gridWidth wide

			// Check if the canvas CSS size matches terminal-root size
			const terminalRoot = document.querySelector(".terminal-root");
			const terminalRootRect = terminalRoot ? terminalRoot.getBoundingClientRect() : null;

			return {
				gridWidth,
				gridHeight,
				canvasCSSWidth,
				canvasCSSHeight,
				terminalRootWidth: terminalRootRect ? terminalRootRect.width : null,
				terminalRootHeight: terminalRootRect ? terminalRootRect.height : null,
				rightGap: canvasCSSWidth - gridWidth,
				bottomGap: canvasCSSHeight - gridHeight,
				renderLineWidth_formula: `cols(${cols}) * charWidth(${charWidth}) = ${gridWidth}`,
				canvasFullWidth: canvasCSSWidth,
				charHeightDetails: {
					fontSize: renderer["fontSize"],
					lineHeightMultiplier: renderer["lineHeightMultiplier"],
					calculatedCharHeight:
						renderer["fontSize"] *
						renderer["lineHeightMultiplier"],
					actualCharHeight: charHeight,
					fractionalPart: charHeight % 1,
				},
				// Check if forceRender was the last render or dirty-row render
				// by looking at the whole canvas fill
				renderInfo: {
					renderLine_fills: `fillRect(0, y, ${gridWidth}, ${charHeight})`,
					forceRender_fills: `fillRect(0, 0, ${canvasCSSWidth}, ${canvasCSSHeight})`,
					issue_right: gridWidth < canvasCSSWidth
						? `renderLine leaves ${canvasCSSWidth - gridWidth}px unfilled on right`
						: "No right gap",
					issue_bottom: gridHeight < canvasCSSHeight
						? `Grid ends at ${gridHeight}px, canvas extends to ${canvasCSSHeight}px (${canvasCSSHeight - gridHeight}px gap)`
						: "No bottom gap",
				},
			};
		});

		console.log(
			"=== Fill Analysis ===",
			JSON.stringify(fillAnalysis, null, 2),
		);

		if (fillAnalysis && !fillAnalysis.error) {
			console.log("\n=== Root Cause Analysis ===");

			if (fillAnalysis.rightGap > 0) {
				console.log(
					`RIGHT EDGE: ${fillAnalysis.rightGap.toFixed(2)}px gap.`,
				);
				console.log(
					`  renderLine fills ${fillAnalysis.gridWidth.toFixed(2)}px but canvas is ${fillAnalysis.canvasCSSWidth}px wide`,
				);
				console.log(
					"  -> forceRender clears the whole canvas with bg color, but dirty-row renderLine only fills gridWidth",
				);
				console.log(
					"  -> After initial forceRender, subsequent dirty renders leave the right margin unfilled (black from canvas default)",
				);
			} else {
				console.log("No right edge gap detected.");
			}

			if (fillAnalysis.bottomGap > 0) {
				console.log(
					`BOTTOM EDGE: ${fillAnalysis.bottomGap.toFixed(2)}px gap.`,
				);
				console.log(
					`  Grid ends at ${fillAnalysis.gridHeight.toFixed(2)}px but canvas is ${fillAnalysis.canvasCSSHeight}px tall`,
				);
				console.log(
					"  -> forceRender clears the whole canvas, but row rendering doesn't reach the bottom margin",
				);
			} else {
				console.log("No bottom edge gap detected.");
			}

			const frac = fillAnalysis.charHeightDetails?.fractionalPart;
			if (frac && frac > 0) {
				console.log(
					`ROW GAPS: charHeight=${fillAnalysis.charHeightDetails.actualCharHeight} has fractional part ${frac.toFixed(6)}`,
				);
				console.log(
					"  -> When rows are rendered at y = rowIndex * charHeight, sub-pixel gaps appear",
				);
				console.log(
					"  -> Canvas fillRect at fractional Y coordinates may leave un-filled pixel rows",
				);
			} else {
				console.log(
					"charHeight is integer - no sub-pixel row gaps from charHeight",
				);
			}
		}

		await browser.saveScreenshot(
			"./screenshots/canvas-render-debug-03-fill-analysis.png",
		);
	});

	it("should capture screenshots with typed content for visual inspection", async () => {
		// Type some text to have content
		const terminal = await $(".terminal-root");
		if (await terminal.isExisting()) {
			await terminal.click();
			await browser.pause(500);

			// Type some text
			await browser.keys("echo 'test line 1'");
			await browser.keys("Enter");
			await browser.pause(500);

			await browser.keys("echo 'test line 2'");
			await browser.keys("Enter");
			await browser.pause(500);

			await browser.keys("ls -la");
			await browser.keys("Enter");
			await browser.pause(1000);
		}

		// Take screenshot with content
		await browser.saveScreenshot(
			"./screenshots/canvas-render-debug-04-with-content.png",
		);

		// Final comprehensive state dump
		const finalState = await browser.execute(() => {
			const canvas = document.querySelector("canvas");
			if (!canvas) return { error: "No canvas" };

			const canvasStyle = getComputedStyle(canvas);

			// Check all parent element backgrounds
			const parents = [];
			let el = canvas.parentElement;
			while (el && parents.length < 5) {
				const style = getComputedStyle(el);
				parents.push({
					tag: el.tagName,
					id: el.id,
					class: el.className,
					bg: style.backgroundColor,
					width: style.width,
					height: style.height,
				});
				el = el.parentElement;
			}

			return {
				canvasDisplay: canvasStyle.display,
				canvasBg: canvasStyle.backgroundColor,
				parents,
			};
		});

		console.log(
			"=== DOM Hierarchy ===",
			JSON.stringify(finalState, null, 2),
		);
	});
});
