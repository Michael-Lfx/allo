import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from 'react';
import { App as AntApp, ConfigProvider } from 'antd';
import { FolderOpen, Image as ImageIcon, Info, Maximize2, Minimize2, Music2, Scan, Trash2, Upload } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import '@oc/lib/canvas/node-registry';
import '@oc/lib/canvas/tool-registry';
import '@oc/styles/globals.css';

import { InfiniteCanvas } from '@oc/components/canvas/infinite-canvas';
import { CanvasNode } from '@oc/components/canvas/canvas-node';
import { CanvasNodeActionContext } from '@oc/components/canvas/canvas-node-action-context';
import { CanvasNodeInfoModal, CanvasNodeToolbar } from '@oc/components/canvas/canvas-node-toolbar';
import { CanvasNodePromptPanel } from '@oc/components/canvas/canvas-node-prompt-panel';
import { CanvasNodePanelOverlay } from '@oc/components/canvas/canvas-workspace-overlays';
import { ConnectionPath, activeConnectionPath } from '@oc/components/canvas/canvas-connections';
import { CanvasZoomControls } from '@oc/components/canvas/canvas-zoom-controls';
import { Minimap } from '@oc/components/canvas/canvas-mini-map';
import { CanvasMenuRow, CanvasMenuSeparator, CanvasOverlay } from '@oc/components/canvas/canvas-overlay';
import { canvasThemes } from '@oc/lib/canvas-theme';
import { canvasT } from '@oc/lib/canvas/canvas-i18n';
import { buildCanvasResourceReferences, getMentionResourceNodes } from '@oc/lib/canvas/canvas-resource-references';
import { canvasActiveNodeId, canvasRelatedHighlight } from '@oc/lib/canvas/canvas-related-highlight';
import { canvasNodeDisplayUrl } from '@oc/lib/canvas/canvas-media-id';
import { getOcPortalHost, disposeOcPortalHost } from '@oc/lib/oc-scope';
import { CanvasColorThemeScope } from '@oc/stores/use-canvas-color-theme';
import { useThemeContext } from '@renderer/hooks/context/ThemeContext';
import {
  CanvasNodeType,
  type CanvasConnection,
  type CanvasNodeData,
  type CanvasNodeMetadata,
  type ContextMenuState,
  type Position,
  type ViewportTransform,
} from '@oc/types/canvas';
import { getVideoCanvasAntTheme } from '@renderer/pages/videoCanvas/lib/ocAntTheme';
import { syncOcConfigFromAlloMediaModels } from '@renderer/pages/videoCanvas/lib/syncOcModels';

import { VIDEO_NODE_ID, clampScale, fitViewport } from '../shotMiniCanvas/layout';
import { parseShotNodeId } from './packetToCanvas';
import { shotBoundMentionReferences } from './shotMentions';
import styles from '../index.module.css';

const NOOP = () => undefined;
const EMPTY_REFS: never[] = [];

type LibraryItem = { path: string; label: string };

export type ShotInfiniteCanvasProps = {
  nodes: CanvasNodeData[];
  connections: CanvasConnection[];
  viewport: ViewportTransform;
  onViewportChange: (viewport: ViewportTransform) => void;
  selectedNodeId: string | null;
  onSelectNode: (nodeId: string | null) => void;
  readOnly: boolean;
  generating: boolean;
  overlay?: React.ReactNode;
  expanded?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  empty?: React.ReactNode;
  loading?: boolean;
  imageLibrary: LibraryItem[];
  audioLibrary: LibraryItem[];
  canAddImage: boolean;
  canAddAudio: boolean;
  onNodeMove: (nodeId: string, position: Position) => void;
  onNodeResize: (nodeId: string, width: number, height: number, position?: Position) => void;
  onPromptChange: (nodeId: string, prompt: string) => void;
  onConfigChange: (nodeId: string, patch: Partial<CanvasNodeMetadata>) => void;
  onGenerate: (nodeId: string, prompt: string) => void;
  onDeleteNode: (node: CanvasNodeData) => void;
  onUploadNode: (node: CanvasNodeData | null, world?: Position) => void;
  onBindLibrary: (nodeId: string, sourcePath: string) => void;
  onAddImage: (world?: Position) => void;
  onAddAudio: (world?: Position) => void;
  onUnbindConnection: (connectionId: string) => void;
  onOpenVersions?: (node: CanvasNodeData) => void;
  onDrop?: (event: React.DragEvent<HTMLDivElement>) => void;
  containerRef?: React.RefObject<HTMLDivElement | null>;
};

