/**
 * Build generation context from upstream connected nodes (open-ai-canvas style).
 */

import type { CanvasConnection, CanvasNodeData } from '../types';

export type GenerationContext = {
  promptParts: string[];
  referenceImageNodeIds: string[];
  referenceVideoNodeIds: string[];
  referenceAudioNodeIds: string[];
  referenceMediaIds: string[];
};

export function upstreamNodeIds(
  nodeId: string,
  connections: CanvasConnection[]
): string[] {
  return connections.filter((c) => c.toNodeId === nodeId).map((c) => c.fromNodeId);
}

export function buildGenerationContext(
  targetNodeId: string,
  nodes: CanvasNodeData[],
  connections: CanvasConnection[]
): GenerationContext {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const upstream = upstreamNodeIds(targetNodeId, connections);
  const promptParts: string[] = [];
  const referenceImageNodeIds: string[] = [];
  const referenceVideoNodeIds: string[] = [];
  const referenceAudioNodeIds: string[] = [];
  const referenceMediaIds: string[] = [];

  for (const id of upstream) {
    const node = byId.get(id);
    if (!node) continue;
    const meta = node.metadata ?? {};
    if (node.type === 'text' && meta.content?.trim()) {
      promptParts.push(meta.content.trim());
    }
    if (meta.prompt?.trim()) {
      promptParts.push(meta.prompt.trim());
    }
    if (node.type === 'image' && meta.mediaId) {
      referenceImageNodeIds.push(node.id);
      referenceMediaIds.push(meta.mediaId);
    }
    if (node.type === 'video' && meta.mediaId) {
      referenceVideoNodeIds.push(node.id);
      referenceMediaIds.push(meta.mediaId);
    }
    if (node.type === 'audio' && meta.mediaId) {
      referenceAudioNodeIds.push(node.id);
      referenceMediaIds.push(meta.mediaId);
    }
    // Config / script can contribute composed prompts.
    if (node.type === 'config' && meta.prompt?.trim()) {
      promptParts.push(meta.prompt.trim());
    }
    if (node.type === 'script' && meta.storyboard) {
      for (const row of meta.storyboard.rows) {
        if (row.plotDescription.trim()) promptParts.push(row.plotDescription.trim());
        if (row.imageGenerationPrompt.trim()) promptParts.push(row.imageGenerationPrompt.trim());
      }
    }
  }

  return {
    promptParts,
    referenceImageNodeIds,
    referenceVideoNodeIds,
    referenceAudioNodeIds,
    referenceMediaIds,
  };
}

export function composePrompt(
  nodePrompt: string | undefined,
  ctx: GenerationContext
): string {
  const parts = [...ctx.promptParts];
  if (nodePrompt?.trim()) parts.push(nodePrompt.trim());
  return parts.join('\n\n').trim();
}
