/**
 * CSS styles for DisplayModeController UI components.
 *
 * @module image-viewer/display-mode-styles
 */

/**
 * CSS styles for the close button and mode toggle bar.
 */
export const DISPLAY_MODE_STYLES = `
.viewer-close-button {
  position: absolute;
  top: 16px;
  right: 16px;
  width: 32px;
  height: 32px;
  background: rgba(0, 0, 0, 0.5);
  border: none;
  border-radius: 6px;
  color: white;
  font-size: 18px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10001;
  transition: background 0.15s ease;
}

.viewer-close-button:hover {
  background: rgba(0, 0, 0, 0.7);
}

.viewer-mode-bar {
  position: absolute;
  bottom: 16px;
  right: 16px;
  display: flex;
  align-items: center;
  gap: 4px;
  background: rgba(0, 0, 0, 0.5);
  border-radius: 6px;
  padding: 4px;
  z-index: 10001;
}

.viewer-mode-button {
  height: 28px;
  padding: 0 12px;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: white;
  font-family: "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace;
  font-size: 12px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.15s ease;
}

.viewer-mode-button:hover {
  background: rgba(255, 255, 255, 0.1);
}

.viewer-mode-button.active {
  background: rgba(255, 255, 255, 0.2);
}

.viewer-mode-toggle {
  min-width: 50px;
  text-align: center;
  color: white;
  font-family: "Inconsolata", "Noto Sans JP", "Noto Color Emoji", monospace;
  font-size: 12px;
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 4px;
  border: none;
  background: transparent;
  transition: background 0.15s ease;
}

.viewer-mode-toggle:hover {
  background: rgba(255, 255, 255, 0.1);
}
`;
