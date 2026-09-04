import {
  flattenArtifacts,
  isAudioArtifactPath,
  isImageArtifactPath,
  isVideoArtifactPath,
} from '../artifactPresentation';
import { loadArtifactMediaUrlCached, loadCameoPreviewUrl } from '../api';
import type { ArtifactNode, CameoPhoto } from '../types';
import type { StudioDocumentRole, StudioSessionMedia } from './types';

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

function isPortraitImage(path: string): boolean {
  return isImageArtifactPath(path) && !SKIP_IMAGE_RE.test(path);
}

function isSceneScoped(path: string): boolean {
  return /(?:^|\/)(?:scene|event)_\d+\//i.test(path) || /\/shots\//i.test(path);
}

function pickShallowest(paths: string[]): string | undefined {
  if (paths.length === 0) return undefined;
  return [...paths].sort(
    (a, b) => a.split('/').length - b.split('/').length || a.localeCompare(b)
  )[0];
}

function pathsMatching(files: string[], basename: string): string[] {
  const needle = basename.toLowerCase();
  return files.filter((path) => {
    const lower = path.toLowerCase();
    return lower === needle || lower.endsWith(`/${needle}`);
  });
}

export function collectPortraitMedia(nodes: ArtifactNode[]): StudioSessionMedia[] {
  return flattenArtifacts(nodes)
    .filter((file) => {
      const path = normalize(file.path);
      if (!/(^|\/)character_portraits\//i.test(path)) return false;
      return isPortraitImage(path) || isAudioArtifactPath(path);
    })
    .map((file) => {
      const path = normalize(file.path);
      const audio = isAudioArtifactPath(path);
      return {
        id: `${audio ? 'voice' : 'portrait'}:${file.path}`,
        kind: (audio ? 'audio' : 'image') as StudioSessionMedia['kind'],
        path: file.path,
        label: fileLabel(file.path),
      };
    })
    .sort(compareCharacterAssets);
}

function compareCharacterAssets(a: StudioSessionMedia, b: StudioSessionMedia): number {
  const dir = characterAssetDir(a.path).localeCompare(characterAssetDir(b.path));
  if (dir !== 0) return dir;
  const rank = (kind: StudioSessionMedia['kind']) =>
    kind === 'image' ? 0 : kind === 'audio' ? 1 : 2;
  const byKind = rank(a.kind) - rank(b.kind);
  return byKind !== 0 ? byKind : a.path.localeCompare(b.path);
}

/** Directory that owns one character's look + voice clips. */
export function characterAssetDir(path: string): string {
  const normalized = normalize(path);
  const match = normalized.match(/^((?:.*\/)?character_portraits\/[^/]+)/i);
  return match?.[1] ?? normalized.replace(/\/[^/]+$/, '');
}

export function characterGroupLabel(path: string): string {
  const base = characterAssetDir(path).split('/').pop() ?? path;
  return base.replace(/^\d+_/, '').replace(/_/g, ' ');
}

export interface PortraitMediaGroup {
  dir: string;
  label: string;
  images: StudioSessionMedia[];
  audios: StudioSessionMedia[];
}

export function groupPortraitMedia(items: StudioSessionMedia[]): PortraitMediaGroup[] {
  const groups = new Map<string, StudioSessionMedia[]>();
  for (const item of items) {
    const dir = characterAssetDir(item.path);
    const list = groups.get(dir);
    if (list) list.push(item);
    else groups.set(dir, [item]);
  }
  return [...groups.entries()].map(([dir, cards]) => ({
    dir,
    label: characterGroupLabel(dir),
    images: cards.filter((card) => card.kind === 'image'),
    audios: cards.filter((card) => card.kind === 'audio'),
  }));
}

export function parseCastEntries(text: string): Array<{ name: string; features: string }> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return [];
  }
  const rows = Array.isArray(parsed)
    ? parsed
    : parsed &&
        typeof parsed === 'object' &&
        Array.isArray((parsed as { characters?: unknown }).characters)
      ? ((parsed as { characters: unknown[] }).characters ?? [])
      : [];
  const out: Array<{ name: string; features: string }> = [];
  for (const row of rows) {
    if (!row || typeof row !== 'object') continue;
    const rec = row as Record<string, unknown>;
    const rawName =
      typeof rec.identifier_in_scene === 'string'
        ? rec.identifier_in_scene.trim()
        : typeof rec.name === 'string'
          ? rec.name.trim()
          : '';
    if (!rawName) continue;
    out.push({
      name: rawName,
      features: typeof rec.static_features === 'string' ? rec.static_features.trim() : '',
    });
  }
  return out;
}

