/**
 * User-data sync stub for allo canvas.
 * Remote cloud sync is disabled; persistence is via allo ocBridge.
 */

import { useAssetStore } from '@oc/stores/use-asset-store';
import type { CanvasProject } from '@oc/stores/canvas/use-canvas-store';
import { useCanvasStore } from '@oc/stores/canvas/use-canvas-store';

export async function syncRemoteUserData(_userId?: string | null) {
  // no-op: remote asset/canvas sync disabled
}

export function installRemoteUserDataAutoSync() {
  // no-op
}

export function resetRemoteUserDataSync() {
  // no-op
}

export function scheduleRemoteUserDataSync() {
  // no-op
}

export async function createCanvasProjectWithRemoteSync(
  title: string,
  projectId?: string,
  initialContent?: Partial<Pick<CanvasProject, 'nodes' | 'connections'>>
) {
  const id = useCanvasStore.getState().createProject(title, projectId);
  if (initialContent) useCanvasStore.getState().updateProject(id, initialContent);
  return { id, syncError: undefined as Error | undefined };
}

export async function deleteAssetWithRemoteSync(id: string) {
  useAssetStore.getState().removeAsset(id);
}

export async function saveRemoteUserDataNow() {
  // no-op: persistence handled by allo ocBridge
}