function screenToCanvas(
  clientX: number,
  clientY: number,
  container: HTMLElement,
  viewport: ViewportTransform
): Position {
  const rect = container.getBoundingClientRect();
  return {
    x: (clientX - rect.left - viewport.x) / viewport.k,
    y: (clientY - rect.top - viewport.y) / viewport.k,
  };
}

function connectionLayerBounds(viewport: ViewportTransform, size: { width: number; height: number }) {
  const padding = 160 / Math.max(viewport.k, 0.05);
  const left = -viewport.x / viewport.k - padding;
  const top = -viewport.y / viewport.k - padding;
  return {
    left,
    top,
    width: Math.max(2, size.width / viewport.k + padding * 2),
    height: Math.max(2, size.height / viewport.k + padding * 2),
  };
}

const ShotInfiniteCanvas: React.FC<ShotInfiniteCanvasProps> = ({
  nodes,
  connections,
  viewport,
  onViewportChange,
  selectedNodeId,
  onSelectNode,
  readOnly,
  generating,
  overlay,
  expanded = false,
  onExpandedChange,
  empty,
  loading: _loading,
  imageLibrary,
  audioLibrary,
  canAddImage,
  canAddAudio,
  onNodeMove,
  onNodeResize,
  onPromptChange,
  onConfigChange,
  onGenerate,
  onDeleteNode,
  onUploadNode,
  onBindLibrary,
  onAddImage,
  onAddAudio,
  onUnbindConnection,
  onOpenVersions,
  onDrop,
  containerRef: containerRefProp,
}) => {
  const { t } = useTranslation();
  const { theme: appTheme } = useThemeContext();
  const colorTheme = appTheme === 'dark' ? 'dark' : 'light';
  const theme = canvasThemes[colorTheme] ?? canvasThemes.light;
  const innerRef = useRef<HTMLDivElement>(null);
  const containerRef = containerRefProp ?? innerRef;
  const nodeDraggingRef = useRef(false);
  const dragRef = useRef<{
    id: string;
    startX: number;
    startY: number;
    origin: Position;
    moved: boolean;
  } | null>(null);
  const viewportRef = useRef(viewport);
  const nodesRef = useRef(nodes);
  const [viewportSize, setViewportSize] = useState({ width: 0, height: 0 });
  const [hoveredNodeId, setHoveredNodeId] = useState<string | null>(null);
  const [toolbarNodeId, setToolbarNodeId] = useState<string | null>(null);
  const [dialogNodeId, setDialogNodeId] = useState<string | null>(null);
  const [infoNodeId, setInfoNodeId] = useState<string | null>(null);
  const [previewNodeId, setPreviewNodeId] = useState<string | null>(null);
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const [isMiniMapOpen, setIsMiniMapOpen] = useState(false);
  const [selectedConnectionId, setSelectedConnectionId] = useState<string | null>(null);
  const [connecting, setConnecting] = useState<{
    fromNodeId: string;
    mouseWorld: Position;
  } | null>(null);
  const toolbarHideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [fillBox, setFillBox] = useState<{ top: number; left: number; width: number; height: number } | null>(
    null
  );

  viewportRef.current = viewport;
  nodesRef.current = nodes;

  useEffect(() => {
    void syncOcConfigFromAlloMediaModels().catch(() => undefined);
  }, []);

  useLayoutEffect(() => {
    if (!expanded) {
      setFillBox(null);
      return;
    }
    const host = document.querySelector<HTMLElement>('[data-studio-main]');
    if (!host) return;
    const update = () => {
      const rect = host.getBoundingClientRect();
      setFillBox({ top: rect.top, left: rect.left, width: rect.width, height: rect.height });
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(host);
    window.addEventListener('resize', update);
    window.addEventListener('scroll', update, true);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', update);
      window.removeEventListener('scroll', update, true);
    };
  }, [expanded]);

  useEffect(() => {
    getOcPortalHost();
    return () => disposeOcPortalHost();
  }, []);

  useEffect(() => {
    if (!expanded) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onExpandedChange?.(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [expanded, onExpandedChange]);

  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const update = () => setViewportSize({ width: el.clientWidth, height: el.clientHeight });
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, [containerRef]);

  useEffect(() => {
    setDialogNodeId((current) => {
      if (selectedNodeId && current && current !== selectedNodeId) return selectedNodeId;
      if (!current && selectedNodeId) {
        const node = nodesRef.current.find((item) => item.id === selectedNodeId);
        if (node?.type === CanvasNodeType.Video || node?.type === CanvasNodeType.Image) return selectedNodeId;
      }
      if (!selectedNodeId) return null;
      return current;
    });
  }, [selectedNodeId]);

  const nodeById = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);
  const selectedNodeIds = useMemo(
    () => (selectedNodeId ? new Set([selectedNodeId]) : new Set<string>()),
    [selectedNodeId]
  );
  const toolbarNode = toolbarNodeId ? nodeById.get(toolbarNodeId) ?? null : null;
  const dialogNode = dialogNodeId ? nodeById.get(dialogNodeId) ?? null : null;
  const infoNode = infoNodeId ? nodeById.get(infoNodeId) ?? null : null;
  const previewNode = previewNodeId ? nodeById.get(previewNodeId) ?? null : null;
  const activeNodeId = canvasActiveNodeId(hoveredNodeId, selectedNodeIds);
  const relatedHighlight = useMemo(
    () => canvasRelatedHighlight(activeNodeId, connections),
    [activeNodeId, connections]
  );
  const resourceReferences = useMemo(
    () => buildCanvasResourceReferences(nodes, connections, dialogNodeId || selectedNodeId),
    [connections, dialogNodeId, nodes, selectedNodeId]
  );
  const resourceByNodeId = useMemo(
    () => new Map(resourceReferences.map((item) => [item.nodeId, item])),
    [resourceReferences]
  );
  const mentionByNodeId = useMemo(() => {
    const map = new Map<string, ReturnType<typeof buildCanvasResourceReferences>>();
    for (const node of nodes) {
      const mentioned = getMentionResourceNodes(node.id, nodes, connections);
      map.set(
        node.id,
        resourceReferences.filter((item) => mentioned.some((ref) => ref.id === item.nodeId) || item.nodeId === node.id)
      );
    }
    return map;
  }, [connections, nodes, resourceReferences]);
  const bounds = useMemo(
    () => connectionLayerBounds(viewport, viewportSize),
    [viewport, viewportSize]
  );

  const keepToolbar = useCallback((nodeId: string) => {
    if (nodeDraggingRef.current) return;
    if (toolbarHideTimer.current) {
      clearTimeout(toolbarHideTimer.current);
      toolbarHideTimer.current = null;
    }
    setToolbarNodeId(nodeId);
  }, []);

  const hideToolbar = useCallback(() => {
    if (toolbarHideTimer.current) clearTimeout(toolbarHideTimer.current);
    toolbarHideTimer.current = setTimeout(() => {
      setToolbarNodeId(null);
      toolbarHideTimer.current = null;
    }, 120);
  }, []);

  const persistViewport = useCallback(
    (next: ViewportTransform) => {
      onViewportChange({ ...next, k: clampScale(next.k) });
    },
    [onViewportChange]
  );

  const setZoomScale = useCallback(
    (scale: number) => {
      const el = containerRef.current;
      const k = clampScale(scale);
      const cx = (el?.clientWidth ?? viewportSize.width) / 2;
      const cy = (el?.clientHeight ?? viewportSize.height) / 2;
      const worldX = (cx - viewport.x) / viewport.k;
      const worldY = (cy - viewport.y) / viewport.k;
      persistViewport({ k, x: cx - worldX * k, y: cy - worldY * k });
    },
    [containerRef, persistViewport, viewport.k, viewport.x, viewport.y, viewportSize.height, viewportSize.width]
  );

  const fitCanvas = useCallback(() => {
    const el = containerRef.current;
    if (!el) return;
    const rects = nodes.map((node) => ({
      x: node.position.x,
      y: node.position.y,
      w: node.width,
      h: node.height,
    }));
    persistViewport(fitViewport(rects, el.clientWidth, el.clientHeight));
  }, [containerRef, nodes, persistViewport]);

  useEffect(() => {
    const onMove = (event: MouseEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const dx = (event.clientX - drag.startX) / viewportRef.current.k;
      const dy = (event.clientY - drag.startY) / viewportRef.current.k;
      if (Math.abs(event.clientX - drag.startX) > 3 || Math.abs(event.clientY - drag.startY) > 3) {
        drag.moved = true;
        nodeDraggingRef.current = true;
      }
      onNodeMove(drag.id, { x: drag.origin.x + dx, y: drag.origin.y + dy });
    };
    const onUp = () => {
      if (dragRef.current?.moved) nodeDraggingRef.current = false;
      dragRef.current = null;
      nodeDraggingRef.current = false;
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [onNodeMove]);

  const handleNodeMouseDown = (event: ReactMouseEvent, nodeId: string) => {
    event.stopPropagation();
    if (event.button !== 0) return;
    const node = nodeById.get(nodeId);
    if (!node) return;
    onSelectNode(nodeId);
    setSelectedConnectionId(null);
    setMenu(null);
    if (readOnly || node.metadata?.locked) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest('button, input, textarea, select, label, [data-canvas-no-zoom], .canvas-node-resize')) return;
    dragRef.current = {
      id: nodeId,
      startX: event.clientX,
      startY: event.clientY,
      origin: { ...node.position },
      moved: false,
    };
  };

  const handleConnectStart = (
    event: ReactPointerEvent,
    nodeId: string,
    handleType: 'source' | 'target'
  ) => {
    event.stopPropagation();
    const parsed = parseShotNodeId(nodeId);
    if (!parsed) return;
    if (handleType === 'source' && parsed.kind !== 'video') {
      const node = nodeById.get(nodeId);
      if (!node) return;
      setConnecting({
        fromNodeId: nodeId,
        mouseWorld: { x: node.position.x + node.width, y: node.position.y + node.height / 2 },
      });
    }
  };

  useEffect(() => {
    if (!connecting) return;
    const onMove = (event: PointerEvent) => {
      const el = containerRef.current;
      if (!el) return;
      setConnecting((current) =>
        current
          ? { ...current, mouseWorld: screenToCanvas(event.clientX, event.clientY, el, viewportRef.current) }
          : current
      );
    };
    const onUp = () => setConnecting(null);
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
    return () => {
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
    };
  }, [connecting, containerRef]);

  const openNodeDialog = (node: CanvasNodeData) => {
    onSelectNode(node.id);
    setDialogNodeId((current) => (current === node.id ? null : node.id));
  };

  const downloadNode = (node: CanvasNodeData) => {
    const url = canvasNodeDisplayUrl(node);
    if (!url) return;
    const link = document.createElement('a');
    link.href = url;
    link.download = `${node.title || node.id}`;
    link.rel = 'noreferrer';
    document.body.appendChild(link);
    link.click();
    link.remove();
  };

  const nodeActionContext = useMemo(
    () => ({
      download: downloadNode,
      deleteNode: onDeleteNode,
      updateMetadata: (nodeId: string, patch: CanvasNodeMetadata) => onConfigChange(nodeId, patch),
      resizeNode: (nodeId: string, size: { width: number; height: number }) =>
        onNodeResize(nodeId, size.width, size.height),
    }),
    [onConfigChange, onDeleteNode, onNodeResize]
  );

  const boundMentions = useMemo(() => shotBoundMentionReferences(nodes), [nodes]);
  const promptMentions =
    dialogNode?.id === VIDEO_NODE_ID
      ? boundMentions
      : dialogNode
        ? (mentionByNodeId.get(dialogNode.id) ?? boundMentions)
        : boundMentions;

  const draftConnection = connecting
    ? activeConnectionPath(nodeById.get(connecting.fromNodeId), { nodeId: connecting.fromNodeId, handleType: 'source' }, connecting.mouseWorld)
    : null;

  return (
    <CanvasColorThemeScope theme={colorTheme}>
    <div
      className={`${styles.shotInfiniteCanvas} oc-root oc-canvas${colorTheme === 'dark' ? ' dark' : ''}`}
      data-testid='shot-infinite-canvas'
      data-canvas-theme={colorTheme}
      data-expanded={expanded ? 'true' : undefined}
      style={
        expanded && fillBox
          ? {
              position: 'fixed',
              top: fillBox.top,
              left: fillBox.left,
              width: fillBox.width,
              height: fillBox.height,
              zIndex: 60,
              minHeight: 0,
            }
          : undefined
      }
    >
      <ConfigProvider theme={getVideoCanvasAntTheme(colorTheme === 'dark')} getPopupContainer={getOcPortalHost}>
        <AntApp>
          <CanvasNodeActionContext.Provider value={nodeActionContext}>
            <div className={styles.shotCanvasStage}>
              <InfiniteCanvas
                containerRef={containerRef}
                viewport={viewport}
                onViewportChange={persistViewport}
                onCanvasDeselect={() => {
                  onSelectNode(null);
                  setDialogNodeId(null);
                  setSelectedConnectionId(null);
                  setMenu(null);
                }}
                onCanvasDoubleClick={fitCanvas}
                onContextMenu={(event) => {
                  event.preventDefault();
                  const el = containerRef.current;
                  if (!el) return;
                  const target = event.target instanceof Element ? event.target : null;
                  if (target?.closest('[data-node-id]')) return;
                  if (target?.closest('[data-connection-id]')) return;
                  setMenu({
                    type: 'canvas',
                    x: event.clientX,
                    y: event.clientY,
                    position: screenToCanvas(event.clientX, event.clientY, el, viewport),
                  });
                }}
                onDrop={onDrop}
              >
                <svg
                  className='absolute overflow-visible'
                  viewBox={`${bounds.left} ${bounds.top} ${bounds.width} ${bounds.height}`}
                  style={{
                    left: bounds.left,
                    top: bounds.top,
                    width: bounds.width,
                    height: bounds.height,
                    pointerEvents: 'none',
                    zIndex: 0,
                  }}
                >
                  {connections.map((connection) => {
                    const from = nodeById.get(connection.fromNodeId);
                    const to = nodeById.get(connection.toNodeId);
                    if (!from || !to) return null;
                    return (
                      <ConnectionPath
                        key={connection.id}
                        connection={connection}
                        from={from}
                        to={to}
                        active={
                          selectedConnectionId === connection.id || relatedHighlight.connectionIds.has(connection.id)
                        }
                        visualMode='full'
                        onSelect={() => {
                          setSelectedConnectionId(connection.id);
                          onSelectNode(null);
                        }}
                        onContextMenu={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          setMenu({
                            type: 'connection',
                            x: event.clientX,
                            y: event.clientY,
                            connectionId: connection.id,
                          });
                        }}
                      />
                    );
                  })}
                  {draftConnection ? (
                    <path
                      d={draftConnection}
                      fill='none'
                      stroke={theme.accent.primary}
                      strokeWidth='2'
                      strokeDasharray='6 6'
                      vectorEffect='non-scaling-stroke'
                    />
                  ) : null}
                </svg>
                {nodes.map((node) => (
                  <CanvasNode
                    key={node.id}
                    data={node}
                    isSelected={selectedNodeId === node.id}
                    isRelated={relatedHighlight.nodeIds.has(node.id)}
                    isFocusRelated={activeNodeId === node.id}
                    isConnectionTarget={connecting != null && node.id === VIDEO_NODE_ID}
                    isConnecting={Boolean(connecting)}
                    showImageInfo
                    mediaActive={selectedNodeId === node.id}
                    hydrateMediaPreview
                    readOnly={readOnly}
                    resourceLabel={resourceByNodeId.get(node.id)}
                    mentionReferences={
                      node.id === VIDEO_NODE_ID
                        ? boundMentions
                        : (mentionByNodeId.get(node.id) ?? EMPTY_REFS)
                    }
                    onMouseDown={handleNodeMouseDown}
                    onHoverStart={(nodeId) => {
                      setHoveredNodeId(nodeId);
                      keepToolbar(nodeId);
                    }}
                    onHoverEnd={(nodeId) => {
                      setHoveredNodeId((current) => (current === nodeId ? null : current));
                      hideToolbar();
                    }}
                    onConnectStart={handleConnectStart}
                    onResize={onNodeResize}
                    onContentChange={NOOP}
                    onRetry={(item) => onGenerate(item.id, item.metadata?.composerContent || item.metadata?.prompt || '')}
                    onOpenVersions={onOpenVersions}
                    onViewImage={(item) => setPreviewNodeId(item.id)}
                    onContextMenu={(event, nodeId) => {
                      event.preventDefault();
                      event.stopPropagation();
                      onSelectNode(nodeId);
                      setMenu({ type: 'node', x: event.clientX, y: event.clientY, nodeId });
                    }}
                  />
                ))}
                {empty ? (
                  <div className='absolute left-12 top-12 max-w-sm text-sm' style={{ color: theme.node.muted }}>
                    {empty}
                  </div>
                ) : null}
              </InfiniteCanvas>

              <CanvasNodeToolbar
                node={toolbarNode}
                viewport={viewport}
                containerRef={containerRef}
                onKeep={keepToolbar}
                onLeave={hideToolbar}
                onInfo={(node) => setInfoNodeId(node.id)}
                onEditText={NOOP}
                onDecreaseFont={NOOP}
                onIncreaseFont={NOOP}
                onToggleDialog={openNodeDialog}
                onAnnotate={NOOP}
                onGenerateImage={(node) => openNodeDialog(node)}
                onUpload={(node) => onUploadNode(node)}
                onDownload={downloadNode}
                onSaveAsset={NOOP}
                onMaskEdit={NOOP}
                onEmotion={NOOP}
                onPortraitTexture={NOOP}
                onCrop={NOOP}
                onSplit={NOOP}
                onUpscale={NOOP}
                onAngle={NOOP}
                onViewImage={(node) => setPreviewNodeId(node.id)}
                onExtractVideoFrames={NOOP}
                extractingVideoFrame={false}
                onReversePrompt={NOOP}
                onRetry={(node) => onGenerate(node.id, node.metadata?.composerContent || node.metadata?.prompt || '')}
                onToggleFreeResize={NOOP}
                onToggleLocked={(node) =>
                  onConfigChange(node.id, { locked: !node.metadata?.locked })
                }
                onSubtitles={NOOP}
                onTimeline={NOOP}
                onOpenDrawing={NOOP}
                onDelete={onDeleteNode}
              />

              {dialogNode && (dialogNode.type === CanvasNodeType.Video || dialogNode.type === CanvasNodeType.Image) ? (
                <CanvasNodePanelOverlay node={dialogNode} viewport={viewport} containerRef={containerRef} panelWidth={520}>
                  <div data-testid='shot-api-prompt'>
                    <CanvasNodePromptPanel
                      node={dialogNode}
                      isRunning={generating && dialogNode.id === VIDEO_NODE_ID}
                      mentionReferences={promptMentions}
                      onPromptChange={onPromptChange}
                      onConfigChange={onConfigChange}
                      onGenerate={(nodeId, _mode, prompt) => onGenerate(nodeId, prompt)}
                      onStop={NOOP}
                    />
                  </div>
                </CanvasNodePanelOverlay>
              ) : null}

              <div
                data-canvas-no-zoom
                className='absolute bottom-4 left-4 z-[var(--z-panel)] flex items-end gap-2'
                onMouseDown={(event) => event.stopPropagation()}
                onPointerDown={(event) => event.stopPropagation()}
              >
                <CanvasZoomControls
                  scale={viewport.k}
                  containerRef={containerRef}
                  onScaleChange={setZoomScale}
                  onReset={fitCanvas}
                  onAutoArrange={fitCanvas}
                  isMiniMapOpen={isMiniMapOpen}
                  onToggleMiniMap={() => setIsMiniMapOpen((value) => !value)}
                  onOpenShortcuts={NOOP}
                />
              </div>
              {isMiniMapOpen ? (
                <Minimap
                  nodes={nodes}
                  viewport={viewport}
                  viewportSize={viewportSize}
                  canvasContainerRef={containerRef}
                  onViewportChange={persistViewport}
                />
              ) : null}

              <div className={styles.shotCanvasChrome} data-canvas-no-zoom>
                {overlay ? <div className={styles.shotCanvasChips}>{overlay}</div> : null}
                <button
                  type='button'
                  className={styles.shotCanvasFullBtn}
                  data-testid='shot-canvas-fullscreen'
                  aria-label={
                    expanded
                      ? t('videoGeneration.studio.storyboard.exitCanvasFullscreen', {
                          defaultValue: '退出全屏',
                        })
                      : t('videoGeneration.studio.storyboard.canvasFullscreen', {
                          defaultValue: '全屏',
                        })
                  }
                  title={
                    expanded
                      ? t('videoGeneration.studio.storyboard.exitCanvasFullscreen', {
                          defaultValue: '退出全屏',
                        })
                      : t('videoGeneration.studio.storyboard.canvasFullscreen', {
                          defaultValue: '全屏',
                        })
                  }
                  onMouseDown={(event) => event.stopPropagation()}
                  onPointerDown={(event) => event.stopPropagation()}
                  onClick={() => onExpandedChange?.(!expanded)}
                >
                  {expanded ? <Minimize2 size={16} /> : <Maximize2 size={16} />}
                </button>
              </div>

              {menu ? (
                <ShotCanvasMenu
                  menu={menu}
                  node={menu.type === 'node' ? nodeById.get(menu.nodeId) ?? null : null}
                  theme={theme}
                  readOnly={readOnly}
                  canAddImage={canAddImage}
                  canAddAudio={canAddAudio}
                  imageLibrary={imageLibrary}
                  audioLibrary={audioLibrary}
                  onClose={() => setMenu(null)}
                  onAddImage={() => onAddImage(menu.type === 'canvas' ? menu.position : undefined)}
                  onAddAudio={() => onAddAudio(menu.type === 'canvas' ? menu.position : undefined)}
                  onFit={fitCanvas}
                  onUpload={() =>
                    onUploadNode(
                      menu.type === 'node' ? nodeById.get(menu.nodeId) ?? null : null,
                      menu.type === 'canvas' ? menu.position : undefined
                    )
                  }
                  onInfo={() => menu.type === 'node' && setInfoNodeId(menu.nodeId)}
                  onDialog={() => {
                    if (menu.type !== 'node') return;
                    const node = nodeById.get(menu.nodeId);
                    if (node) openNodeDialog(node);
                  }}
                  onPreview={() => menu.type === 'node' && setPreviewNodeId(menu.nodeId)}
                  onDelete={() => {
                    if (menu.type === 'node') {
                      const node = nodeById.get(menu.nodeId);
                      if (node) onDeleteNode(node);
                    }
                    if (menu.type === 'connection') onUnbindConnection(menu.connectionId);
                  }}
                  onBindLibrary={(path) => menu.type === 'node' && onBindLibrary(menu.nodeId, path)}
                />
              ) : null}

              <CanvasNodeInfoModal
                node={infoNode}
                open={Boolean(infoNode)}
                onClose={() => setInfoNodeId(null)}
                onMetadataChange={onConfigChange}
                readOnly={readOnly}
              />

              {previewNode ? (
                <button
                  type='button'
                  className={styles.shotCanvasLightbox}
                  onClick={() => setPreviewNodeId(null)}
                  aria-label={t('videoGeneration.studio.storyboard.closePreview', { defaultValue: '关闭预览' })}
                >
                  {previewNode.type === CanvasNodeType.Video ? (
                    <video src={canvasNodeDisplayUrl(previewNode)} controls autoPlay className={styles.shotCanvasLightboxMedia} />
                  ) : (
                    <img src={canvasNodeDisplayUrl(previewNode)} alt={previewNode.title} className={styles.shotCanvasLightboxMedia} />
                  )}
                </button>
              ) : null}
            </div>
          </CanvasNodeActionContext.Provider>
        </AntApp>
      </ConfigProvider>
    </div>
    </CanvasColorThemeScope>
  );
};

