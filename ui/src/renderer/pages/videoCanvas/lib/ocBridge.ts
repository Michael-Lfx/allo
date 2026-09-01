/**
 * Bridge allo `/api/video-canvas` projects into the ported open-ai-canvas Zustand store.
 */

import {
  canvasMediaUrl,
  deleteCanvasProject,
  getCanvasProject,
  putCanvasDoc,
  listCanvasProjects,
  createCanvasProject,
  type CanvasMediaMeta,
} from '../api';
import type { CanvasDocument } from '../types';
import {
  useCanvasStore,
  type CanvasProject,
  flushCanvasStorePersistence,
} from '@oc/stores/canvas/use-canvas-store';
import type { CanvasNodeData, CanvasConnection, ViewportTransform } from '@oc/types/canvas';
import { CanvasNodeType } from '@oc/types/canvas';
import { createCanvasNode } from '@oc/lib/canvas/canvas-project-domain';
import { encodeChannelModel } from '@oc/stores/use-config-store';
import { canonicalizeVideoResolution } from '@oc/lib/canvas-video-resolution';
import { isMiniMaxH3VideoModel } from '@renderer/services/videoModelCapabilities';
import { isMiniMaxH3ResolutionToken } from '@oc/lib/video-generation-options';
import { parsePersistedChatSessions, projectToCanvasDocument } from './canvasChatPersist';

export type CanvasHomeLaunch = {
  prompt: string;
  requirement?: string;
  mediaKind: 'image' | 'video';
  /**
   * `generate` = ordinary T2V/I2V from home (no style skill, may auto-start).
   * `creation` (default) = open canvas Agent with the homepage prompt as the first turn.
   */
  intent?: 'creation' | 'generate';
  /** When true, canvas opens and fires one video generation on the config node. */
  autoGenerate?: boolean;
  /** When true, canvas opens Agent and sends the homepage prompt as the first chat turn. */
  autoAgent?: boolean;
  skill?: {
    id: string;
    label: string;
    description: string;
    stylePrompt: string;
  };
  preferences: {
    automatic: boolean;
    aspectRatio: string;
    resolution: string;
    fps: number;
    targetDurationSecs: number;
    imageModel?: string;
    videoModel?: string;
  };
  references?: CanvasMediaMeta[];
};

function connection(fromNodeId: string, toNodeId: string): CanvasConnection {
  return {
    id: `connection-${fromNodeId}-${toNodeId}`,
    fromNodeId,
    toNodeId,
  };
}

function initializeProjectFromHome(project: CanvasProject, launch: CanvasHomeLaunch): CanvasProject {
  const intent = launch.intent ?? 'creation';
  const isGenerate = intent === 'generate';
  const autoAgent = isCanvasHomeAgentLaunch({ intent, autoAgent: launch.autoAgent });
  const skill = launch.skill;
  const referenceNodes = (launch.references ?? []).map((reference, index) => ({
    ...createCanvasNode(
      CanvasNodeType.Image,
      { x: 600, y: 150 + index * 210 },
      {
        content: canvasMediaUrl(reference.media_id),
        status: 'success',
        mimeType: reference.mime,
        bytes: reference.bytes,
        naturalWidth: reference.width ?? undefined,
        naturalHeight: reference.height ?? undefined,
        assetId: reference.media_id,
        workflowKind: 'reference_set',
      }
    ),
    title: reference.title || `参考图 ${index + 1}`,
  }));
  const seeded = autoAgent ? { nodes: referenceNodes, connections: [] as CanvasConnection[] } : seedLegacyHomeGraph(launch, isGenerate, skill, referenceNodes);
  return {
    ...project,
    nodes: seeded.nodes,
    connections: seeded.connections,
    viewport: autoAgent
      ? { x: 80, y: 60, k: 1 }
      : { x: 80, y: 60, k: referenceNodes.length > 2 ? 0.72 : 0.86 },
    alloCreative: {
      ...(project.alloCreative ?? {}),
      homeLaunch: {
        schema: 1,
        intent,
        autoGenerate: Boolean(launch.autoGenerate),
        autoAgent,
        agentBriefSent: false,
        prompt: launch.prompt,
        requirement: launch.requirement,
        mediaKind: launch.mediaKind,
        skill,
        preferences: launch.preferences,
        referenceMediaIds: (launch.references ?? []).map((item) => item.media_id),
        createdAt: new Date().toISOString(),
      },
    },
  };
}

