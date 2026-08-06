/**
 * Bridge allo `/api/video-canvas` projects into the ported open-ai-canvas Zustand store.
 */

import { getCanvasProject, putCanvasDoc, listCanvasProjects, createCanvasProject } from '../api';
import type { CanvasDocument } from '../types';
import {
  useCanvasStore,
  type CanvasProject,
  flushCanvasStorePersistence,
} from '@oc/stores/canvas/use-canvas-store';
import type { CanvasNodeData, CanvasConnection, ViewportTransform } from '@oc/types/canvas';

function docToProject(projectId: string, title: string, doc: CanvasDocument): CanvasProject {
  const now = new Date().toISOString();
  return {
    id: projectId,
    title: doc.title || title || '未命名画布',
    createdAt: now,
    updatedAt: now,
    nodes: (doc.nodes || []) as unknown as CanvasNodeData[],
    connections: (doc.connections || []) as unknown as CanvasConnection[],
    chatSessions: [],
    activeChatId: null,
    backgroundMode: (doc.backgroundMode as CanvasProject['backgroundMode']) || 'dots',
    showImageInfo: false,
    viewport: (doc.viewport || { x: 0, y: 0, k: 1 }) as ViewportTransform,
    directorScenes: [],
  };
}

export async function hydrateCanvasProjectFromServer(projectId: string): Promise<CanvasProject> {
  const { meta, doc } = await getCanvasProject(projectId);
  const project = docToProject(projectId, meta.title, doc as CanvasDocument);
  const store = useCanvasStore.getState();
  const existing = store.projects.filter((p) => p.id !== projectId);
  store.replaceProjects([...existing, project]);
  return project;
}

export async function syncCanvasProjectToServer(projectId: string): Promise<void> {
  const project = useCanvasStore.getState().projects.find((p) => p.id === projectId);
  if (!project) return;
  await flushCanvasStorePersistence();
  const doc: CanvasDocument = {
    schema: 1,
    title: project.title,
    nodes: project.nodes as unknown as CanvasDocument['nodes'],
    connections: project.connections as unknown as CanvasDocument['connections'],
    viewport: project.viewport as CanvasDocument['viewport'],
    backgroundMode: (project.backgroundMode as CanvasDocument['backgroundMode']) || 'dots',
  };
  await putCanvasDoc(projectId, doc);
}

export async function ensureServerProjectsInStore(): Promise<void> {
  const list = await listCanvasProjects();
  const store = useCanvasStore.getState();
  const byId = new Map(store.projects.map((p) => [p.id, p]));
  for (const meta of list) {
    if (!byId.has(meta.project_id)) {
      byId.set(meta.project_id, {
        id: meta.project_id,
        title: meta.title,
        createdAt: new Date(meta.created_at).toISOString(),
        updatedAt: new Date(meta.updated_at).toISOString(),
        nodes: [],
        connections: [],
        chatSessions: [],
        activeChatId: null,
        backgroundMode: 'lines',
        showImageInfo: false,
        viewport: { x: 0, y: 0, k: 1 },
        directorScenes: [],
      });
    }
  }
  store.replaceProjects([...byId.values()]);
}

export async function createServerBackedCanvasProject(title?: string): Promise<string> {
  const meta = await createCanvasProject(title);
  await hydrateCanvasProjectFromServer(meta.project_id);
  return meta.project_id;
}
