import {
  openSettings,
  switchCategory,
  setSelect,
} from "../helpers/settings-helpers.js";

describe("Bottom gap debug", () => {
  it("should check bottom gap with solarized-dark theme", async () => {
    // Wait for app to load
    await browser.pause(2000);

    // Take initial screenshot
    await browser.saveScreenshot("/app/e2e-tests/screenshots/bottom-gap-01-initial.png");

    // Get terminal dimensions
    const dims = await browser.execute(() => {
      const tabContent = document.querySelector(".tab-content");
      const canvas = tabContent ? tabContent.querySelector("canvas") : null;
      const termRoot = tabContent ? tabContent.querySelector(".terminal-root") : null;
      
      const result = {
        tabContent: tabContent ? {
          width: tabContent.offsetWidth,
          height: tabContent.offsetHeight,
          computedBg: window.getComputedStyle(tabContent).backgroundColor,
          rect: tabContent.getBoundingClientRect(),
        } : null,
        canvas: canvas ? {
          width: canvas.width,
          height: canvas.height,
          styleWidth: canvas.style.width,
          styleHeight: canvas.style.height,
          rect: canvas.getBoundingClientRect(),
        } : null,
        termRoot: termRoot ? {
          width: termRoot.offsetWidth,
          height: termRoot.offsetHeight,
          rect: termRoot.getBoundingClientRect(),
        } : null,
        body: {
          computedBg: window.getComputedStyle(document.body).backgroundColor,
        },
        terminal: document.querySelector('[data-testid="terminal"]') ? {
          computedBg: window.getComputedStyle(document.querySelector('[data-testid="terminal"]')).backgroundColor,
        } : null,
        cssVarBackground: getComputedStyle(document.documentElement).getPropertyValue("--terminal-background"),
      };
      return result;
    });
    console.log("=== INITIAL DIMENSIONS ===");
    console.log(JSON.stringify(dims, null, 2));

    // Apply solarized-dark color scheme via settings UI
    await openSettings();
    await browser.pause(500);
    await switchCategory("terminal-appearance");
    await browser.pause(500);
    await setSelect("settings-terminal-color-scheme", "solarized-dark");
    await browser.pause(1000);

    // Close settings by clicking on a terminal tab
    const terminalTab = await browser.$(".tab-bar .tab:first-child");
    if (terminalTab) {
      await terminalTab.click();
      await browser.pause(500);
    }

    // Take screenshot after theme change
    await browser.saveScreenshot("/app/e2e-tests/screenshots/bottom-gap-02-solarized.png");

    // Get dimensions after theme change
    const dims2 = await browser.execute(() => {
      const tabContent = document.querySelector(".tab-content");
      const canvas = tabContent ? tabContent.querySelector("canvas") : null;
      const termRoot = tabContent ? tabContent.querySelector(".terminal-root") : null;
      
      // Check renderer state
      let rendererInfo = null;
      if (window.tabManager) {
        const tabs = window.tabManager.getTabs?.() || [];
        const terminalTab = tabs.find(t => t.type === "terminal");
        if (terminalTab) {
          const app = window.tabManager.getTerminalApp?.(terminalTab.id);
          if (app && app.terminalRenderer) {
            const r = app.terminalRenderer;
            rendererInfo = {
              charHeight: r.getCharHeight?.(),
              charWidth: r.getCharWidth?.(),
              cols: r.cols,
              rows: r.rows,
              gridWidth: r.cols * (r.getCharWidth?.() || 0),
              gridHeight: r.rows * (r.getCharHeight?.() || 0),
              currentBackground: r.currentBackground,
            };
          }
        }
      }

      return {
        tabContent: tabContent ? {
          width: tabContent.offsetWidth,
          height: tabContent.offsetHeight,
          computedBg: window.getComputedStyle(tabContent).backgroundColor,
          padding: window.getComputedStyle(tabContent).padding,
          rect: tabContent.getBoundingClientRect(),
        } : null,
        canvas: canvas ? {
          width: canvas.width,
          height: canvas.height,
          styleWidth: canvas.style.width,
          styleHeight: canvas.style.height,
          rect: canvas.getBoundingClientRect(),
        } : null,
        termRoot: termRoot ? {
          width: termRoot.offsetWidth,
          height: termRoot.offsetHeight,
          rect: termRoot.getBoundingClientRect(),
        } : null,
        body: {
          computedBg: window.getComputedStyle(document.body).backgroundColor,
        },
        cssVarBackground: getComputedStyle(document.documentElement).getPropertyValue("--terminal-background"),
        dataSchemeAttr: document.documentElement.getAttribute("data-terminal-color-scheme"),
        rendererInfo,
      };
    });
    console.log("=== AFTER SOLARIZED-DARK ===");
    console.log(JSON.stringify(dims2, null, 2));

    // Pixel analysis: check bottom area of canvas
    const bottomPixels = await browser.execute(() => {
      const canvas = document.querySelector(".tab-content canvas");
      if (!canvas) return { error: "no canvas" };
      const ctx = canvas.getContext("2d");
      const w = canvas.width;
      const h = canvas.height;

      // Sample bottom 10 rows of pixels
      const results = [];
      for (let py = h - 10; py < h; py++) {
        const pixel = ctx.getImageData(w / 2, py, 1, 1).data;
        results.push({ y: py, r: pixel[0], g: pixel[1], b: pixel[2], a: pixel[3] });
      }

      // Also check if the canvas covers the full container
      const tabContent = document.querySelector(".tab-content");
      const tabRect = tabContent.getBoundingClientRect();
      const canvasRect = canvas.getBoundingClientRect();
      
      return {
        bottomPixels: results,
        canvasCSSBottom: canvasRect.bottom,
        containerCSSBottom: tabRect.bottom,
        gapBelowCanvas: tabRect.bottom - canvasRect.bottom,
        canvasCSSHeight: canvasRect.height,
        containerCSSHeight: tabRect.height,
        containerPadding: window.getComputedStyle(tabContent).padding,
      };
    });
    console.log("=== BOTTOM PIXEL ANALYSIS ===");
    console.log(JSON.stringify(bottomPixels, null, 2));

    // Check if forceRender fills entire canvas
    const forceRenderCheck = await browser.execute(() => {
      const canvas = document.querySelector(".tab-content canvas");
      if (!canvas) return { error: "no canvas" };
      const ctx = canvas.getContext("2d");
      const w = canvas.width;
      const h = canvas.height;

      // Check corners and edges
      const checks = {
        topLeft: ctx.getImageData(0, 0, 1, 1).data,
        topRight: ctx.getImageData(w - 1, 0, 1, 1).data,
        bottomLeft: ctx.getImageData(0, h - 1, 1, 1).data,
        bottomRight: ctx.getImageData(w - 1, h - 1, 1, 1).data,
        bottomCenter: ctx.getImageData(Math.floor(w / 2), h - 1, 1, 1).data,
        // Row at grid boundary  
        rightEdgeMiddle: ctx.getImageData(w - 1, Math.floor(h / 2), 1, 1).data,
      };

      // Convert to readable format
      const readable = {};
      for (const [key, val] of Object.entries(checks)) {
        readable[key] = `rgb(${val[0]}, ${val[1]}, ${val[2]})`;
      }
      return readable;
    });
    console.log("=== CORNER PIXEL CHECK ===");
    console.log(JSON.stringify(forceRenderCheck, null, 2));

    console.log("=== INVESTIGATION COMPLETE ===");
  });
});
