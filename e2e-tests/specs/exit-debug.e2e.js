/**
 * Exit Debug E2E Test - Tests exit with longer wait time
 */

async function typeSlowly(text, delay = 150) {
  for (const char of text) {
    await browser.keys(char);
    await browser.pause(delay);
  }
}

describe('Exit Debug Test', () => {

  it('should test exit command with long wait', async () => {
    const terminal = await $('#terminal');
    await terminal.click();

    // Wait for shell to be ready
    console.log('Waiting for shell to be ready...');
    await browser.pause(3000);

    // Get initial session ID
    const initialState = await browser.execute(() => {
      return {
        sessionId: window.ptyClient?.getSessionId?.() || null,
        ptyExists: !!window.ptyClient,
      };
    });
    console.log('Initial state:', JSON.stringify(initialState, null, 2));
    await browser.saveScreenshot('./screenshots/exit-debug-01-initial.png');

    // Type "exit" slowly and clearly
    console.log('Typing exit command...');
    await typeSlowly('exit', 200);
    await browser.pause(1000);
    await browser.saveScreenshot('./screenshots/exit-debug-02-typed.png');

    // Press Enter
    console.log('Pressing Enter...');
    await browser.keys('Enter');
    await browser.pause(500);

    // Check if shell already exited after exit + Enter
    const stateAfterExit = await browser.execute(() => {
      return {
        sessionId: window.ptyClient?.getSessionId?.() || null,
      };
    });
    console.log('State after exit+Enter:', JSON.stringify(stateAfterExit));

    // Send Ctrl+D via WebDriver keys (Ctrl+d should send 0x04)
    console.log('Sending Ctrl+D via WebDriver keys...');
    await browser.keys(['Control', 'd']);
    await browser.pause(500);

    // Also try sending it as a key combination
    console.log('Sending Ctrl+D key combination...');
    await browser.keys(['\uE009', 'd', '\uE009']); // Control down, d, Control up
    await browser.pause(1000);

    // Wait for shell to exit
    console.log('Waiting for shell to exit...');
    for (let i = 0; i < 10; i++) {
      await browser.pause(1000);
      const state = await browser.execute(() => {
        return {
          sessionId: window.ptyClient?.getSessionId?.() || null,
        };
      });
      console.log(`After ${i + 1}s: sessionId = ${state.sessionId}`);
      if (state.sessionId === null) {
        console.log('Session cleared - shell exited!');
        break;
      }
    }

    await browser.saveScreenshot('./screenshots/exit-debug-03-after-wait.png');

    // Check if window is still open
    try {
      const title = await browser.getTitle();
      console.log('Window still open, title:', title);
    } catch (e) {
      console.log('Window closed:', e.message);
    }
  });

});
