/**
 * CSS styles for ZoomController UI components.
 *
 * @module shared/zoom-styles
 */

/**
 * CSS styles for the close button and zoom control bar.
 */
export const ZOOM_CONTROLLER_STYLES = `
.viewer-close-button {
  position: fixed;
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

.viewer-zoom-bar {
  position: fixed;
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

.viewer-zoom-button {
  width: 28px;
  height: 28px;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: white;
  font-size: 16px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
}

.viewer-zoom-button:hover {
  background: rgba(255, 255, 255, 0.1);
}

.viewer-zoom-level {
  min-width: 50px;
  text-align: center;
  color: white;
  font-family: monospace;
  font-size: 12px;
  cursor: pointer;
  padding: 4px 8px;
  border-radius: 4px;
}

.viewer-zoom-level:hover {
  background: rgba(255, 255, 255, 0.1);
}
`;
