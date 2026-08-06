/**
 * Infinite canvas shell — DOM nodes + SVG connections (open-ai-canvas architecture
 * rewritten without Leafer; pan/zoom via viewport transform).
 */

import React, { useCallback, useMemo, useRef, useState } from 'react';
import classNames from 'classnames';
import { Button, Dropdown, Input, Select, Space, Typography } from '@arco-design/web-react';
import {
  AddOne,
  Delete,
  FullScreen,
  Pic,
  Play,
  Text,
  VideoTwo,
  Voice,
  Connect,
} from '@icon-park/react';
import type {
  CanvasConnection,
  CanvasDocument,
  CanvasNodeData,
  CanvasNodeType,
  ViewportTransform,
} from '../types';
import { defaultNodeSize, newNodeId } from '../types';
import { canvasMediaUrl } from '../api';
import { CAMERA_MOVE_PRESETS } from '../lib/cameraPresets';
import { runNodeGeneration, cancelNodeGeneration } from '../lib/runGeneration';
import { useMediaModels } from '@renderer/hooks/agent/useMediaModels';
import styles from './CanvasEditor.module.css';

const { TextArea } = Input;

type Props = {
  doc: CanvasDocument;
  onChange: (doc: CanvasDocument) => void;
  busy?: boolean;
};

type DragState =
  | { kind: 'pan'; startX: number; startY: number; origin: ViewportTransform }
  | {
      kind: 'node';
      nodeId: string;
      startX: number;
      startY: number;
      originPos: { x: number; y: number };
    }
  | {
      kind: 'connect';
      fromNodeId: string;
      x: number;
      y: number;
    };

