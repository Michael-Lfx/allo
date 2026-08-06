/**
 * Canvas share API stub (disabled in allo canvas).
 */

import type { CanvasProject } from '@oc/stores/canvas/use-canvas-store';

export type CanvasShareStatus = {
  enabled: boolean;
  token?: string;
  expiresAt?: string;
  createdAt?: string;
};

export type PublicCanvasShare = {
  project: CanvasProject;
  expiresAt?: string;
};

export function getCanvasShare(_projectId: string): Promise<{ share: CanvasShareStatus }> {
  return Promise.resolve({ share: { enabled: false } });
}

export function createCanvasShare(
  _projectId: string,
  _params: { expiresDays: number; rotate?: boolean }
): Promise<{ share: CanvasShareStatus }> {
  return Promise.reject(new Error('Canvas share is disabled in allo canvas'));
}

export function deleteCanvasShare(projectId: string): Promise<{ id: string }> {
  return Promise.resolve({ id: projectId });
}

export function getPublicCanvasShare(_token: string): Promise<PublicCanvasShare> {
  return Promise.reject(new Error('Public canvas share is disabled in allo canvas'));
}
