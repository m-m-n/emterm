/**
 * Exit Command E2E Test - Tests if "exit" command closes the window
 */

async function typeSlowly(text, delay = 100) {
  for (const char of text) {
    await browser.keys(char);
    await browser.pause(delay);
  }
}

describe('Exit Command Test', () => {

  it('should close window when typing exit command', async () => {
    const terminal = await $('#terminal');
    await terminal.click();

    // Wait for prompt
    await browser.pause(1000);
    await browser.saveScreenshot('./screenshots/exit-01-initial.png');

    // Type "exit" command
    console.log('Typing exit command...');
    await typeSlowly('exit', 100);
    await browser.pause(500);
    await browser.saveScreenshot('./screenshots/exit-02-typed.png');

    // Press Enter
    console.log('Pressing Enter...');
    await browser.keys('Enter');

    // Wait for window to close
    console.log('Waiting for window to close...');
    await browser.pause(3000);

    // Check if window is still open
    try {
      const title = await browser.getTitle();
      console.log('Window still open, title:', title);
      await browser.saveScreenshot('./screenshots/exit-03-after.png');

      // Get terminal content
      const terminalText = await terminal.getText();
      console.log('Terminal content:', terminalText.slice(0, 200));
    } catch (e) {
      console.log('Window closed or error:', e.message);
    }
  });

  it('should check Ctrl+D key event handling', async () => {
    const terminal = await $('#terminal');
    await terminal.click();
    await browser.pause(500);

    // Test via JavaScript execution - dispatch Ctrl+D and check if write is called
    console.log('Testing Ctrl+D via synthetic keydown...');
    const result = await browser.execute(async () => {
      const ptyClient = window.ptyClient;
      const sessionId = ptyClient?.getSessionId?.() || null;

      // Create and dispatch a synthetic Ctrl+D keydown event
      const event = new KeyboardEvent('keydown', {
        key: 'd',
        code: 'KeyD',
        ctrlKey: true,
        bubbles: true,
        cancelable: true
      });

      // Dispatch the event to the document (where our keydown handler listens)
      document.dispatchEvent(event);

      // Wait a moment for the async write to complete
      await new Promise(resolve => setTimeout(resolve, 500));

      // Get updated session status
      const newSessionId = ptyClient?.getSessionId?.() || null;

      return {
        originalSessionId: sessionId,
        newSessionId: newSessionId,
        sessionWasCleared: sessionId !== null && newSessionId === null,
        ptyClientExists: !!ptyClient,
      };
    });

    console.log('Ctrl+D dispatch result:', JSON.stringify(result, null, 2));

    // Wait longer to see if window closes
    await browser.pause(3000);

    try {
      const title = await browser.getTitle();
      console.log('Window still open after Ctrl+D, title:', title);
      await browser.saveScreenshot('./screenshots/exit-04-after-ctrlD.png');
    } catch (e) {
      console.log('Window closed after Ctrl+D:', e.message);
    }
  });

  it('should check pty_exit event listener', async () => {
    const terminal = await $('#terminal');
    await terminal.click();
    await browser.pause(500);

    // Check if pty_exit listener is registered and set up a test listener
    console.log('Setting up pty_exit test listener...');
    const result = await browser.execute(async () => {
      const ptyClient = window.ptyClient;

      // Set up a custom listener to capture pty_exit events
      let exitEventReceived = false;
      let exitEventPayload = null;

      // Import listen function and set up a direct listener
      const { listen } = await import('@tauri-apps/api/event');
      const unlisten = await listen('pty_exit', (event) => {
        console.log('[TEST] pty_exit event received:', event);
        exitEventReceived = true;
        exitEventPayload = event.payload;
      });

      return {
        ptyClientExists: !!ptyClient,
        sessionId: ptyClient?.getSessionId?.() || null,
        testListenerSetup: true,
      };
    });

    console.log('Listener check result:', JSON.stringify(result, null, 2));
    await browser.saveScreenshot('./screenshots/exit-05-listener-check.png');
  });

  it('should test exit with debug logging', async () => {
    const terminal = await $('#terminal');
    await terminal.click();
    await browser.pause(500);

    // Get initial state
    const initialState = await browser.execute(() => {
      return {
        sessionId: window.ptyClient?.getSessionId?.() || null,
      };
    });
    console.log('Initial state:', JSON.stringify(initialState, null, 2));

    // Type exit and wait for result
    console.log('Typing exit...');
    await typeSlowly('exit', 100);
    await browser.pause(300);

    // Press Enter
    console.log('Pressing Enter...');
    await browser.keys('Enter');

    // Check immediately after Enter
    await browser.pause(100);
    const afterEnter = await browser.execute(() => {
      return {
        sessionId: window.ptyClient?.getSessionId?.() || null,
      };
    });
    console.log('After Enter (100ms):', JSON.stringify(afterEnter, null, 2));

    // Wait longer
    await browser.pause(2000);
    const afterWait = await browser.execute(() => {
      return {
        sessionId: window.ptyClient?.getSessionId?.() || null,
      };
    });
    console.log('After wait (2s):', JSON.stringify(afterWait, null, 2));

    // Check console logs
    const consoleOutput = await browser.execute(() => {
      // Return any console messages that were logged
      return {
        sessionIdNow: window.ptyClient?.getSessionId?.() || null,
      };
    });
    console.log('Final state:', JSON.stringify(consoleOutput, null, 2));

    await browser.saveScreenshot('./screenshots/exit-06-debug.png');
  });

});
