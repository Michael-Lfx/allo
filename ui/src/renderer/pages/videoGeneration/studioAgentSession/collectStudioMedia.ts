import { flattenArtifacts } from '../artifactPresentation';
import type { ArtifactNode } from '../types';
import type { StudioSessionMedia } from './types';

const IMAGE_RE = /\.(png|jpe?g|webp|gif)$/i;
const VIDEO_RE = /\.(mp4|webm|mov)$/i;
const SKIP_IMAGE_RE = /_raw|_generation_prompt|_atmosphere/i;

function normalize(path: string): string {
  return path.replace(/\\/g, '/');
}

function shotSceneId(path: string): string | undefined {
  const match = normalize(path).match(/^(.*)\/shots\/(\d+)\//i);
  if (!match) return undefined;
  const root = match[1] ?? '';
  const index = Number(match[2]);
  return root ? `${root}/shot-${index}` : `shot-${index}`;
}

function fileLabel(path: string): string {
  const base = normalize(path).split('/').pop() ?? path;
  return base.replace(/\.[^.]+$/, '').replace(/_/g, ' ');
}

function isImage(path: string): boolean {
  return IMAGE_RE.test(path) && !SKIP_IMAGE_RE.test(path);
}

function isVideo(path: string): boolean {
  return VIDEO_RE.test(path);
}

export function collectPortraitMedia(nodes: ArtifactNode[]): StudioSessionMedia[] {
  return flattenArtifacts(nodes)
    .filter((file) => {
      const path = normalize(file.path);
      return /(^|\/)character_portraits\//i.test(path) && isImage(path);
    })
    .map((file) => ({
      id: `portrait:${file.path}`,
      kind: 'image' as const,
      path: file.path,
      label: fileLabel(file.path),
    }));
}

export function collectWorldMedia(nodes: ArtifactNode[]): StudioSessionMedia[] {
  return flattenArtifacts(nodes)
    .filter((file) => {
      const path = normalize(file.path);
      if (!isImage(path)) return false;
      return (
        /(^|\/)look_plate\.png$/i.test(path) ||
        /\/environments\//i.test(path) ||
        /\/props\//i.test(path) ||
        /\/world_assets\//i.test(path)
      );
    })
    .map((file) => ({
      id: `world:${file.path}`,
      kind: 'image' as const,
      path: file.path,
      label: fileLabel(file.path),
    }));
}

export function collectShotFrameMedia(nodes: ArtifactNode[]): StudioSessionMedia[] {
  return flattenArtifacts(nodes)
    .filter((file) => {
      const path = normalize(file.path);
      return /\/shots\/\d+\//i.test(path) && isImage(path);
    })
    .map((file) => ({
      id: `frame:${file.path}`,
      kind: 'image' as const,
      path: file.path,
      label: fileLabel(file.path),
      sceneId: shotSceneId(file.path),
    }));
}

export function collectShotVideoMedia(nodes: ArtifactNode[]): StudioSessionMedia[] {
  return flattenArtifacts(nodes)
    .filter((file) => {
      const path = normalize(file.path);
      return /\/shots\/\d+\//i.test(path) && isVideo(path);
    })
    .map((file) => ({
      id: `clip:${file.path}`,
      kind: 'video' as const,
      path: file.path,
      label: fileLabel(file.path),
      sceneId: shotSceneId(file.path),
    }));
}

export function collectFilmMedia(
  finalVideoPath?: string | null,
  coverPath?: string | null
): StudioSessionMedia[] {
  const out: StudioSessionMedia[] = [];
  if (coverPath) {
    out.push({
      id: `cover:${coverPath}`,
      kind: 'image',
      path: coverPath,
      label: 'cover',
    });
  }
  if (finalVideoPath) {
    out.push({
      id: `film:${finalVideoPath}`,
      kind: 'video',
      path: finalVideoPath,
      label: 'film',
    });
  }
  return out;
}
