/**
 * Backspace Debug E2E Test
 *
 * Tests backspace functionality and rendering updates.
 */

describe('Backspace Debug Test', () => {
  async function getTerminalState() {
    return await browser.execute(() => {
      const ts = window.terminalState;
      if (!ts) return null;
      return {
        cols: ts.cols,
        rows: ts.rows,
        cursorCol: ts.cursorCol,
        cursorRow: ts.cursorRow,
        dirtyRows: ts.getDirtyRows?.() || [],
      };
    });
  }

  async function getBufferContent(row) {
    return await browser.execute((r) => {
      const ts = window.terminalState;
      if (!ts) return null;
      const buffer = ts.getActiveBuffer?.();
      if (!buffer) return null;
      const line = buffer.getLine(r);
      let text = '';
      for (let i = 0; i < line.length; i++) {
        text += line.getCell(i).char;
      }
      return text.trim();
    }, row);
  }

  async function getDOMContent(row) {
    return await browser.execute((r) => {
      const terminal = document.getElementById('terminal');
      const lines = terminal?.querySelectorAll('.terminal-line');
      if (!lines || r >= lines.length) return null;
      return lines[r].textContent?.trim();
    }, row);
  }

  it('should test backspace functionality', async () => {
    const terminal = await $('#terminal');
    await terminal.click();
    await browser.pause(1000);

    // Set up console log capture
    await browser.execute(() => {
      window.consoleLogs = [];
      const originalLog = console.log;
      console.log = function(...args) {
        window.consoleLogs.push(args.join(' '));
        originalLog.apply(console, args);
      };
    });

    // Get initial state
    const initialState = await getTerminalState();
    console.log('Initial cursor position:', initialState?.cursorCol);

    // Type "abc"
    console.log('Typing "abc"...');
    await browser.keys('a');
    await browser.pause(100);
    await browser.keys('b');
    await browser.pause(100);
    await browser.keys('c');
    await browser.pause(300);

    // Get state after typing
    const afterTypingState = await getTerminalState();
    const row = afterTypingState?.cursorRow || 0;
    const bufferAfterType = await getBufferContent(row);
    const domAfterType = await getDOMContent(row);

    console.log('=== After typing "abc" ===');
    console.log('Cursor col:', afterTypingState?.cursorCol);
    console.log('Buffer:', bufferAfterType);
    console.log('DOM:', domAfterType);

    await browser.saveScreenshot('./screenshots/backspace-01-after-typing.png');

    // Press Backspace
    console.log('Pressing Backspace...');
    await browser.keys('Backspace');
    await browser.pause(500);

    // Get state after backspace
    const afterBackspaceState = await getTerminalState();
    const bufferAfterBackspace = await getBufferContent(row);
    const domAfterBackspace = await getDOMContent(row);

    console.log('=== After Backspace ===');
    console.log('Cursor col:', afterBackspaceState?.cursorCol);
    console.log('Buffer:', bufferAfterBackspace);
    console.log('DOM:', domAfterBackspace);

    // Get render logs
    const logs = await browser.execute(() => {
      return window.consoleLogs?.filter(log =>
        log.includes('[scheduleRender]') ||
        log.includes('[render]') ||
        log.includes('[renderLineOptimized]') ||
        log.includes('[KeyDown]')
      ) || [];
    });

    console.log('=== Render logs ===');
    for (const log of logs) {
      console.log(log);
    }

    await browser.saveScreenshot('./screenshots/backspace-02-after-backspace.png');

    // Verify backspace worked
    const cursorMoved = afterBackspaceState?.cursorCol < afterTypingState?.cursorCol;
    console.log('Cursor moved back:', cursorMoved);

    // Press more backspaces
    console.log('Pressing 2 more Backspaces...');
    await browser.keys('Backspace');
    await browser.pause(200);
    await browser.keys('Backspace');
    await browser.pause(500);

    const afterMultiBackspace = await getBufferContent(row);
    const domAfterMultiBackspace = await getDOMContent(row);

    console.log('=== After 2 more Backspaces ===');
    console.log('Buffer:', afterMultiBackspace);
    console.log('DOM:', domAfterMultiBackspace);

    await browser.saveScreenshot('./screenshots/backspace-03-after-multi.png');
  });

  it('should test Enter key', async () => {
    const terminal = await $('#terminal');
    await terminal.click();
    await browser.pause(500);

    // Clear logs
    await browser.execute(() => {
      window.consoleLogs = [];
    });

    // Get initial row
    const initialState = await getTerminalState();
    const initialRow = initialState?.cursorRow || 0;
    console.log('Initial row:', initialRow);

    // Type a command
    console.log('Typing "echo hello"...');
    await browser.keys(['e', 'c', 'h', 'o', ' ', 'h', 'e', 'l', 'l', 'o']);
    await browser.pause(300);

    const bufferBeforeEnter = await getBufferContent(initialRow);
    console.log('Buffer before Enter:', bufferBeforeEnter);

    await browser.saveScreenshot('./screenshots/backspace-04-before-enter.png');

    // Press Enter
    console.log('Pressing Enter...');
    await browser.keys('Enter');
    await browser.pause(1000);

    // Get new state
    const afterEnterState = await getTerminalState();
    console.log('Row after Enter:', afterEnterState?.cursorRow);

    // Get logs
    const logs = await browser.execute(() => {
      return window.consoleLogs?.filter(log =>
        log.includes('[KeyDown]') ||
        log.includes('[scheduleRender]') ||
        log.includes('[render]')
      ) || [];
    });

    console.log('=== Logs after Enter ===');
    for (const log of logs) {
      console.log(log);
    }

    // Get content of several rows
    for (let i = 0; i <= Math.min(5, afterEnterState?.cursorRow || 2); i++) {
      const content = await getBufferContent(i);
      console.log(`Row ${i}:`, content);
    }

    await browser.saveScreenshot('./screenshots/backspace-05-after-enter.png');
  });
});