export function collectWorldMedia(nodes: ArtifactNode[]): StudioSessionMedia[] {
  return flattenArtifacts(nodes)
    .filter((file) => {
      const path = normalize(file.path);
      if (!isImageArtifactPath(path) || SKIP_IMAGE_RE.test(path)) return false;
      // Vacant look plate is an internal img2img bible for env/prop plates, not a
      // user-facing world asset (and never a character reference).
      if (/(^|\/)look_plate\.png$/i.test(path)) return false;
      return (
        /(^|\/)environments\//i.test(path) ||
        /(^|\/)props\//i.test(path) ||
        /(^|\/)world_assets\//i.test(path)
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
      return /\/shots\/\d+\//i.test(path) && isPortraitImage(path);
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
      return /\/shots\/\d+\//i.test(path) && isVideoArtifactPath(path);
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

export function collectCameoMedia(photos: CameoPhoto[]): StudioSessionMedia[] {
  return photos.map((photo) => ({
    id: `cameo:${photo.id}`,
    kind: 'image' as const,
    path: photo.id,
    label: photo.character_name.trim() || photo.rel_path.split('/').pop() || photo.id,
    origin: 'cameo' as const,
  }));
}

export function collectSourceDocumentMedia(name?: string | null): StudioSessionMedia[] {
  const trimmed = name?.trim();
  if (!trimmed) return [];
  return [
    {
      id: `doc:${trimmed}`,
      kind: 'file',
      path: trimmed,
      label: trimmed,
    },
  ];
}

const DOCUMENT_LABEL: Record<StudioDocumentRole, string> = {
  story: 'story',
  script: 'script',
  cast: 'cast',
};

/**
 * Planning-stage documents the session should surface: story/synopsis, the
 * full script (all scenes), and the film-level cast list.
 */
export function collectStoryDocuments(nodes: ArtifactNode[]): StudioSessionMedia[] {
  const files = flattenArtifacts(nodes).map((file) => normalize(file.path));
  const out: StudioSessionMedia[] = [];
  const push = (path: string | undefined, role: StudioDocumentRole) => {
    if (!path) return;
    out.push({
      id: `doc:${role}:${path}`,
      kind: 'document',
      path,
      label: DOCUMENT_LABEL[role],
      role,
    });
  };

  push(pickShallowest(pathsMatching(files, 'story.txt')), 'story');
  pushScriptDocument(out, files);
  const casts = pathsMatching(files, 'characters.json');
  push(
    pickShallowest(casts.filter((path) => !isSceneScoped(path))) ?? pickShallowest(casts),
    'cast'
  );

  return out;
}

function sceneScriptIndex(path: string): number {
  const match = path.match(/(?:scene_|event_)(\d+)/i);
  return match ? Number(match[1]) : Number.POSITIVE_INFINITY;
}

function compareSceneScriptPaths(a: string, b: string): number {
  const byScene = sceneScriptIndex(a) - sceneScriptIndex(b);
  return byScene !== 0 ? byScene : a.localeCompare(b);
}

function pushScriptDocument(out: StudioSessionMedia[], files: string[]): void {
  const json =
    pickShallowest(pathsMatching(files, 'script.json').filter((path) => !isSceneScoped(path))) ??
    pickShallowest(pathsMatching(files, 'script.json'));
  if (json) {
    out.push({
      id: `doc:script:${json}`,
      kind: 'document',
      path: json,
      label: DOCUMENT_LABEL.script,
      role: 'script',
    });
    return;
  }

  const scripts = pathsMatching(files, 'script.txt');
  const root = pickShallowest(scripts.filter((path) => !isSceneScoped(path)));
  if (root) {
    out.push({
      id: `doc:script:${root}`,
      kind: 'document',
      path: root,
      label: DOCUMENT_LABEL.script,
      role: 'script',
    });
    return;
  }

  const scenes = scripts.filter(isSceneScoped).sort(compareSceneScriptPaths);
  const first = scenes[0];
  if (!first) return;
  out.push({
    id: `doc:script:${first}`,
    kind: 'document',
    path: first,
    label: DOCUMENT_LABEL.script,
    role: 'script',
    paths: scenes.length > 1 ? scenes : undefined,
  });
}

function jsonScriptRows(parsed: unknown): unknown[] {
  if (Array.isArray(parsed)) return parsed;
  if (!parsed || typeof parsed !== 'object') return [];
  const rec = parsed as Record<string, unknown>;
  for (const key of ['scenes', 'script', 'scripts'] as const) {
    const value = rec[key];
    if (Array.isArray(value)) return value;
  }
  return [];
}

function sceneBody(row: unknown): string | null {
  if (typeof row === 'string' && row.trim()) return row.trim();
  if (!row || typeof row !== 'object') return null;
  const rec = row as Record<string, unknown>;
  for (const key of ['script', 'text', 'body', 'content'] as const) {
    const value = rec[key];
    if (typeof value === 'string' && value.trim()) return value.trim();
  }
  return null;
}

/** Split a script artifact into scene bodies (JSON array or a single prose file). */
export function parseScriptScenes(text: string): string[] {
  const trimmed = text.trim();
  if (!trimmed) return [];
  if (trimmed.startsWith('[') || trimmed.startsWith('{')) {
    try {
      const scenes = jsonScriptRows(JSON.parse(trimmed) as unknown).flatMap((row) => {
        const body = sceneBody(row);
        return body ? [body] : [];
      });
      if (scenes.length > 0) return scenes;
    } catch {
      // Fall through to prose.
    }
  }
  return [trimmed];
}

export async function loadStudioMediaPreviewUrl(
  sessionId: string,
  item: StudioSessionMedia
): Promise<string> {
  if (item.kind === 'file' || item.kind === 'document') {
    throw new Error('file attachments have no preview url');
  }
  if (item.origin === 'cameo') {
    return loadCameoPreviewUrl(sessionId, item.path);
  }
  return loadArtifactMediaUrlCached(sessionId, item.path);
}