const CanvasEditor: React.FC<Props> = ({ doc, onChange, busy }) => {
  const surfaceRef = useRef<HTMLDivElement>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [runningId, setRunningId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { imageModels, videoModels } = useMediaModels();

  const selected = useMemo(
    () => doc.nodes.find((n) => n.id === selectedId) ?? null,
    [doc.nodes, selectedId]
  );

  const updateViewport = useCallback(
    (viewport: ViewportTransform) => {
      onChange({ ...doc, viewport });
    },
    [doc, onChange]
  );

  const updateNode = useCallback(
    (nodeId: string, patch: Partial<CanvasNodeData>) => {
      onChange({
        ...doc,
        nodes: doc.nodes.map((n) => (n.id === nodeId ? { ...n, ...patch } : n)),
      });
    },
    [doc, onChange]
  );

  const updateNodeMeta = useCallback(
    (nodeId: string, meta: Record<string, unknown>) => {
      onChange({
        ...doc,
        nodes: doc.nodes.map((n) =>
          n.id === nodeId
            ? { ...n, metadata: { ...(n.metadata ?? {}), ...meta } }
            : n
        ),
      });
    },
    [doc, onChange]
  );

  const addNode = useCallback(
    (type: CanvasNodeType) => {
      const size = defaultNodeSize(type);
      const vp = doc.viewport;
      const worldX = (-vp.x + 120) / vp.k;
      const worldY = (-vp.y + 100) / vp.k;
      const titles: Record<CanvasNodeType, string> = {
        text: '文本',
        image: '图片',
        video: '视频',
        audio: '音频',
        config: '生成配置',
        script: '分镜脚本',
        frame: '分组框',
        drawing: '绘图',
        skill: '技能',
      };
      const node: CanvasNodeData = {
        id: newNodeId(),
        type,
        title: titles[type],
        position: {
          x: worldX + doc.nodes.length * 24,
          y: worldY + doc.nodes.length * 16,
        },
        width: size.width,
        height: size.height,
        metadata: {
          status: 'idle',
          generationMode: type === 'video' ? 'video' : type === 'config' ? 'video' : 'image',
          prompt: '',
          content: type === 'text' ? '' : undefined,
          size: '16:9',
          seconds: '5',
          vquality: '720p',
          videoEditOperation: 'text_to_video',
          storyboard:
            type === 'script'
              ? {
                  rows: [
                    {
                      id: newNodeId(),
                      shotNumber: 1,
                      durationSeconds: 5,
                      plotDescription: '',
                      dialogue: '',
                      imageGenerationPrompt: '',
                      videoMotionPrompt: '',
                    },
                  ],
                }
              : undefined,
        },
      };
      onChange({ ...doc, nodes: [...doc.nodes, node] });
      setSelectedId(node.id);
    },
    [doc, onChange]
  );

  const deleteSelected = useCallback(() => {
    if (!selectedId) return;
    onChange({
      ...doc,
      nodes: doc.nodes.filter((n) => n.id !== selectedId),
      connections: doc.connections.filter(
        (c) => c.fromNodeId !== selectedId && c.toNodeId !== selectedId
      ),
    });
    setSelectedId(null);
  }, [doc, onChange, selectedId]);

  const addConnection = useCallback(
    (fromNodeId: string, toNodeId: string) => {
      if (fromNodeId === toNodeId) return;
      if (
        doc.connections.some((c) => c.fromNodeId === fromNodeId && c.toNodeId === toNodeId)
      ) {
        return;
      }
      const conn: CanvasConnection = {
        id: newNodeId(),
        fromNodeId,
        toNodeId,
      };
      onChange({ ...doc, connections: [...doc.connections, conn] });
    },
    [doc, onChange]
  );

  const onWheel = useCallback(
    (e: React.WheelEvent) => {
      e.preventDefault();
      const factor = e.deltaY > 0 ? 0.92 : 1.08;
      const nextK = Math.min(2.5, Math.max(0.25, doc.viewport.k * factor));
      updateViewport({ ...doc.viewport, k: nextK });
    },
    [doc.viewport, updateViewport]
  );

  const onPointerDownBackground = useCallback(
    (e: React.PointerEvent) => {
      if (e.button !== 0 && e.button !== 1) return;
      if ((e.target as HTMLElement).closest('[data-canvas-node]')) return;
      setSelectedId(null);
      setDrag({
        kind: 'pan',
        startX: e.clientX,
        startY: e.clientY,
        origin: { ...doc.viewport },
      });
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    },
    [doc.viewport]
  );

  const onPointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!drag) return;
      if (drag.kind === 'pan') {
        updateViewport({
          ...drag.origin,
          x: drag.origin.x + (e.clientX - drag.startX),
          y: drag.origin.y + (e.clientY - drag.startY),
        });
      } else if (drag.kind === 'node') {
        const dx = (e.clientX - drag.startX) / doc.viewport.k;
        const dy = (e.clientY - drag.startY) / doc.viewport.k;
        updateNode(drag.nodeId, {
          position: { x: drag.originPos.x + dx, y: drag.originPos.y + dy },
        });
      } else if (drag.kind === 'connect') {
        setDrag({ ...drag, x: e.clientX, y: e.clientY });
      }
    },
    [drag, doc.viewport.k, updateNode, updateViewport]
  );

  const onPointerUp = useCallback(
    (e: React.PointerEvent) => {
      if (drag?.kind === 'connect') {
        const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
        const target = el?.closest('[data-canvas-node]') as HTMLElement | null;
        const toId = target?.dataset.nodeId;
        if (toId) addConnection(drag.fromNodeId, toId);
      }
      setDrag(null);
    },
    [addConnection, drag]
  );

  const handleRun = useCallback(
    async (nodeId: string) => {
      if (runningId) return;
      setRunningId(nodeId);
      setError(null);
      updateNodeMeta(nodeId, { status: 'loading', taskProgress: 0 });
      try {
        const result = await runNodeGeneration({
          doc,
          nodeId,
          onProgress: (t) => {
            updateNodeMeta(nodeId, {
              status: 'loading',
              taskId: t.task_id,
              taskProgress: t.progress,
            });
          },
        });
        let nextNodes = doc.nodes.map((n) =>
          n.id === nodeId ? result.updatedNode : n
        );
        let nextConnections = doc.connections;
        if (result.spawnedNode) {
          nextNodes = [...nextNodes, result.spawnedNode];
          nextConnections = [
            ...nextConnections,
            {
              id: newNodeId(),
              fromNodeId: nodeId,
              toNodeId: result.spawnedNode.id,
            },
          ];
        }
        onChange({ ...doc, nodes: nextNodes, connections: nextConnections });
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setError(msg);
        updateNodeMeta(nodeId, { status: 'error', errorDetails: msg });
      } finally {
        setRunningId(null);
      }
    },
    [doc, onChange, runningId, updateNodeMeta]
  );

  const addMenu = [
    { key: 'config', label: '生成配置', onClick: () => addNode('config') },
    { key: 'text', label: '文本', onClick: () => addNode('text') },
    { key: 'image', label: '图片', onClick: () => addNode('image') },
    { key: 'video', label: '视频', onClick: () => addNode('video') },
    { key: 'audio', label: '音频', onClick: () => addNode('audio') },
    { key: 'script', label: '分镜脚本', onClick: () => addNode('script') },
    { key: 'frame', label: '分组框', onClick: () => addNode('frame') },
  ];

  const vp = doc.viewport;

  return (
    <div className={styles.root}>
      <div className={styles.toolbar}>
        <Space size={8}>
          <Dropdown
            droplist={
              <div className={styles.menu}>
                {addMenu.map((item) => (
                  <button
                    key={item.key}
                    type='button'
                    className={styles.menuItem}
                    onClick={item.onClick}
                  >
                    {item.label}
                  </button>
                ))}
              </div>
            }
            trigger='click'
            position='bl'
          >
            <Button type='primary' size='small' icon={<AddOne theme='outline' size={14} />}>
              添加节点
            </Button>
          </Dropdown>
          <Button
            size='small'
            status='danger'
            disabled={!selectedId}
            icon={<Delete theme='outline' size={14} />}
            onClick={deleteSelected}
          >
            删除
          </Button>
          <Button
            size='small'
            icon={<FullScreen theme='outline' size={14} />}
            onClick={() => updateViewport({ x: 0, y: 0, k: 1 })}
          >
            重置视图
          </Button>
        </Space>
        <Typography.Text type='secondary' className={styles.hint}>
          拖动画布平移 · 滚轮缩放 · 从节点右侧圆点拖出连线
        </Typography.Text>
      </div>

      {error ? <div className={styles.errorBanner}>{error}</div> : null}

      <div
        ref={surfaceRef}
        className={classNames(styles.surface, styles[`bg_${doc.backgroundMode}`])}
        onWheel={onWheel}
        onPointerDown={onPointerDownBackground}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      >
        <div
          className={styles.world}
          style={{
            transform: `translate(${vp.x}px, ${vp.y}px) scale(${vp.k})`,
          }}
        >
          <svg className={styles.edges} width='100%' height='100%'>
            {doc.connections.map((c) => {
              const from = doc.nodes.find((n) => n.id === c.fromNodeId);
              const to = doc.nodes.find((n) => n.id === c.toNodeId);
              if (!from || !to) return null;
              const a = { x: from.position.x + from.width, y: from.position.y + from.height / 2 };
              const b = { x: to.position.x, y: to.position.y + to.height / 2 };
              const mx = (a.x + b.x) / 2;
              return (
                <path
                  key={c.id}
                  d={`M ${a.x} ${a.y} C ${mx} ${a.y}, ${mx} ${b.y}, ${b.x} ${b.y}`}
                  className={styles.edgePath}
                  fill='none'
                />
              );
            })}
          </svg>

          {doc.nodes.map((node) => (
            <div
              key={node.id}
              data-canvas-node
              data-node-id={node.id}
              className={classNames(styles.node, {
                [styles.nodeSelected]: selectedId === node.id,
                [styles.nodeFrame]: node.type === 'frame',
              })}
              style={{
                left: node.position.x,
                top: node.position.y,
                width: node.width,
                minHeight: node.height,
              }}
              onPointerDown={(e) => {
                e.stopPropagation();
                setSelectedId(node.id);
                setDrag({
                  kind: 'node',
                  nodeId: node.id,
                  startX: e.clientX,
                  startY: e.clientY,
                  originPos: { ...node.position },
                });
                (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
              }}
            >
              <div className={styles.nodeHeader}>
                <span className={styles.nodeIcon}>
                  {node.type === 'video' ? (
                    <VideoTwo theme='outline' size={14} />
                  ) : node.type === 'image' ? (
                    <Pic theme='outline' size={14} />
                  ) : node.type === 'text' ? (
                    <Text theme='outline' size={14} />
                  ) : node.type === 'audio' ? (
                    <Voice theme='outline' size={14} />
                  ) : (
                    <Connect theme='outline' size={14} />
                  )}
                </span>
                <span className={styles.nodeTitle}>{node.title}</span>
                <span className={styles.nodeType}>{node.type}</span>
              </div>

              <div className={styles.nodeBody} onPointerDown={(e) => e.stopPropagation()}>
                {(node.type === 'text' || node.type === 'config') && (
                  <TextArea
                    autoSize={{ minRows: 3, maxRows: 8 }}
                    placeholder={node.type === 'text' ? '输入文本 / 提示词' : '生成提示词'}
                    value={
                      node.type === 'text'
                        ? node.metadata?.content ?? ''
                        : node.metadata?.prompt ?? ''
                    }
                    onChange={(v) =>
                      updateNodeMeta(
                        node.id,
                        node.type === 'text' ? { content: v } : { prompt: v }
                      )
                    }
                  />
                )}

                {node.type === 'config' && (
                  <div className={styles.configGrid}>
                    <Select
                      size='small'
                      value={node.metadata?.generationMode ?? 'video'}
                      onChange={(v) => updateNodeMeta(node.id, { generationMode: v })}
                      options={[
                        { label: '视频', value: 'video' },
                        { label: '图片', value: 'image' },
                      ]}
                    />
                    <Select
                      size='small'
                      placeholder='模型'
                      value={node.metadata?.model}
                      onChange={(v) => updateNodeMeta(node.id, { model: v })}
                      options={(
                        (node.metadata?.generationMode ?? 'video') === 'video'
                          ? videoModels
                          : imageModels
                      ).map((m) => ({
                        label: m.name || m.id,
                        value: m.id,
                      }))}
                      allowClear
                    />
                    <Select
                      size='small'
                      value={node.metadata?.size ?? '16:9'}
                      onChange={(v) => updateNodeMeta(node.id, { size: v })}
                      options={['16:9', '9:16', '1:1', '4:3', '3:4'].map((r) => ({
                        label: r,
                        value: r,
                      }))}
                    />
                    {(node.metadata?.generationMode ?? 'video') === 'video' && (
                      <>
                        <Select
                          size='small'
                          value={node.metadata?.seconds ?? '5'}
                          onChange={(v) => updateNodeMeta(node.id, { seconds: v })}
                          options={['4', '5', '6', '8', '10', '12', '15'].map((s) => ({
                            label: `${s}s`,
                            value: s,
                          }))}
                        />
                        <Select
                          size='small'
                          value={node.metadata?.vquality ?? '720p'}
                          onChange={(v) => updateNodeMeta(node.id, { vquality: v })}
                          options={['480p', '720p', '1080p'].map((s) => ({
                            label: s,
                            value: s,
                          }))}
                        />
                        <Select
                          size='small'
                          value={node.metadata?.videoEditOperation ?? 'text_to_video'}
                          onChange={(v) => updateNodeMeta(node.id, { videoEditOperation: v })}
                          options={[
                            { label: '文生视频', value: 'text_to_video' },
                            { label: '图生视频', value: 'image_to_video' },
                            { label: '运镜', value: 'camera_motion' },
                            { label: '拼接成片', value: 'concat' },
                          ]}
                        />
                        {node.metadata?.videoEditOperation === 'camera_motion' && (
                          <Select
                            size='small'
                            placeholder='运镜预设'
                            onChange={(id) => {
                              const preset = CAMERA_MOVE_PRESETS.find((p) => p.id === id);
                              if (!preset) return;
                              const base = node.metadata?.prompt?.trim() || '';
                              updateNodeMeta(node.id, {
                                prompt: base
                                  ? `${base}\n${preset.prompt}`
                                  : preset.prompt,
                              });
                            }}
                            options={CAMERA_MOVE_PRESETS.map((p) => ({
                              label: p.label,
                              value: p.id,
                            }))}
                          />
                        )}
                      </>
                    )}
                    <Button
                      type='primary'
                      size='small'
                      long
                      loading={runningId === node.id}
                      disabled={!!busy || !!runningId}
                      icon={<Play theme='outline' size={14} />}
                      onClick={() => void handleRun(node.id)}
                    >
                      {node.metadata?.videoEditOperation === 'concat' ? '拼接' : '生成'}
                    </Button>
                    {runningId === node.id && node.metadata?.taskId ? (
                      <Button
                        size='small'
                        status='warning'
                        long
                        onClick={() => {
                          void cancelNodeGeneration(node.metadata!.taskId!).then(() => {
                            setRunningId(null);
                            updateNodeMeta(node.id, { status: 'idle', taskProgress: 0 });
                          });
                        }}
                      >
                        取消
                      </Button>
                    ) : null}
                  </div>
                )}

                {node.type === 'image' && node.metadata?.mediaId && (
                  <img
                    className={styles.media}
                    src={canvasMediaUrl(node.metadata.mediaId)}
                    alt={node.title}
                    draggable={false}
                  />
                )}
                {node.type === 'video' && node.metadata?.mediaId && (
                  <video
                    className={styles.media}
                    src={canvasMediaUrl(node.metadata.mediaId)}
                    controls
                    playsInline
                  />
                )}
                {(node.type === 'image' || node.type === 'video') && !node.metadata?.mediaId && (
                  <div className={styles.emptyMedia}>空槽 — 连接配置节点生成，或上传素材</div>
                )}

                {node.type === 'script' && node.metadata?.storyboard && (
                  <div className={styles.storyboard}>
                    {node.metadata.storyboard.rows.map((row, idx) => (
                      <div key={row.id} className={styles.shotRow}>
                        <Typography.Text bold>镜头 {row.shotNumber}</Typography.Text>
                        <Input
                          size='small'
                          placeholder='情节描述'
                          value={row.plotDescription}
                          onChange={(v) => {
                            const rows = [...(node.metadata?.storyboard?.rows ?? [])];
                            rows[idx] = { ...row, plotDescription: v };
                            updateNodeMeta(node.id, { storyboard: { rows } });
                          }}
                        />
                        <Input
                          size='small'
                          placeholder='画面提示词'
                          value={row.imageGenerationPrompt}
                          onChange={(v) => {
                            const rows = [...(node.metadata?.storyboard?.rows ?? [])];
                            rows[idx] = { ...row, imageGenerationPrompt: v };
                            updateNodeMeta(node.id, { storyboard: { rows } });
                          }}
                        />
                      </div>
                    ))}
                    <Button
                      size='mini'
                      onClick={() => {
                        const rows = [...(node.metadata?.storyboard?.rows ?? [])];
                        rows.push({
                          id: newNodeId(),
                          shotNumber: rows.length + 1,
                          durationSeconds: 5,
                          plotDescription: '',
                          dialogue: '',
                          imageGenerationPrompt: '',
                          videoMotionPrompt: '',
                        });
                        updateNodeMeta(node.id, { storyboard: { rows } });
                      }}
                    >
                      添加镜头
                    </Button>
                  </div>
                )}

                {node.metadata?.status === 'loading' && (
                  <div className={styles.progress}>
                    生成中… {Math.round((node.metadata.taskProgress ?? 0) * 100)}%
                  </div>
                )}
                {node.metadata?.status === 'error' && (
                  <div className={styles.nodeError}>{node.metadata.errorDetails}</div>
                )}
              </div>

              <button
                type='button'
                className={styles.handleOut}
                title='拖出连线'
                onPointerDown={(e) => {
                  e.stopPropagation();
                  setDrag({
                    kind: 'connect',
                    fromNodeId: node.id,
                    x: e.clientX,
                    y: e.clientY,
                  });
                  (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
                }}
              />
            </div>
          ))}
        </div>
      </div>
    </div>
  );
};

export default CanvasEditor;
