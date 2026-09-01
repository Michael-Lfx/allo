/**
 * Single loader for the canvas Agent panel chunk.
 *
 * React.lazy and every prefetch site MUST resolve the module through this
 * function: Vite emits one chunk per dynamic-import specifier, so a second
 * import path would duplicate the module graph.
 */
export function loadCanvasAssistantPanel() {
  return import('@oc/components/canvas/canvas-assistant-panel');
}
