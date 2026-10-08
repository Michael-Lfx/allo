import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Popconfirm } from '@arco-design/web-react';
import {
  approveShot,
  getArtifactImagePrompt,
  getShotPacket,
  putShotPacket,
  replaceShotRef,
  retakeShot,
  selectShotTake,
  updateArtifactImagePrompt,
  uploadShotRefFile,
} from '../api';
import {
  flattenArtifacts,
  isAudioArtifactPath,
  isImageArtifactPath,
  type StoryboardScene,
} from '../artifactPresentation';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import type {
  ArtifactNode,
  ShotGraphLayout,
  ShotPacketPatch,
  ShotPacketView,
  ShotRefRole,
  ShotRefSlot,
} from '../types';
import type { CanvasNodeData, CanvasNodeMetadata, Position } from '@oc/types/canvas';
import type { StoryboardVideoSlotStatus } from '../storyboardVideoStatus';
import ShotInfiniteCanvas from '../shotInfiniteCanvas/ShotInfiniteCanvas';
import { packetToCanvasGraph, parseShotNodeId, slotHasGenerationPrompt } from '../shotInfiniteCanvas/packetToCanvas';
import { nodeMentionsToSeedancePrompt } from '../shotInfiniteCanvas/shotMentions';
import { useShotCanvasMedia } from '../shotInfiniteCanvas/useShotCanvasMedia';
import {
  MAX_SHOT_AUDIO_REFS,
  MAX_SHOT_IMAGE_REFS,
  VIDEO_NODE_ID,
  audioNodeId,
  clampViewport,
  defaultNodePositions,
  fitViewport,
  imageNodeId,
  mergePositions,
  nextSlot,
  nodeSize,
  type MiniPoint,
  type MiniViewport,
} from '../shotMiniCanvas/layout';
import styles from '../index.module.css';

const SAVE_DEBOUNCE_MS = 500;
const LAYOUT_DEBOUNCE_MS = 450;
const EMPTY_REFS: ShotRefSlot[] = [];
const PLANNING_PACKET_ERROR = /cannot edit a shot packet while planning/i;

function isPlanningPacketError(err: unknown): boolean {
  const detail = err instanceof Error ? err.message : String(err);
  return PLANNING_PACKET_ERROR.test(detail);
}

function sameShotLayout(
  current: ShotGraphLayout | null | undefined,
  next: ShotGraphLayout
): boolean {
  return JSON.stringify(current ?? null) === JSON.stringify(next);
}

export interface ShotPacketInspectorProps {
  sessionId: string;
  scene: StoryboardScene;
  artifacts: ArtifactNode[];
  preview?: React.ReactNode;
  videoPath?: string | null;
  posterPath?: string | null;
  videoStatus?: StoryboardVideoSlotStatus;
  generating: boolean;
  /** Session-level planning job — packets exist but must not be PUT yet. */
  planning?: boolean;
  reviewLocked: boolean;
  shotNumber?: number;
  shotTotal?: number;
  imageModel?: string | null;
  videoModel?: string | null;
  onUnsavedChange?: (dirty: boolean) => void;
  onPacket?: (packet: ShotPacketView | null) => void;
  onBusyAction?: () => void;
}

function packetSceneRoot(scene: StoryboardScene): string {
  return (scene.sceneRoot ?? '').replace(/\\/g, '/');
}

function packetShotIdx(scene: StoryboardScene): number {
  return scene.shotIndex ?? 0;
}

function effectivePrompt(packet: ShotPacketView): string {
  return packet.prompt_override ?? packet.compiled_prompt ?? '';
}

function libraryOptions(artifacts: ArtifactNode[], kind: 'image' | 'audio'): Array<{ path: string; label: string }> {
  const files = flattenArtifacts(artifacts).filter((node) => {
    if (node.is_dir) return false;
    return kind === 'image' ? isImageArtifactPath(node.path) : isAudioArtifactPath(node.path);
  });
  return files
    .filter((node) => {
      const path = node.path.replace(/\\/g, '/').toLowerCase();
      if (kind === 'audio') return path.endsWith('.wav') || path.includes('voice') || path.includes('cameo');
      return (
        path.includes('character_portrait') ||
        path.includes('environment') ||
        path.includes('prop') ||
        path.includes('cameo') ||
        path.includes('user_refs')
      );
    })
    .map((node) => {
      const path = node.path.replace(/\\/g, '/');
      const parts = path.split('/');
      return { path, label: parts.slice(-2).join(' / ') };
    });
}

