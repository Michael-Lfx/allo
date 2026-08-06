/**
 * Remote user-data API stub. Persistence goes through allo ocBridge.
 */

import type { Asset } from '@oc/stores/use-asset-store';
import type { CanvasProject } from '@oc/stores/canvas/use-canvas-store';

export type RemoteUserDataSummary = {
  id: string;
  kind?: string;
  title: string;
  createdAt: string;
  updatedAt: string;
};

export function listRemoteAssets() {
  return Promise.resolve({ assets: [] as RemoteUserDataSummary[] });
}

export function getRemoteAsset(_id: string) {
  return Promise.reject(new Error('Remote assets are disabled; use allo ocBridge'));
}

export function upsertRemoteAsset(asset: Asset) {
  return Promise.resolve({
    asset: {
      id: asset.id,
      kind: asset.kind,
      title: asset.title,
      createdAt: asset.createdAt,
      updatedAt: asset.updatedAt,
    } as RemoteUserDataSummary,
  });
}

export function deleteRemoteAsset(id: string) {
  return Promise.resolve({ id });
}

export function listRemoteCanvasProjects() {
  return Promise.resolve({ projects: [] as RemoteUserDataSummary[] });
}

export function getRemoteCanvasProject(_id: string) {
  return Promise.reject(new Error('Remote canvas projects are disabled; use allo ocBridge'));
}

export function upsertRemoteCanvasProject(project: CanvasProject) {
  return Promise.resolve({
    project: {
      id: project.id,
      title: project.title,
      createdAt: project.createdAt,
      updatedAt: project.updatedAt,
    } as RemoteUserDataSummary,
  });
}

export function deleteRemoteCanvasProject(id: string) {
  return Promise.resolve({ id });
}
