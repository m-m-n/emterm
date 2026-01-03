/**
 * Startup Render Debug E2E Test
 *
 * Captures the full render flow from startup to detect rendering issues.
 */

describe('Startup Render Test', () => {
  it('should capture render flow on startup', async () => {
    // Wait for terminal to initialize
    await browser.pause(2000);

    // Check if terminal exists
    const terminal = await $('#terminal');
    const exists = await terminal.isExisting();
    console.log('Terminal element exists:', exists);

    // Get terminal state
    const state = await browser.execute(() => {
      const ts = window.terminalState;
      if (!ts) return null;
      return {
        cols: ts.cols,
        rows: ts.rows,
        cursorCol: ts.cursorCol,
        cursorRow: ts.cursorRow,
      };
    });
    console.log('Terminal state:', JSON.stringify(state));

    // Get renderer state
    const rendererState = await browser.execute(() => {
      const tr = window.terminalRenderer;
      if (!tr) return null;
      return {
        lineElementCount: tr.lineElements?.length || 0,
        useOptimizedRendering: tr.useOptimizedRendering,
        hashCacheSize: tr.lastRowHash?.size || 0,
      };
    });
    console.log('Renderer state:', JSON.stringify(rendererState));

    // Get DOM line count
    const lineCount = await browser.execute(() => {
      const terminal = document.getElementById('terminal');
      return terminal?.querySelectorAll('.terminal-line').length || 0;
    });
    console.log('DOM line elements:', lineCount);

    // Get first 5 lines content from buffer and DOM
    for (let i = 0; i < 5; i++) {
      const content = await browser.execute((row) => {
        const ts = window.terminalState;
        if (!ts) return { buffer: null, dom: null };
        const buffer = ts.getActiveBuffer?.();
        if (!buffer) return { buffer: null, dom: null };

        // Get buffer content
        const line = buffer.getLine(row);
        let bufferText = '';
        for (let j = 0; j < line.length; j++) {
          bufferText += line.getCell(j).char;
        }

        // Get DOM content
        const terminal = document.getElementById('terminal');
        const lines = terminal?.querySelectorAll('.terminal-line');
        const domText = lines && row < lines.length ? lines[row].textContent : null;

        return {
          buffer: bufferText.trim(),
          dom: domText?.trim() || null
        };
      }, i);
      console.log(`Row ${i}: buffer="${content.buffer}" dom="${content.dom}"`);
    }

    // Take screenshot
    await browser.saveScreenshot('./screenshots/startup-01-initial.png');

    // Now type a character and check render
    console.log('=== Typing character "a" ===');
    await terminal.click();
    await browser.pause(200);
    await browser.keys('a');
    await browser.pause(500);

    // Check state after typing
    const afterTyping = await browser.execute(() => {
      const ts = window.terminalState;
      if (!ts) return null;
      return {
        cursorCol: ts.cursorCol,
        cursorRow: ts.cursorRow,
        dirtyRows: ts.getDirtyRows?.() || [],
      };
    });
    console.log('After typing "a":', JSON.stringify(afterTyping));

    // Check content at cursor row
    const cursorRow = afterTyping?.cursorRow || 0;
    const rowContent = await browser.execute((row) => {
      const ts = window.terminalState;
      if (!ts) return { buffer: null, dom: null };
      const buffer = ts.getActiveBuffer?.();
      if (!buffer) return { buffer: null, dom: null };

      const line = buffer.getLine(row);
      let bufferText = '';
      for (let j = 0; j < line.length; j++) {
        bufferText += line.getCell(j).char;
      }

      const terminal = document.getElementById('terminal');
      const lines = terminal?.querySelectorAll('.terminal-line');
      const domText = lines && row < lines.length ? lines[row].textContent : null;

      return {
        buffer: bufferText.trim(),
        dom: domText?.trim() || null
      };
    }, cursorRow);
    console.log(`Cursor row ${cursorRow}: buffer="${rowContent.buffer}" dom="${rowContent.dom}"`);

    await browser.saveScreenshot('./screenshots/startup-02-after-a.png');

    // Press Enter
    console.log('=== Pressing Enter ===');
    await browser.keys('Enter');
    await browser.pause(1000);

    const afterEnter = await browser.execute(() => {
      const ts = window.terminalState;
      if (!ts) return null;
      return {
        cursorCol: ts.cursorCol,
        cursorRow: ts.cursorRow,
      };
    });
    console.log('After Enter:', JSON.stringify(afterEnter));

    await browser.saveScreenshot('./screenshots/startup-03-after-enter.png');

    // Verify terminal is responsive
    expect(state).not.toBeNull();
    expect(lineCount).toBeGreaterThan(0);
  });
});
