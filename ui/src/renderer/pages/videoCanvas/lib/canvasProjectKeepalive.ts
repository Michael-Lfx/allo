/**
 * Unload-safe best-effort project save fired from `pagehide`.
 *
 * The canonical save path (`syncCanvasProjectToServer` in ./ocBridge) goes
 * through `httpRequest`, whose plain `fetch` is not marked `keepalive` and may
 * be cancelled by the browser while the page unloads. This module duplicates
 * only the doc serialization so a `keepalive: true` PUT can be fired as dual
 * insurance. PUT is idempotent, so a redundant request is harmless. Keepalive
 * bodies are capped at 64 KiB, so oversized documents silently fall back to
 * the regular flush; failures are swallowed by design (best effort only).
 */

import { buildBackendAuthHeaders, getBaseUrl } from '@/common/adapter/httpBridge';

import { useCanvasStore } from '@oc/stores/canvas/use-canvas-store';
import { projectToCanvasDocument } from './canvasChatPersist';

const KEEPALIVE_MAX_BYTES = 60 * 1024;

export function keepaliveSyncCanvasProject(projectId: string): void {
  try {
    const project = useCanvasStore.getState().projects.find((p) => p.id === projectId);
    if (!project) return;
    const body = JSON.stringify(projectToCanvasDocument(project));
    if (body.length > KEEPALIVE_MAX_BYTES) return;
    const url = `${getBaseUrl()}/api/video-canvas/projects/${encodeURIComponent(projectId)}/doc`;
    void fetch(url, {
      method: 'PUT',
      keepalive: true,
      headers: { 'Content-Type': 'application/json', ...buildBackendAuthHeaders('PUT') },
      body,
    }).catch(() => {
      // 卸载兜底：失败时静默，常规 flush 已尽力。
    });
  } catch {
    // 序列化/构造失败不影响常规 flush。
  }
}