function seedLegacyHomeGraph(
  launch: CanvasHomeLaunch,
  isGenerate: boolean,
  skill: CanvasHomeLaunch['skill'],
  referenceNodes: CanvasNodeData[],
) {
  const promptNode = {
    ...createCanvasNode(CanvasNodeType.Text, { x: 220, y: 190 }, {
      content: launch.prompt,
      prompt: launch.prompt,
      status: 'success',
      workflowKind: 'story_input',
      workflowTitle: isGenerate ? '视频提示' : '创作输入',
      workflowDescription: launch.requirement,
    }),
    title: isGenerate ? '生成提示' : '创作提示',
  };
  const skillNode =
    !isGenerate && skill
      ? {
          ...createCanvasNode(CanvasNodeType.Skill, { x: 220, y: 500 }, {
            content: skill.stylePrompt,
            prompt: skill.stylePrompt,
            status: 'success',
            skillId: skill.id,
            skillVersion: 1,
            stylePresetId: skill.id,
            skillSnapshot: {
              id: skill.id,
              name: skill.label,
              description: skill.description,
              category: launch.mediaKind,
              template: skill.stylePrompt,
              outputMode: launch.mediaKind === 'image' ? 'image_prompt' : 'workflow',
              outputContract: 'Apply the selected visual style to the connected generation node.',
              version: 1,
              tags: ['video-home', 'style'],
            },
          }),
          title: skill.label,
        }
      : null;
  const selectedModel =
    launch.mediaKind === 'image'
      ? launch.preferences.imageModel
      : launch.preferences.videoModel;
  const videoModel = selectedModel || '';
  const canonicalVquality = canonicalizeVideoResolution(
    videoModel,
    launch.preferences.resolution,
  );
  const storedVquality =
    isMiniMaxH3VideoModel(videoModel) || isMiniMaxH3ResolutionToken(canonicalVquality)
      ? canonicalVquality
      : String(canonicalVquality).replace(/p$/i, '');
  const composedPrompt = [
    launch.prompt,
    !isGenerate ? skill?.stylePrompt : undefined,
    launch.requirement,
  ]
    .filter(Boolean)
    .join('\n\n');
  const configNode = {
    ...createCanvasNode(CanvasNodeType.Config, { x: 1050, y: 310 }, {
      content: launch.requirement ?? '',
      composerContent: launch.prompt,
      prompt: composedPrompt,
      status: 'idle',
      generationMode: launch.mediaKind,
      videoEditOperation: isGenerate
        ? (launch.references?.length ? 'image_to_video' : 'text_to_video')
        : undefined,
      model: selectedModel
        ? encodeChannelModel('allo-media', selectedModel)
        : undefined,
      size: launch.preferences.aspectRatio,
      seconds: String(launch.preferences.targetDurationSecs),
      vquality: storedVquality,
      workflowKind: isGenerate ? 'shot' : 'styleboard',
      workflowTitle: isGenerate
        ? '视频生成'
        : `${skill?.label ?? '创作'}创作`,
      workflowDescription: launch.requirement,
      ...(skill && !isGenerate
        ? { stylePresetId: skill.id, skillId: skill.id }
        : {}),
    }),
    title: launch.mediaKind === 'image' ? '图片生成配置' : '视频生成配置',
  };
  return {
    nodes: [
      promptNode,
      ...(skillNode ? [skillNode] : []),
      ...referenceNodes,
      configNode,
    ],
    connections: [
      connection(promptNode.id, configNode.id),
      ...(skillNode ? [connection(skillNode.id, configNode.id)] : []),
      ...referenceNodes.map((node) => connection(node.id, configNode.id)),
    ],
  };
}

function docToProject(projectId: string, title: string, doc: CanvasDocument, existing?: CanvasProject): CanvasProject {
  const now = new Date().toISOString();
  const sessions = parsePersistedChatSessions(doc.chatSessions);
  const chatSessions = sessions.length ? sessions : existing?.chatSessions || [];
  const activeChatId = typeof doc.activeChatId === 'string' && doc.activeChatId
    ? doc.activeChatId
    : existing?.activeChatId || chatSessions[0]?.id || null;
  return {
    id: projectId,
    title: doc.title || title || '未命名画布',
    createdAt: now,
    updatedAt: now,
    nodes: (doc.nodes || []) as unknown as CanvasNodeData[],
    connections: (doc.connections || []) as unknown as CanvasConnection[],
    chatSessions,
    activeChatId,
    backgroundMode: (doc.backgroundMode as CanvasProject['backgroundMode']) || 'dots',
    showImageInfo: false,
    viewport: (doc.viewport || { x: 0, y: 0, k: 1 }) as ViewportTransform,
    directorScenes: [],
    timeline: doc.timeline as CanvasProject['timeline'],
    alloCreative: doc.alloCreative,
  };
}

export { projectToCanvasDocument };

export async function hydrateCanvasProjectFromServer(
  projectId: string,
  prefetched?: Awaited<ReturnType<typeof getCanvasProject>>
): Promise<CanvasProject> {
  const { meta, doc } = prefetched ?? (await getCanvasProject(projectId));
  const store = useCanvasStore.getState();
  const existingProject = store.projects.find((p) => p.id === projectId);
  const project = docToProject(projectId, meta.title, doc as CanvasDocument, existingProject);
  const existing = store.projects.filter((p) => p.id !== projectId);
  store.replaceProjects([...existing, project]);
  return project;
}

export async function syncCanvasProjectToServer(projectId: string): Promise<void> {
  const project = useCanvasStore.getState().projects.find((p) => p.id === projectId);
  if (!project) return;
  await flushCanvasStorePersistence();
  await putCanvasDoc(projectId, projectToCanvasDocument(project));
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

export async function createServerBackedCanvasProject(
  title?: string,
  launch?: CanvasHomeLaunch
): Promise<string> {
  const meta = await createCanvasProject(title);
  try {
    const project = await hydrateCanvasProjectFromServer(meta.project_id);
    if (launch) {
      const initialized = initializeProjectFromHome(project, launch);
      const store = useCanvasStore.getState();
      store.replaceProjects([
        ...store.projects.filter((item) => item.id !== initialized.id),
        initialized,
      ]);
      await syncCanvasProjectToServer(meta.project_id);
    }
    return meta.project_id;
  } catch (error) {
    useCanvasStore
      .getState()
      .replaceProjects(
        useCanvasStore.getState().projects.filter((item) => item.id !== meta.project_id)
      );
    await deleteCanvasProject(meta.project_id).catch(() => undefined);
    throw error;
  }
}
