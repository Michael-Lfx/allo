import type { CanvasResourceReference } from '@oc/lib/canvas/canvas-resource-references';
import { encodeChannelModel } from '@oc/stores/use-config-store';
import { CanvasNodeType, type CanvasNodeData } from '@oc/types/canvas';
import type { ShotPacketView, ShotRefSlot } from '../types';
import { audioNodeId, imageNodeId } from '../shotMiniCanvas/layout';

const ALLO_MEDIA_CHANNEL_ID = 'allo-media';
const SEEDANCE_AT_TOKEN = /@(Image|image|IMAGE|Audio|audio|AUDIO|图片|音频)\s*(\d+)/g;
const NODE_MENTION_TOKEN = /@\[node:(image|audio)-(\d+)\]/g;

function boundSlots(slots: ShotRefSlot[] | undefined): ShotRefSlot[] {
  return (slots ?? []).filter((slot) => Boolean(slot.path && !slot.unbound));
}

export function shotNodeModelValue(raw?: string | null): string | undefined {
  const model = raw?.trim();
  if (!model) return undefined;
  return model.includes('::') ? model : encodeChannelModel(ALLO_MEDIA_CHANNEL_ID, model);
}

/** Seedance `@Image1` / `@Audio1` → OpenCanvas `@[node:image-N]` chips. */
export function seedancePromptToNodeMentions(prompt: string, packet: ShotPacketView): string {
  const images = boundSlots(packet.image_refs);
  const audios = boundSlots(packet.audio_refs);
  return prompt.replace(SEEDANCE_AT_TOKEN, (full, kind: string, num: string) => {
    const index = Number(num) - 1;
    if (!Number.isInteger(index) || index < 0) return full;
    const audio = kind === '音频' || kind.toLowerCase() === 'audio';
    const slot = (audio ? audios : images)[index];
    if (!slot) return full;
    const nodeId = audio ? audioNodeId(slot.slot) : imageNodeId(slot.slot);
    return `@[node:${nodeId}]`;
  });
}

/** Bound image/audio refs as active OC chips even before blob URLs hydrate. */
export function shotBoundMentionReferences(nodes: CanvasNodeData[]): CanvasResourceReference[] {
  let imageIndex = 0;
  let audioIndex = 0;
  const refs: CanvasResourceReference[] = [];
  for (const node of nodes) {
    const isImage = node.type === CanvasNodeType.Image;
    const isAudio = node.type === CanvasNodeType.Audio;
    if (!isImage && !isAudio) continue;
    const vimax = node.metadata?.alloVimax as { path?: string | null; unbound?: boolean } | undefined;
    const bound = Boolean((vimax?.path || node.metadata?.storageKey || node.metadata?.content) && !vimax?.unbound);
    if (!bound) continue;
    const kind = isAudio ? 'audio' : 'image';
    const index = isAudio ? audioIndex++ : imageIndex++;
    const label = isAudio ? `音频${index + 1}` : `图片${index + 1}`;
    refs.push({
      id: node.id,
      nodeId: node.id,
      kind,
      label,
      title: node.title || label,
      previewUrl: node.metadata?.previewContent || node.metadata?.content,
      storageKey: node.metadata?.storageKey,
      active: true,
      sourceType: node.type,
    });
  }
  return refs;
}

/** Persist / pipeline still speak Seedance `@ImageN` / `@AudioN`. */
export function nodeMentionsToSeedancePrompt(prompt: string, packet: ShotPacketView): string {
  const images = boundSlots(packet.image_refs);
  const audios = boundSlots(packet.audio_refs);
  const imageOrdinal = new Map(images.map((slot, index) => [imageNodeId(slot.slot), index + 1]));
  const audioOrdinal = new Map(audios.map((slot, index) => [audioNodeId(slot.slot), index + 1]));
  return prompt.replace(NODE_MENTION_TOKEN, (full, kind: string, slotStr: string) => {
    const nodeId = `${kind}-${slotStr}`;
    if (kind === 'audio') {
      const ordinal = audioOrdinal.get(nodeId);
      return ordinal != null ? `@Audio${ordinal}` : full;
    }
    const ordinal = imageOrdinal.get(nodeId);
    return ordinal != null ? `@Image${ordinal}` : full;
  });
}