function ShotCanvasMenu({
  menu,
  node,
  theme,
  readOnly,
  canAddImage,
  canAddAudio,
  imageLibrary,
  audioLibrary,
  onClose,
  onAddImage,
  onAddAudio,
  onFit,
  onUpload,
  onInfo,
  onDialog,
  onPreview,
  onDelete,
  onBindLibrary,
}: {
  menu: ContextMenuState;
  node: CanvasNodeData | null;
  theme: (typeof canvasThemes)[keyof typeof canvasThemes];
  readOnly: boolean;
  canAddImage: boolean;
  canAddAudio: boolean;
  imageLibrary: LibraryItem[];
  audioLibrary: LibraryItem[];
  onClose: () => void;
  onAddImage: () => void;
  onAddAudio: () => void;
  onFit: () => void;
  onUpload: () => void;
  onInfo: () => void;
  onDialog: () => void;
  onPreview: () => void;
  onDelete: () => void;
  onBindLibrary: (path: string) => void;
}) {
  const run = (action: () => void) => {
    action();
    onClose();
  };
  const parsed = node ? parseShotNodeId(node.id) : null;
  const library = parsed?.kind === 'audio' ? audioLibrary : imageLibrary;
  const canDelete = parsed?.kind !== 'video';

  useEffect(() => {
    const close = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Element && target.closest('[data-canvas-context-menu]')) return;
      onClose();
    };
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  }, [onClose]);

  return (
    <CanvasOverlay
      theme={theme}
      data-canvas-context-menu
      className='fixed z-[var(--z-popover)] flex w-[220px] origin-top-left flex-col overflow-hidden p-1'
      style={{ left: menu.x, top: menu.y }}
      onContextMenu={(event) => event.preventDefault()}
      onPointerDown={(event) => event.stopPropagation()}
    >
      {menu.type === 'canvas' ? (
        <>
          <CanvasMenuRow
            icon={<ImageIcon />}
            label={canvasT('videoCanvas.node.image', '图片')}
            disabled={readOnly || !canAddImage}
            onClick={() => run(onAddImage)}
          />
          <CanvasMenuRow
            icon={<Music2 />}
            label={canvasT('videoCanvas.node.audio', '音频')}
            disabled={readOnly || !canAddAudio}
            onClick={() => run(onAddAudio)}
          />
          <CanvasMenuRow icon={<Upload />} label={canvasT('videoCanvas.menu.uploadHere', '上传到这里')} disabled={readOnly} onClick={() => run(onUpload)} />
          <CanvasMenuSeparator />
          <CanvasMenuRow icon={<Scan />} label={canvasT('videoCanvas.zoom.fitAll', '适应全部内容')} onClick={() => run(onFit)} />
        </>
      ) : null}
      {menu.type === 'node' && node ? (
        <>
          <CanvasMenuRow icon={<Info />} label={canvasT('videoCanvas.toolbar.info', '信息')} onClick={() => run(onInfo)} />
          {node.type === CanvasNodeType.Video || node.type === CanvasNodeType.Image ? (
            <CanvasMenuRow icon={<Maximize2 />} label={canvasT('videoCanvas.nodeUi.genSettings', '生成设置')} onClick={() => run(onDialog)} />
          ) : null}
          {node.metadata?.content ? (
            <CanvasMenuRow icon={<Maximize2 />} label={canvasT('videoCanvas.menu.fullscreenPreview', '进入全景预览')} onClick={() => run(onPreview)} />
          ) : null}
          <CanvasMenuRow icon={<Upload />} label={canvasT('videoCanvas.toolbar.uploadImage', '上传图片')} disabled={readOnly || node.id === VIDEO_NODE_ID} onClick={() => run(onUpload)} />
          {library.length > 0 && node.id !== VIDEO_NODE_ID ? (
            <>
              <CanvasMenuSeparator />
              <div className='px-2 py-1 text-[11px] opacity-50'>{canvasT('videoCanvas.menu.insertFromAssets', '从素材空间插入')}</div>
              {library.slice(0, 8).map((item) => (
                <CanvasMenuRow
                  key={item.path}
                  icon={<FolderOpen />}
                  label={item.label}
                  disabled={readOnly}
                  onClick={() => run(() => onBindLibrary(item.path))}
                />
              ))}
            </>
          ) : null}
          {canDelete ? (
            <>
              <CanvasMenuSeparator />
              <CanvasMenuRow icon={<Trash2 />} label={canvasT('videoCanvas.menu.deleteNode', '删除节点')} danger disabled={readOnly} onClick={() => run(onDelete)} />
            </>
          ) : null}
        </>
      ) : null}
      {menu.type === 'connection' ? (
        <CanvasMenuRow icon={<Trash2 />} label={canvasT('videoCanvas.menu.deleteConnection', '删除连线')} danger disabled={readOnly} onClick={() => run(onDelete)} />
      ) : null}
    </CanvasOverlay>
  );
}

export default ShotInfiniteCanvas;
