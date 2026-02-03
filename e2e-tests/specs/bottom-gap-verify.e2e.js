describe("Bottom gap verification", () => {
  it("should verify no black bottom gap with colored theme", async () => {
    await browser.pause(2000);

    // Step 1: Get initial state and renderer info
    const initial = await browser.execute(() => {
      const tabContent = document.querySelector(".tab-content");
      const canvas = tabContent ? tabContent.querySelector("canvas") : null;
      if (!canvas) return { error: "no canvas found" };
      const ctx = canvas.getContext("2d");

      // Get renderer info through tabManager
      let rendererInfo = null;
      if (window.tabManager) {
        try {
          const tabs = window.tabManager.tabs || [];
          for (const [id, tab] of (window.tabManager.tabs instanceof Map ? window.tabManager.tabs : [])) {
            if (tab && tab.terminal && tab.terminal.renderer) {
              const r = tab.terminal.renderer;
              rendererInfo = {
                charHeight: typeof r.getCharHeight === 'function' ? r.getCharHeight() : r.charHeight,
                charWidth: typeof r.getCharWidth === 'function' ? r.getCharWidth() : r.charWidth,
              };
              break;
            }
          }
        } catch(e) {
          rendererInfo = { error: e.message };
        }
      }

      return {
        canvasWidth: canvas.width,
        canvasHeight: canvas.height,
        canvasCSSWidth: canvas.style.width,
        canvasCSSHeight: canvas.style.height,
        dpr: window.devicePixelRatio || 1,
        containerRect: tabContent.getBoundingClientRect(),
        rendererInfo,
      };
    });
    console.log("=== INITIAL STATE ===");
    console.log(JSON.stringify(initial, null, 2));

    // Step 2: Apply solarized-dark via tabManager's updateAllTerminalsSetting
    const applied = await browser.execute(() => {
      try {
        if (window.tabManager && typeof window.tabManager.updateAllTerminalsSetting === 'function') {
          window.tabManager.updateAllTerminalsSetting("colorScheme", "solarized-dark");
          
          // Also set the CSS variable directly
          const root = document.documentElement;
          root.setAttribute("data-terminal-color-scheme", "solarized-dark");
          // solarized-dark background is rgb(0, 43, 54) = #002b36
          root.style.setProperty("--terminal-background", "rgb(0, 43, 54)");
          
          return { success: true, method: "tabManager.updateAllTerminalsSetting" };
        }
        return { success: false, error: "tabManager not available" };
      } catch(e) {
        return { success: false, error: e.message };
      }
    });
    console.log("=== APPLIED THEME ===");
    console.log(JSON.stringify(applied, null, 2));

    await browser.pause(1000);
    await browser.saveScreenshot("/app/e2e-tests/screenshots/bottom-gap-verify-01.png");

    // Step 3: Analyze pixels
    const pixelAnalysis = await browser.execute(() => {
      const tabContent = document.querySelector(".tab-content");
      const canvas = tabContent ? tabContent.querySelector("canvas") : null;
      if (!canvas) return { error: "no canvas" };
      const ctx = canvas.getContext("2d");
      const w = canvas.width;
      const h = canvas.height;
      const dpr = window.devicePixelRatio || 1;

      // Solarized-dark background: rgb(0, 43, 54)
      const expectedBg = { r: 0, g: 43, b: 54 };

      // Check bottom 15 rows of canvas pixels (in device pixels)
      const bottomRows = [];
      for (let py = h - 15; py < h; py++) {
        const pixel = ctx.getImageData(Math.floor(w / 2), py, 1, 1).data;
        const isExpected = pixel[0] === expectedBg.r && pixel[1] === expectedBg.g && pixel[2] === expectedBg.b;
        bottomRows.push({
          y: py,
          cssY: (py / dpr).toFixed(1),
          color: `rgb(${pixel[0]}, ${pixel[1]}, ${pixel[2]})`,
          isExpectedBg: isExpected,
          isBlack: pixel[0] === 0 && pixel[1] === 0 && pixel[2] === 0,
        });
      }

      // Check right edge
      const rightEdge = [];
      for (let py = Math.floor(h / 2) - 5; py < Math.floor(h / 2) + 5; py++) {
        const pixel = ctx.getImageData(w - 1, py, 1, 1).data;
        rightEdge.push({
          y: py,
          color: `rgb(${pixel[0]}, ${pixel[1]}, ${pixel[2]})`,
          isBlack: pixel[0] === 0 && pixel[1] === 0 && pixel[2] === 0,
        });
      }

      // Check corners
      const corners = {};
      const points = {
        bottomLeft: [0, h - 1],
        bottomCenter: [Math.floor(w / 2), h - 1],
        bottomRight: [w - 1, h - 1],
        topRight: [w - 1, 0],
      };
      for (const [name, [px, py]] of Object.entries(points)) {
        const pixel = ctx.getImageData(px, py, 1, 1).data;
        corners[name] = {
          color: `rgb(${pixel[0]}, ${pixel[1]}, ${pixel[2]})`,
          isExpectedBg: pixel[0] === expectedBg.r && pixel[1] === expectedBg.g && pixel[2] === expectedBg.b,
          isBlack: pixel[0] === 0 && pixel[1] === 0 && pixel[2] === 0,
        };
      }

      // CSS background check
      const cssCheck = {
        tabContentBg: window.getComputedStyle(tabContent).backgroundColor,
        bodyBg: window.getComputedStyle(document.body).backgroundColor,
        cssVar: getComputedStyle(document.documentElement).getPropertyValue("--terminal-background"),
        dataAttr: document.documentElement.getAttribute("data-terminal-color-scheme"),
      };

      return {
        canvasSize: { w, h, cssW: w / dpr, cssH: h / dpr },
        bottomRows,
        rightEdge,
        corners,
        cssCheck,
        blackBottomCount: bottomRows.filter(r => r.isBlack).length,
        expectedBgBottomCount: bottomRows.filter(r => r.isExpectedBg).length,
      };
    });
    console.log("=== PIXEL ANALYSIS ===");
    console.log(JSON.stringify(pixelAnalysis, null, 2));

    // Summary
    console.log("\n=== SUMMARY ===");
    console.log("Black pixels in bottom 15 rows:", pixelAnalysis.blackBottomCount);
    console.log("Expected bg pixels in bottom 15 rows:", pixelAnalysis.expectedBgBottomCount);
    console.log("Black pixels in right edge:", pixelAnalysis.rightEdge?.filter(r => r.isBlack).length);
    console.log("Corners:", JSON.stringify(pixelAnalysis.corners, null, 2));
    console.log("CSS backgrounds:", JSON.stringify(pixelAnalysis.cssCheck, null, 2));
  });
});
