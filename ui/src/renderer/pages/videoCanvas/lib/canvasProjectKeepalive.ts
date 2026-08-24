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
import type { CanvasDocument } from '../types';

export function keepaliveSyncCanvasProject(projectId: string): void {
  try {
    const project = useCanvasStore.getState().projects.find((p) => p.id === projectId);
    if (!project) return;
    const doc: CanvasDocument = {
      schema: 1,
      title: project.title,
      nodes: project.nodes as unknown as CanvasDocument['nodes'],
      connections: project.connections as unknown as CanvasDocument['connections'],
      viewport: project.viewport as CanvasDocument['viewport'],
      backgroundMode: (project.backgroundMode as CanvasDocument['backgroundMode']) || 'dots',
      ...(project.timeline ? { timeline: project.timeline as CanvasDocument['timeline'] } : {}),
      ...(project.alloCreative ? { alloCreative: project.alloCreative } : {}),
    };
    const url = `${getBaseUrl()}/api/video-canvas/projects/${encodeURIComponent(projectId)}/doc`;
    void fetch(url, {
      method: 'PUT',
      keepalive: true,
      headers: { 'Content-Type': 'application/json', ...buildBackendAuthHeaders('PUT') },
      body: JSON.stringify(doc),
    }).catch(() => {
      // 卸载兜底：失败时静默，常规 flush 已尽力。
    });
  } catch {
    // 序列化/构造失败不影响常规 flush。
  }
}