function fileKind(file: File): 'image' | 'audio' | null {
  if (file.type.startsWith('image/') || /\.(png|jpe?g|webp)$/i.test(file.name)) return 'image';
  if (file.type.startsWith('audio/') || /\.(wav|mp3|m4a)$/i.test(file.name)) return 'audio';
  return null;
}

const ShotPacketInspector: React.FC<ShotPacketInspectorProps> = ({
  sessionId,
  scene,
  artifacts,
  preview,
  videoPath,
  posterPath,
  videoStatus,
  generating,
  planning = false,
  reviewLocked,
  shotNumber,
  shotTotal,
  imageModel,
  videoModel,
  onUnsavedChange,
  onPacket,
  onBusyAction,
}) => {
  const { t } = useTranslation();
  const [message, messageHolder] = useArcoMessage();
  const sceneRoot = packetSceneRoot(scene);
  const shotIdx = packetShotIdx(scene);
  const [packet, setPacket] = useState<ShotPacketView | null>(null);
  const [promptText, setPromptText] = useState('');
  const [imagePrompts, setImagePrompts] = useState<Record<string, string>>({});
  const [metaPatch, setMetaPatch] = useState<Record<string, Partial<CanvasNodeMetadata>>>({});
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(VIDEO_NODE_ID);
  const [positions, setPositions] = useState<Record<string, MiniPoint>>({});
  const [viewport, setViewport] = useState<MiniViewport>({ x: 36, y: 28, k: 1 });
  const fileRef = useRef<HTMLInputElement>(null);
  const pendingUpload = useRef<{ kind: 'image' | 'audio'; slot: number } | null>(null);
  const saveTimer = useRef<number | null>(null);
  const layoutTimer = useRef<number | null>(null);
  const packetRef = useRef<ShotPacketView | null>(null);
  const positionsRef = useRef<Record<string, MiniPoint>>({});
  const viewportRef = useRef(viewport);
  const canvasRef = useRef<HTMLDivElement>(null);
  const fittedKey = useRef('');
  const [canvasExpanded, setCanvasExpanded] = useState(false);

  const editable =
    Boolean(packet) &&
    !planning &&
    !generating &&
    !reviewLocked &&
    packet?.run_state !== 'generating';
  const canGenerate =
    Boolean(packet) &&
    !planning &&
    !generating &&
    !reviewLocked &&
    packet?.run_state !== 'generating';

  const applyLayoutFromPacket = useCallback((next: ShotPacketView) => {
    const defaults = defaultNodePositions(
      next.image_refs.map((slot) => slot.slot),
      next.audio_refs.slice(0, MAX_SHOT_AUDIO_REFS).map((slot) => slot.slot)
    );
    const merged = mergePositions(defaults, next.layout?.nodes ?? undefined);
    setPositions(merged);
    positionsRef.current = merged;
    if (next.layout?.viewport) {
      const vp = next.layout.viewport;
      const nextViewport = clampViewport({ x: vp.x, y: vp.y, k: vp.k });
      setViewport(nextViewport);
      viewportRef.current = nextViewport;
    }
  }, []);

  const applyPacket = useCallback(
    (next: ShotPacketView, opts?: { keepPrompt?: boolean; keepLayout?: boolean }) => {
      setPacket(next);
      packetRef.current = next;
      if (!opts?.keepPrompt) setPromptText(effectivePrompt(next));
      if (!opts?.keepLayout) applyLayoutFromPacket(next);
      onPacket?.(next);
      onUnsavedChange?.(false);
    },
    [applyLayoutFromPacket, onPacket, onUnsavedChange]
  );

  const loadPacket = useCallback(async () => {
    if (!sceneRoot) {
      setPacket(null);
      onPacket?.(null);
      return;
    }
    setLoading(true);
    try {
      const next = await getShotPacket(sessionId, sceneRoot, shotIdx);
      applyPacket(next, { keepLayout: fittedKey.current === `${sceneRoot}:${shotIdx}` });
    } catch {
      setPacket(null);
      onPacket?.(null);
    } finally {
      setLoading(false);
    }
  }, [applyPacket, onPacket, sceneRoot, sessionId, shotIdx]);

  useEffect(() => {
    void loadPacket();
  }, [loadPacket, scene.id, generating, planning]);

  useLayoutEffect(() => {
    setSelectedNodeId(VIDEO_NODE_ID);
    setImagePrompts({});
    setMetaPatch({});
    setCanvasExpanded(false);
    fittedKey.current = '';
  }, [scene.id]);

  useLayoutEffect(() => {
    fittedKey.current = '';
  }, [canvasExpanded]);

  const dirty = useMemo(() => {
    if (!packet) return false;
    return promptText !== effectivePrompt(packet);
  }, [packet, promptText]);

  useEffect(() => {
    onUnsavedChange?.(dirty);
  }, [dirty, onUnsavedChange]);

  const persist = useCallback(
    async (patch: ShotPacketPatch) => {
      if (!sceneRoot || !editable) return;
      setSaving(true);
      try {
        const next = await putShotPacket(sessionId, sceneRoot, shotIdx, patch);
        applyPacket(next, {
          keepPrompt: patch.recompile == null || patch.recompile === false,
          keepLayout: true,
        });
        setSavedAt(Date.now());
      } catch (err) {
        if (isPlanningPacketError(err)) return;
        message.error(
          t('videoGeneration.studio.storyboard.packetSaveFailed', {
            defaultValue: '镜头包保存失败',
          }) + `: ${err instanceof Error ? err.message : String(err)}`
        );
      } finally {
        setSaving(false);
      }
    },
    [applyPacket, editable, message, sceneRoot, sessionId, shotIdx, t]
  );

  const layoutPayload = useCallback(
    (nextPositions = positionsRef.current, nextViewport = viewportRef.current): ShotGraphLayout => {
      const current = packetRef.current;
      const allowed = new Set<string>([VIDEO_NODE_ID]);
      current?.image_refs.forEach((slot) => allowed.add(imageNodeId(slot.slot)));
      current?.audio_refs.forEach((slot) => allowed.add(audioNodeId(slot.slot)));
      const nodes: Record<string, MiniPoint> = {};
      for (const [id, point] of Object.entries(nextPositions)) {
        if (allowed.has(id)) nodes[id] = point;
      }
      return { viewport: nextViewport, nodes };
    },
    []
  );

  const persistLayout = useCallback(
    (nextPositions = positionsRef.current, nextViewport = viewportRef.current) => {
      if (!editable) return;
      const next = layoutPayload(nextPositions, nextViewport);
      if (sameShotLayout(packetRef.current?.layout, next)) return;
      if (layoutTimer.current) window.clearTimeout(layoutTimer.current);
      layoutTimer.current = window.setTimeout(() => {
        if (sameShotLayout(packetRef.current?.layout, next)) return;
        void persist({ layout: next, recompile: false });
      }, LAYOUT_DEBOUNCE_MS);
    },
    [editable, layoutPayload, persist]
  );

  const schedulePromptSave = useCallback(
    (text: string) => {
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => {
        void persist({ prompt_override: text, recompile: false });
      }, SAVE_DEBOUNCE_MS);
    },
    [persist]
  );

  const persistImagePrompt = useCallback(
    async (path: string, text: string) => {
      if (!editable) return;
      setSaving(true);
      try {
        await updateArtifactImagePrompt(sessionId, path, text);
        setSavedAt(Date.now());
      } catch (err) {
        message.error(
          t('videoGeneration.studio.storyboard.imagePromptSaveFailed', {
            defaultValue: '生图提示词保存失败',
          }) + `: ${err instanceof Error ? err.message : String(err)}`
        );
      } finally {
        setSaving(false);
      }
    },
    [editable, message, sessionId, t]
  );

  useEffect(
    () => () => {
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      if (layoutTimer.current) window.clearTimeout(layoutTimer.current);
    },
    []
  );

  useEffect(() => {
    if (editable) return;
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
    if (layoutTimer.current) {
      window.clearTimeout(layoutTimer.current);
      layoutTimer.current = null;
    }
  }, [editable]);

  const imageRefs = packet?.image_refs ?? EMPTY_REFS;
  const audioRefs = (packet?.audio_refs ?? EMPTY_REFS).slice(0, MAX_SHOT_AUDIO_REFS);

  useEffect(() => {
    if (!packet) return;
    let cancelled = false;
    const paths = packet.image_refs.filter((slot) => slotHasGenerationPrompt(slot, 'image') && slot.path);
    void Promise.all(
      paths.map(async (slot) => {
        try {
          const info = await getArtifactImagePrompt(sessionId, slot.path!);
          return [slot.path!, info.prompt || ''] as const;
        } catch {
          return [slot.path!, ''] as const;
        }
      })
    ).then((entries) => {
      if (cancelled) return;
      setImagePrompts((current) => {
        const next = { ...current };
        for (const [path, prompt] of entries) next[path] = prompt;
        return next;
      });
    });
    return () => {
      cancelled = true;
    };
  }, [packet, sessionId]);

  const mediaPaths = useMemo(() => {
    const paths: Array<string | null | undefined> = [videoPath, posterPath];
    for (const slot of imageRefs) paths.push(slot.path);
    for (const slot of audioRefs) paths.push(slot.path);
    for (const take of packet?.takes ?? []) paths.push(take.video_path);
    return paths;
  }, [audioRefs, imageRefs, packet?.takes, posterPath, videoPath]);

  const mediaUrls = useShotCanvasMedia(sessionId, mediaPaths);
  const imageLibrary = useMemo(() => libraryOptions(artifacts, 'image'), [artifacts]);
  const audioLibrary = useMemo(() => libraryOptions(artifacts, 'audio'), [artifacts]);

  const roleLabel = useCallback(
    (kind: 'image' | 'audio', role: ShotRefRole, slot: ShotRefSlot) => {
      const custom = slot.label.trim();
      const useCustom =
        Boolean(custom) &&
        custom.length <= 8 &&
        !custom.includes('/') &&
        !custom.includes('_') &&
        role !== 'continuity_last_frame';
      if (useCustom) return custom;
      if (kind === 'audio') {
        return t('videoGeneration.studio.storyboard.refAudio', { defaultValue: '音色' });
      }
      switch (role) {
        case 'continuity_last_frame':
          return t('videoGeneration.studio.storyboard.refContinuity', { defaultValue: '尾帧' });
        case 'portrait':
          return t('videoGeneration.studio.storyboard.refPortrait', { defaultValue: '定妆' });
        case 'environment':
          return t('videoGeneration.studio.storyboard.refEnvironment', { defaultValue: '场景' });
        case 'prop':
          return t('videoGeneration.studio.storyboard.refProp', { defaultValue: '道具' });
        default:
          return t('videoGeneration.studio.storyboard.refCustom', { defaultValue: '参考' });
      }
    },
    [t]
  );

  const { nodes, connections } = useMemo(() => {
    if (!packet) return { nodes: [] as CanvasNodeData[], connections: [] };
    return packetToCanvasGraph({
      packet,
      positions,
      mediaUrls,
      imagePrompts,
      prompt: promptText,
      generating,
      videoPath,
      posterPath,
      videoStatus,
      metaPatch,
      models: { image: imageModel, video: videoModel },
      titles: {
        video: t('videoGeneration.studio.storyboard.videoNode', { defaultValue: '视频' }),
        image: (slot) => roleLabel('image', slot.role, slot),
        audio: (slot) => roleLabel('audio', slot.role, slot),
      },
    });
  }, [
    generating,
    imageModel,
    imagePrompts,
    mediaUrls,
    metaPatch,
    packet,
    positions,
    posterPath,
    promptText,
    roleLabel,
    t,
    videoModel,
    videoPath,
    videoStatus,
  ]);

  useLayoutEffect(() => {
    if (!packet || !sceneRoot) return;
    if (packet.scene_root.replace(/\\/g, '/') !== sceneRoot || packet.shot_idx !== shotIdx) return;
    const key = `${sceneRoot}:${shotIdx}`;
    if (fittedKey.current === key) return;
    const el = canvasRef.current;
    if (!el) return;
    const box = el.getBoundingClientRect();
    if (box.width < 40 || box.height < 40) return;
    const next = clampViewport(
      fitViewport(
        nodes.map((node) => ({ x: node.position.x, y: node.position.y, w: node.width, h: node.height })),
        box.width,
        box.height
      )
    );
    setViewport(next);
    viewportRef.current = next;
    fittedKey.current = key;
  }, [nodes, packet, sceneRoot, shotIdx]);

  const handleRefFile = (kind: 'image' | 'audio', slot: number) => {
    pendingUpload.current = { kind, slot };
    fileRef.current?.click();
  };

  const onFileChosen = async (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = '';
    const pending = pendingUpload.current;
    pendingUpload.current = null;
    if (!file || !pending || !sceneRoot || !editable) return;
    try {
      const next = await uploadShotRefFile(sessionId, {
        sceneRoot,
        shotIdx,
        kind: pending.kind,
        slot: pending.slot,
        file,
      });
      applyPacket(next, { keepLayout: true });
      setSelectedNodeId(pending.kind === 'image' ? imageNodeId(pending.slot) : audioNodeId(pending.slot));
    } catch (err) {
      message.error(err instanceof Error ? err.message : String(err));
    }
  };

  const bindLibrary = async (kind: 'image' | 'audio', slot: number, sourcePath: string) => {
    if (!sceneRoot || !editable || !sourcePath) return;
    try {
      const next = await replaceShotRef(sessionId, {
        scene_root: sceneRoot,
        shot_idx: shotIdx,
        kind,
        slot,
        source_path: sourcePath,
      });
      applyPacket(next, { keepLayout: true });
    } catch (err) {
      message.error(err instanceof Error ? err.message : String(err));
    }
  };

  const unbindSlot = async (slot: ShotRefSlot, kind: 'image' | 'audio', remove = false) => {
    if (!sceneRoot || !editable) return;
    try {
      const next = await replaceShotRef(sessionId, {
        scene_root: sceneRoot,
        shot_idx: shotIdx,
        kind,
        slot: slot.slot,
        source_path: null,
        unbound: !remove,
        remove,
      });
      applyPacket(next, { keepLayout: true });
      if (remove) setSelectedNodeId(VIDEO_NODE_ID);
    } catch (err) {
      message.error(err instanceof Error ? err.message : String(err));
    }
  };

  const addRef = (kind: 'image' | 'audio', world?: Position) => {
    if (!editable) return;
    const current = kind === 'image' ? imageRefs : audioRefs;
    const cap = kind === 'image' ? MAX_SHOT_IMAGE_REFS : MAX_SHOT_AUDIO_REFS;
    if (current.length >= cap) {
      message.warning(
        kind === 'image'
          ? t('videoGeneration.studio.storyboard.maxImageRefs', { defaultValue: '每镜最多 9 张参考图' })
          : t('videoGeneration.studio.storyboard.maxAudioRefs', { defaultValue: '每镜最多 3 路音色' })
      );
      return;
    }
    const slot = nextSlot(current);
    const id = kind === 'image' ? imageNodeId(slot) : audioNodeId(slot);
    const size = nodeSize(kind);
    const last = current.at(-1);
    const lastId = last ? (kind === 'image' ? imageNodeId(last.slot) : audioNodeId(last.slot)) : null;
    const lastPos = lastId ? positionsRef.current[lastId] : null;
    const point: MiniPoint = world
      ? { x: world.x, y: world.y, w: size.w, h: size.h }
      : lastPos
        ? { x: lastPos.x, y: lastPos.y + size.h + 28, w: size.w, h: size.h }
        : { x: 48, y: kind === 'image' ? 48 : 280, w: size.w, h: size.h };
    const nextPositions = { ...positionsRef.current, [id]: point };
    setPositions(nextPositions);
    positionsRef.current = nextPositions;
    persistLayout(nextPositions);
    handleRefFile(kind, slot);
  };

  const handleGenerate = async (cascade = false, promptOverride?: string) => {
    if (!sceneRoot || !packet || !canGenerate) return;
    const outbound = nodeMentionsToSeedancePrompt(promptOverride ?? promptText, packet);
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
    if (outbound !== effectivePrompt(packet)) {
      await persist({ prompt_override: outbound, recompile: false });
    }
    onBusyAction?.();
    try {
      if (packet.run_state === 'awaiting_review') {
        await approveShot(sessionId, sceneRoot, shotIdx, false);
      } else {
        await retakeShot(sessionId, sceneRoot, shotIdx, { concat: true, cascade });
      }
      message.success(
        t('videoGeneration.studio.storyboard.generateStarted', {
          defaultValue: '已开始生成本镜视频',
        })
      );
    } catch (err) {
      message.error(err instanceof Error ? err.message : String(err));
    }
  };

  const handleSelectTake = async (take: number) => {
    if (!sceneRoot) return;
    onBusyAction?.();
    try {
      await selectShotTake(sessionId, sceneRoot, shotIdx, take, true);
    } catch (err) {
      message.error(err instanceof Error ? err.message : String(err));
    }
  };

  const handleNodeMove = (nodeId: string, position: Position) => {
    const current = positionsRef.current[nodeId] ?? { x: position.x, y: position.y };
    const next = { ...positionsRef.current, [nodeId]: { ...current, x: position.x, y: position.y } };
    positionsRef.current = next;
    setPositions(next);
    persistLayout(next);
  };

  const handleNodeResize = (nodeId: string, width: number, height: number, position?: Position) => {
    const current = positionsRef.current[nodeId] ?? { x: 0, y: 0 };
    const nextPoint: MiniPoint = {
      x: position?.x ?? current.x,
      y: position?.y ?? current.y,
      w: width,
      h: height,
    };
    const next = { ...positionsRef.current, [nodeId]: nextPoint };
    positionsRef.current = next;
    setPositions(next);
    persistLayout(next);
  };

  const handlePromptChange = (nodeId: string, prompt: string) => {
    const parsed = parseShotNodeId(nodeId);
    if (parsed?.kind === 'video') {
      const currentPacket = packetRef.current;
      const seedance = currentPacket ? nodeMentionsToSeedancePrompt(prompt, currentPacket) : prompt;
      setPromptText(seedance);
      if (editable) schedulePromptSave(seedance);
      return;
    }
    if (parsed?.kind === 'image') {
      const slot = imageRefs.find((item) => item.slot === parsed.slot);
      if (!slot?.path) return;
      setImagePrompts((current) => ({ ...current, [slot.path!]: prompt }));
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => {
        void persistImagePrompt(slot.path!, prompt);
      }, SAVE_DEBOUNCE_MS);
    }
  };

  const handleConfigChange = (nodeId: string, patch: Partial<CanvasNodeMetadata>) => {
    const { prompt, composerContent, ...rest } = patch;
    if (Object.keys(rest).length > 0) {
      setMetaPatch((current) => ({ ...current, [nodeId]: { ...current[nodeId], ...rest } }));
    }
    if (nodeId === VIDEO_NODE_ID && patch.seconds != null) {
      const parsed = Number(patch.seconds);
      if (Number.isFinite(parsed) && parsed > 0 && packetRef.current?.duration_secs !== parsed) {
        void persist({ duration_secs: parsed, recompile: false });
      }
    }
    if (nodeId === VIDEO_NODE_ID && (prompt != null || composerContent != null)) {
      const text = composerContent ?? prompt ?? promptText;
      const currentPacket = packetRef.current;
      const seedance = currentPacket ? nodeMentionsToSeedancePrompt(text, currentPacket) : text;
      setPromptText(seedance);
      if (editable) schedulePromptSave(seedance);
    }
  };

  const handleGenerateNode = (nodeId: string, prompt: string) => {
    const parsed = parseShotNodeId(nodeId);
    if (parsed?.kind === 'image') {
      handlePromptChange(nodeId, prompt);
      return;
    }
    if (parsed?.kind === 'video') {
      const currentPacket = packetRef.current;
      const seedance = currentPacket ? nodeMentionsToSeedancePrompt(prompt, currentPacket) : prompt;
      setPromptText(seedance);
      void handleGenerate(false, seedance);
    }
  };

  const handleDeleteNode = (node: CanvasNodeData) => {
    const parsed = parseShotNodeId(node.id);
    if (!parsed || parsed.kind === 'video' || !editable) return;
    const slot =
      parsed.kind === 'image'
        ? imageRefs.find((item) => item.slot === parsed.slot)
        : audioRefs.find((item) => item.slot === parsed.slot);
    if (!slot) return;
    const drop = slot.role === 'custom' && !slot.character_id;
    void unbindSlot(slot, parsed.kind, drop);
  };

  const handleUploadNode = (node: CanvasNodeData | null, world?: Position) => {
    if (!editable) return;
    if (!node) {
      addRef('image', world);
      return;
    }
    const parsed = parseShotNodeId(node.id);
    if (!parsed || parsed.kind === 'video') return;
    handleRefFile(parsed.kind, parsed.slot);
  };

  const handleBindLibrary = (nodeId: string, sourcePath: string) => {
    const parsed = parseShotNodeId(nodeId);
    if (!parsed || parsed.kind === 'video') return;
    void bindLibrary(parsed.kind, parsed.slot, sourcePath);
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Delete' && event.key !== 'Backspace') return;
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) return;
      if (!selectedNodeId || selectedNodeId === VIDEO_NODE_ID || !editable) return;
      const node = nodes.find((item) => item.id === selectedNodeId);
      if (!node) return;
      event.preventDefault();
      handleDeleteNode(node);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [audioRefs, editable, handleDeleteNode, imageRefs, nodes, selectedNodeId]);

  const handleCanvasDrop = async (event: React.DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    if (!editable || !sceneRoot) return;
    const file = event.dataTransfer.files?.[0];
    if (!file) return;
    const kind = fileKind(file);
    if (!kind) return;
    const target = event.target instanceof Element ? event.target.closest('[data-node-id]') : null;
    const nodeId = target?.getAttribute('data-node-id');
    let slot: number | null = null;
    if (nodeId?.startsWith(`${kind}-`)) {
      slot = Number(nodeId.slice(kind.length + 1));
    } else if (!nodeId || nodeId === VIDEO_NODE_ID) {
      const current = kind === 'image' ? imageRefs : audioRefs;
      const cap = kind === 'image' ? MAX_SHOT_IMAGE_REFS : MAX_SHOT_AUDIO_REFS;
      if (current.length >= cap) {
        message.warning(
          kind === 'image'
            ? t('videoGeneration.studio.storyboard.maxImageRefs', { defaultValue: '每镜最多 9 张参考图' })
            : t('videoGeneration.studio.storyboard.maxAudioRefs', { defaultValue: '每镜最多 3 路音色' })
        );
        return;
      }
      slot = nextSlot(current);
    }
    if (slot == null || !Number.isFinite(slot)) return;
    try {
      const next = await uploadShotRefFile(sessionId, { sceneRoot, shotIdx, kind, slot, file });
      applyPacket(next, { keepLayout: true });
      setSelectedNodeId(kind === 'image' ? imageNodeId(slot) : audioNodeId(slot));
    } catch (err) {
      message.error(err instanceof Error ? err.message : String(err));
    }
  };

  const handleUnbindConnection = (connectionId: string) => {
    const connection = connections.find((item) => item.id === connectionId);
    if (!connection) return;
    const parsed = parseShotNodeId(connection.fromNodeId);
    if (!parsed || parsed.kind === 'video') return;
    const slot =
      parsed.kind === 'image'
        ? imageRefs.find((item) => item.slot === parsed.slot)
        : audioRefs.find((item) => item.slot === parsed.slot);
    if (!slot) return;
    void unbindSlot(slot, parsed.kind, slot.role === 'custom' && !slot.character_id);
  };

  const saveLabel = saving
    ? t('videoGeneration.studio.storyboard.saving', { defaultValue: '保存中' })
    : savedAt
      ? t('videoGeneration.studio.storyboard.savedAt', {
          time: new Date(savedAt).toLocaleTimeString(),
          defaultValue: '已保存 {{time}}',
        })
      : dirty
        ? t('videoGeneration.studio.storyboard.unsaved', { defaultValue: '未保存' })
        : null;

  return (
    <div className={styles.shotGraph} data-testid='shot-graph'>
      {messageHolder}
      <input
        ref={fileRef}
        type='file'
        className={styles.packetFileInput}
        accept='image/png,image/jpeg,image/webp,audio/wav,audio/mpeg'
        onChange={(event) => void onFileChosen(event)}
      />
      <div className={styles.shotGraphFacts}>
        <span>
          {t('videoGeneration.studio.storyboard.shotNumberOf', {
            number: shotNumber ?? (packet?.shot_idx ?? 0) + 1,
            total: shotTotal ?? 0,
            defaultValue: '镜头 {{number}} / {{total}}',
          })}
          {packet?.location_id ? ` · ${packet.location_id}` : ''}
        </span>
        <span className={styles.shotMiniFactActions}>
          {saveLabel ? <span className={styles.packetSaveHint}>{saveLabel}</span> : null}
          {packet?.run_state === 'continuity_stale' ? (
            <Popconfirm
              title={t('videoGeneration.studio.storyboard.cascadeTitle', {
                defaultValue: '顺延重渲后续镜头？',
              })}
              content={t('videoGeneration.studio.storyboard.cascadeBody', {
                defaultValue: '后续镜的尾帧参考已过期。确认后将按顺序重渲本场后续镜头并消耗积分。',
              })}
              onOk={() => void handleGenerate(true)}
            >
              <button type='button' className={styles.shotGraphTextBtn} disabled={!canGenerate}>
                {t('videoGeneration.studio.storyboard.cascadeRetake', { defaultValue: '顺延' })}
              </button>
            </Popconfirm>
          ) : null}
          {(packet?.takes ?? []).map((take) => (
            <button
              key={take.take}
              type='button'
              className={`${styles.packetTakeChip} ${take.current ? styles.packetTakeCurrent : ''}`}
              disabled={generating || reviewLocked}
              onClick={() => {
                if (!take.current) void handleSelectTake(take.take);
              }}
            >
              v{take.take}
            </button>
          ))}
        </span>
      </div>
      <ShotInfiniteCanvas
        containerRef={canvasRef}
        nodes={nodes}
        connections={connections}
        viewport={viewport}
        onViewportChange={(next) => {
          const clamped = clampViewport(next);
          setViewport(clamped);
          viewportRef.current = clamped;
          persistLayout(positionsRef.current, clamped);
        }}
        selectedNodeId={selectedNodeId}
        onSelectNode={setSelectedNodeId}
        readOnly={!editable}
        generating={generating || packet?.run_state === 'generating'}
        overlay={preview}
        expanded={canvasExpanded}
        onExpandedChange={setCanvasExpanded}
        loading={loading && !packet}
        empty={
          !sceneRoot
            ? t('videoGeneration.studio.storyboard.packetMissingRoot', {
                defaultValue: '规划完成后可在此装配本镜参考与提示词。',
              })
            : undefined
        }
        imageLibrary={imageLibrary}
        audioLibrary={audioLibrary}
        canAddImage={editable && imageRefs.length < MAX_SHOT_IMAGE_REFS}
        canAddAudio={editable && audioRefs.length < MAX_SHOT_AUDIO_REFS}
        onNodeMove={handleNodeMove}
        onNodeResize={handleNodeResize}
        onPromptChange={handlePromptChange}
        onConfigChange={handleConfigChange}
        onGenerate={handleGenerateNode}
        onDeleteNode={handleDeleteNode}
        onUploadNode={handleUploadNode}
        onBindLibrary={handleBindLibrary}
        onAddImage={(world) => addRef('image', world)}
        onAddAudio={(world) => addRef('audio', world)}
        onUnbindConnection={handleUnbindConnection}
        onDrop={handleCanvasDrop}
        onOpenVersions={() => {
          const current = packet?.takes?.find((take) => take.current);
          const others = packet?.takes?.filter((take) => !take.current) ?? [];
          const next = others[0];
          if (next && next.take !== current?.take) void handleSelectTake(next.take);
        }}
      />
      {packet?.error ? <p className={styles.packetError}>{packet.error}</p> : null}
    </div>
  );
};

export default ShotPacketInspector;
