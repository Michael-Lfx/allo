

import type { ArtifactNode } from './types';

export interface StoryboardShot {
  index: number;
  visualDescription: string;
  audioDescription?: string;
}

export interface StoryboardScene {
  id: string;
  index: number;
  visualDescription: string;
  audioDescription?: string;
  imagePath?: string;
  videoPath?: string;
  revisionPath?: string;
}

export function flattenArtifacts(nodes: ArtifactNode[]): ArtifactNode[] {
  const flattened: ArtifactNode[] = [];
  for (const node of nodes) {
    if (node.is_dir) {
      flattened.push(...flattenArtifacts(node.children ?? []));
    } else {
      flattened.push(node);
    }
  }
  return flattened;
}

export function findStoryboardPath(nodes: ArtifactNode[]): string | undefined {
  const files = flattenArtifacts(nodes);
  return (
    files.find((file) => /\/storyboard\.json$/i.test(file.path))?.path ??
    files.find((file) => /storyboard.*\.json$/i.test(file.path))?.path
  );
}

export function parseStoryboard(text: string | undefined): StoryboardShot[] {
  if (!text) return [];
  try {
    const parsed = JSON.parse(text) as unknown;
    const rows = storyboardRows(parsed);
    return rows.flatMap((row, fallbackIndex) => {
      if (!row || typeof row !== 'object') return [];
      const value = row as Record<string, unknown>;
      const visual =
        stringValue(value.visual_desc) ??
        stringValue(value.visualDescription) ??
        stringValue(value.description) ??
        stringValue(value.prompt);
      if (!visual) return [];
      const rawIndex = value.idx ?? value.index ?? value.shot_index;
      return [
        {
          index: typeof rawIndex === 'number' ? rawIndex : fallbackIndex,
          visualDescription: visual,
          audioDescription:
            stringValue(value.audio_desc) ??
            stringValue(value.audioDescription) ??
            stringValue(value.audio),
        },
      ];
    });
  } catch {
    return [];
  }
}

export function buildStoryboardScenes(
  nodes: ArtifactNode[],
  shots: StoryboardShot[],
  storyboardPath?: string
): StoryboardScene[] {
  const files = flattenArtifacts(nodes);
  const imageFiles = files.filter((file) => /\.(png|jpe?g|webp)$/i.test(file.path));
  const videoFiles = files.filter((file) => /\/video\.(mp4|webm|mov)$/i.test(file.path));
  const revisionFiles = files.filter((file) => /shot_description\.json$/i.test(file.path));

  if (shots.length > 0) {
    return shots.map((shot) => ({
      id: `shot-${shot.index}`,
      index: shot.index,
      visualDescription: shot.visualDescription,
      audioDescription: shot.audioDescription,
      imagePath: bestShotFile(imageFiles, shot.index, 'first_frame'),
      videoPath: bestShotFile(videoFiles, shot.index),
      revisionPath:
        bestShotFile(revisionFiles, shot.index) ??
        storyboardPath,
    }));
  }

  const indices = new Set<number>();
  for (const file of [...imageFiles, ...videoFiles]) {
    const index = shotIndexFromPath(file.path);
    if (index != null) indices.add(index);
  }
  return [...indices]
    .sort((a, b) => a - b)
    .map((index) => ({
      id: `shot-${index}`,
      index,
      visualDescription: '',
      imagePath: bestShotFile(imageFiles, index, 'first_frame'),
      videoPath: bestShotFile(videoFiles, index),
      revisionPath: bestShotFile(revisionFiles, index) ?? storyboardPath,
    }));
}

function storyboardRows(value: unknown): unknown[] {
  if (Array.isArray(value)) return value;
  if (!value || typeof value !== 'object') return [];
  const record = value as Record<string, unknown>;
  if (Array.isArray(record.storyboard)) return record.storyboard;
  if (Array.isArray(record.shots)) return record.shots;
  if (Array.isArray(record.shot_descriptions)) return record.shot_descriptions;
  return [];
}

function stringValue(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

function shotIndexFromPath(path: string): number | null {
  const match = path.replace(/\\/g, '/').match(/\/shots\/(\d+)\//i);
  return match ? Number(match[1]) : null;
}

function bestShotFile(files: ArtifactNode[], index: number, preferredName?: string): string | undefined {
  const matches = files.filter((file) => shotIndexFromPath(file.path) === index);
  return (
    matches.find((file) => preferredName && file.path.includes(preferredName))?.path ??
    matches[0]?.path
  );
}
