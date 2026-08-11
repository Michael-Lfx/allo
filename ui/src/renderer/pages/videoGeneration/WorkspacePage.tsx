/**
 * Montage Backlot workspace (`/video-generation/:sessionId`).
 *
 * Stage rail · artifact JSON editor · HITL approval · start/cancel/export/canvas/TV Show.
 */
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Button,
  Input,
  Popconfirm,
  Result,
  Select,
  Spin,
  Tag,
} from '@arco-design/web-react';
import {
  ArrowLeft,
  Delete,
  Export,
  Play,
  Refresh,
  Share,
  Pause,
  Cube,
} from '@icon-park/react';
import { ipcBridge } from '@/common';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { isDesktopShell } from '@renderer/utils/platform';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import {
  cancelProject,
  deleteProject,
  exportProject,
  getArtifact,
  getBoardState,
  getProject,
  getProjectStatus,
  isActiveStatus,
  listArtifacts,
  materializeProjectToCanvas,
  publishProjectToTvShow,
  startProject,
  submitApproval,
} from './api';
import type {
  ArtifactContent,
  BoardState,
  MontageRunStatus,
  ProjectDetail,
  RunStatus,
} from './types';
import ArtifactPreviewPanel from './components/ArtifactPreviewPanel';
import ProjectMediaPlayer from './components/ProjectMediaPlayer';
import StudioStageRail from './components/StudioStageRail';
import { modeLabel, pipelineLabel, statusLabel, statusTagColor } from './components/SessionCard';
import { stageLabel } from './stageI18n';
import {
  clearVideoGenerationSessionMemory,
  rememberVideoGenerationSession,
} from './routeMemory';
import { isInsufficientCreditsError } from './creditsError';
import styles from './index.module.css';

const TextArea = Input.TextArea;
const POLL_MS = 1500;

const WorkspacePage: React.FC = () => {
  const { sessionId: projectId = '' } = useParams<{ sessionId: string }>();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const [message, messageHolder] = useArcoMessage();
  const { status: cloudStatus } = useCloudAuth();

  const [detail, setDetail] = useState<ProjectDetail | null>(null);
  const [board, setBoard] = useState<BoardState | null>(null);
  const [runStatus, setRunStatus] = useState<RunStatus | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const [artifactNames, setArtifactNames] = useState<string[]>([]);
  const [selectedArtifact, setSelectedArtifact] = useState<string | null>(null);
  const [preview, setPreview] = useState<ArtifactContent | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);

  const [starting, setStarting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [materializing, setMaterializing] = useState(false);
  const [approving, setApproving] = useState(false);
  const [approvalNote, setApprovalNote] = useState('');
  const [sendBackTo, setSendBackTo] = useState<string | undefined>(undefined);

  const record = detail?.record ?? null;
  const status = (runStatus?.status ?? board?.status ?? 'idle') as MontageRunStatus | string;
  const busy = isActiveStatus(status) || runStatus?.is_job_running === true;
  const awaitingStage = runStatus?.awaiting_human_stage ?? board?.awaiting_human_stage ?? null;

  /** Stages strictly before the awaiting stage — valid `send_back_to` targets. */
  const sendBackTargets = useMemo(() => {
    if (!board?.stages?.length || !awaitingStage) return [];
    const idx = board.stages.findIndex((s) => s.name === awaitingStage);
    if (idx <= 0) return [];
    return board.stages.slice(0, idx);
  }, [awaitingStage, board?.stages]);

  useEffect(() => {
    if (sendBackTargets.length === 0) {
      setSendBackTo(undefined);
      return;
    }
    setSendBackTo((prev) => {
      if (prev && sendBackTargets.some((s) => s.name === prev)) return prev;
      return sendBackTargets[sendBackTargets.length - 1]?.name;
    });
  }, [sendBackTargets]);

  const loadCore = useCallback(async () => {
    if (!projectId) return;
    const [project, boardState, statusRes, artifacts] = await Promise.all([
      getProject(projectId),
      getBoardState(projectId),
      getProjectStatus(projectId),
      listArtifacts(projectId),
    ]);
    setDetail(project);
    setBoard(boardState);
    setRunStatus(statusRes);
    setArtifactNames(artifacts);
    rememberVideoGenerationSession(projectId, project.record.title);
    setLoadError(null);

    const current = boardState.current_stage;
    const preferred =
      boardState.stages.find((s) => s.name === current)?.produces[0] ??
      boardState.stages.find((s) => s.produces.length > 0)?.produces[0] ??
      artifacts[0] ??
      null;
    setSelectedArtifact((prev) => {
      if (prev && artifacts.includes(prev)) return prev;
      return preferred && artifacts.includes(preferred) ? preferred : preferred;
    });
  }, [projectId]);

  useEffect(() => {
    if (!projectId) return;
    let cancelled = false;
    setLoading(true);
    void loadCore()
      .catch((e) => {
        if (!cancelled) {
          setLoadError(e instanceof Error ? e.message : String(e));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [projectId, loadCore]);

  // Status / board polling while active or awaiting human.
  useEffect(() => {
    if (!projectId) return;
    const shouldPoll = busy || status === 'awaiting_human';
    if (!shouldPoll) return;
    const timer = window.setInterval(() => {
      void (async () => {
        try {
          const [statusRes, boardState, artifacts] = await Promise.all([
            getProjectStatus(projectId),
            getBoardState(projectId),
            listArtifacts(projectId),
          ]);
          setRunStatus(statusRes);
          setBoard(boardState);
          setArtifactNames(artifacts);
        } catch {
          /* keep last good state */
        }
      })();
    }, POLL_MS);
    return () => window.clearInterval(timer);
  }, [projectId, busy, status]);

  useEffect(() => {
    if (!projectId || !selectedArtifact) {
      setPreview(null);
      return;
    }
    let cancelled = false;
    setPreviewLoading(true);
    void getArtifact(projectId, selectedArtifact)
      .then((content) => {
        if (!cancelled) setPreview(content);
      })
      .catch(() => {
        if (!cancelled) {
          setPreview({
            kind: 'text',
            text: '',
          });
        }
      })
      .finally(() => {
        if (!cancelled) setPreviewLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [projectId, selectedArtifact, artifactNames]);

  const selectStage = useCallback(
    (stageName: string) => {
      const stage = board?.stages.find((s) => s.name === stageName);
      const produce = stage?.produces[0];
      if (produce) {
        setSelectedArtifact(produce);
        return;
      }
      if (artifactNames.includes(stageName)) {
        setSelectedArtifact(stageName);
      }
    },
    [board, artifactNames]
  );

  const handleStart = useCallback(async () => {
    if (!projectId || starting || busy) return;
    setStarting(true);
    try {
      await startProject(projectId);
      trackFunnelEvent('first_task_started', {
        feature: 'video_generation',
        pipeline: record?.pipeline ?? null,
        project_id: projectId,
      });
      message.success(
        t('videoGeneration.workspace.startOk', { defaultValue: '已开始运行' })
      );
      await loadCore();
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      message.error(
        isInsufficientCreditsError(raw)
          ? t('videoGeneration.workspace.failure.creditsToast', {
              defaultValue: '积分不足，请充值后重试。',
            })
          : `${t('videoGeneration.workspace.startFailed', { defaultValue: '启动失败' })}: ${raw}`
      );
    } finally {
      setStarting(false);
    }
  }, [busy, loadCore, message, projectId, record?.pipeline, starting, t]);

  const handleCancel = useCallback(async () => {
    if (!projectId || cancelling) return;
    setCancelling(true);
    try {
      await cancelProject(projectId);
      message.success(t('videoGeneration.workspace.cancelOk', { defaultValue: '已取消' }));
      await loadCore();
    } catch (e) {
      message.error(
        `${t('videoGeneration.workspace.cancelFailed', { defaultValue: '取消失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setCancelling(false);
    }
  }, [cancelling, loadCore, message, projectId, t]);

  const handleDelete = useCallback(async () => {
    if (!projectId || deleting) return;
    setDeleting(true);
    try {
      await deleteProject(projectId);
      clearVideoGenerationSessionMemory(projectId);
      message.success(t('videoGeneration.actions.deleteOk', { defaultValue: '已删除任务' }));
      navigate('/video-generation');
    } catch (e) {
      message.error(
        `${t('videoGeneration.actions.deleteFailed', { defaultValue: '删除失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setDeleting(false);
    }
  }, [deleting, message, navigate, projectId, t]);

  const handleExport = useCallback(async () => {
    if (!projectId || exporting) return;
    if (!isDesktopShell()) {
      message.info(
        t('videoGeneration.actions.exportDesktopOnly', {
          defaultValue: '导出工程仅桌面端可用。',
        })
      );
      return;
    }
    const safeTitle = (record?.title || 'montage').replace(/[\\/:*?"<>|]/g, '_');
    const paths = await ipcBridge.dialog.showSave.invoke({
      defaultPath: `${safeTitle}.nomimontage`,
      filters: [
        {
          name: t('videoGeneration.actions.exportFilter', { defaultValue: 'Flowy 视频工程' }),
          extensions: ['nomimontage'],
        },
      ],
    });
    const dest = typeof paths === 'string' ? paths : null;
    if (!dest) return;
    setExporting(true);
    try {
      await exportProject(projectId, dest);
      message.success(t('videoGeneration.actions.exportOk', { defaultValue: '工程已导出' }));
    } catch (e) {
      message.error(
        `${t('videoGeneration.actions.exportFailed', { defaultValue: '导出失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setExporting(false);
    }
  }, [exporting, message, projectId, record?.title, t]);

  const handleMaterialize = useCallback(async () => {
    if (!projectId || materializing) return;
    setMaterializing(true);
    try {
      const result = await materializeProjectToCanvas(projectId);
      message.success(
        result.reused
          ? t('videoGeneration.actions.openInCanvasReused', {
              defaultValue: '已打开关联画布',
            })
          : t('videoGeneration.actions.openInCanvasOk', {
              defaultValue: '已物化到创作画布',
            })
      );
      navigate(`/video-generation/canvas/${encodeURIComponent(result.project_id)}`);
    } catch (e) {
      message.error(
        `${t('videoGeneration.actions.openInCanvasFailed', { defaultValue: '打开画布失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setMaterializing(false);
    }
  }, [materializing, message, navigate, projectId, t]);

  const handlePublish = useCallback(async () => {
    if (!projectId || publishing) return;
    if (cloudStatus !== 'authenticated') {
      message.info(
        t('videoGeneration.tvShow.authRequired.publish', {
          defaultValue: '发布到 TV Show 需要先登录云端账号。',
        })
      );
      return;
    }
    if (status !== 'succeeded') {
      message.info(
        t('videoGeneration.tvShow.publish.needSucceeded', {
          defaultValue: '请先完成成片生成后再发布。',
        })
      );
      return;
    }
    setPublishing(true);
    try {
      await publishProjectToTvShow(projectId, { title: record?.title });
      message.success(
        t('videoGeneration.tvShow.publish.ok', {
          defaultValue: '已提交审核，通过后会出现在 TV Show 广场。',
        })
      );
    } catch (e) {
      message.error(
        `${t('videoGeneration.tvShow.publish.failed', { defaultValue: '发布失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setPublishing(false);
    }
  }, [cloudStatus, message, projectId, publishing, record?.title, status, t]);

  const handleApproval = useCallback(
    async (decision: 'approve' | 'reject' | 'send_back') => {
      if (!projectId || !awaitingStage || approving) return;
      if (decision === 'send_back' && !sendBackTo) {
        message.error(
          t('videoGeneration.workspace.approval.sendBackNeedTarget', {
            defaultValue: '请选择要打回的阶段',
          })
        );
        return;
      }
      setApproving(true);
      try {
        // MontageService::approve auto-resumes when status becomes Idle — do not call start again.
        const next = await submitApproval(projectId, {
          stage: awaitingStage,
          decision,
          note: approvalNote.trim() || undefined,
          send_back_to: decision === 'send_back' ? sendBackTo : undefined,
        });
        setBoard(next);
        setApprovalNote('');
        await loadCore();
        message.success(
          t('videoGeneration.workspace.approval.ok', { defaultValue: '审批已提交' })
        );
      } catch (e) {
        message.error(
          `${t('videoGeneration.workspace.approval.failed', { defaultValue: '审批失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setApproving(false);
      }
    },
    [approvalNote, approving, awaitingStage, loadCore, message, projectId, sendBackTo, t]
  );

  const errorText = runStatus?.last_error ?? board?.last_error ?? null;

  const recentEvents = useMemo(
    () => (board?.recent_events ?? []).slice(-8).reverse(),
    [board?.recent_events]
  );

  if (!projectId) {
    return (
      <Result
        status='error'
        title={t('videoGeneration.workspace.missingId', { defaultValue: '缺少项目 ID' })}
      />
    );
  }

  if (loading) {
    return (
      <div className='flex flex-1 items-center justify-center'>
        <Spin />
      </div>
    );
  }

  if (loadError || !record) {
    return (
      <Result
        status='error'
        title={t('videoGeneration.workspace.loadError', { defaultValue: '加载失败' })}
        subTitle={loadError ?? undefined}
        extra={
          <Button onClick={() => navigate('/video-generation')}>
            {t('videoGeneration.workspace.back', { defaultValue: '返回列表' })}
          </Button>
        }
      />
    );
  }

  return (
    <div
      className={[
        styles.page,
        'flex-1 min-h-0 size-full box-border overflow-y-auto',
        isMobile ? 'px-12px py-12px' : 'px-16px py-20px md:px-28px',
      ].join(' ')}
    >
      {messageHolder}
      <div className='mx-auto flex w-full max-w-1280px box-border flex-col gap-16px'>
        <header className='flex flex-wrap items-start justify-between gap-12px'>
          <div className='min-w-0 flex flex-1 flex-col gap-8px'>
            <div className='flex flex-wrap items-center gap-8px'>
              <Button
                type='text'
                size='small'
                icon={<ArrowLeft theme='outline' size={16} />}
                onClick={() => navigate('/video-generation')}
              >
                {t('videoGeneration.workspace.back', { defaultValue: '返回' })}
              </Button>
              <Tag size='small' color={statusTagColor(status)}>
                {statusLabel(status, t)}
              </Tag>
              <Tag size='small'>{modeLabel(record.mode, t)}</Tag>
              <Tag size='small' color='arcoblue'>
                {pipelineLabel(record.pipeline, t)}
              </Tag>
            </div>
            <h1 className='m-0 truncate text-20px font-650 text-[var(--color-text-1)]'>
              {record.title || t('videoGeneration.list.untitled', { defaultValue: '未命名任务' })}
            </h1>
            <p className='m-0 text-12px text-[var(--color-text-3)]'>
              {stageLabel(runStatus?.current_stage ?? board?.current_stage, t) ||
                t('videoGeneration.workspace.idleHint', {
                  defaultValue: '尚未开始。确认提示词后点击开始。',
                })}
            </p>
          </div>

          <div className='flex flex-wrap items-center gap-8px'>
            <Button
              type='primary'
              size='small'
              loading={starting}
              disabled={busy || status === 'succeeded'}
              icon={<Play theme='outline' size={14} />}
              onClick={() => void handleStart()}
            >
              {status === 'idle' || status === 'awaiting_human' || status === 'failed' || status === 'cancelled'
                ? t('videoGeneration.workspace.start', { defaultValue: '开始 / 继续' })
                : t('videoGeneration.workspace.running', { defaultValue: '运行中' })}
            </Button>
            {busy ? (
              <Button
                size='small'
                status='warning'
                loading={cancelling}
                icon={<Pause theme='outline' size={14} />}
                onClick={() => void handleCancel()}
              >
                {t('videoGeneration.workspace.cancel', { defaultValue: '取消' })}
              </Button>
            ) : null}
            <Button
              size='small'
              type='outline'
              loading={materializing}
              icon={<Cube theme='outline' size={14} />}
              onClick={() => void handleMaterialize()}
            >
              {t('videoGeneration.actions.openInCanvas', { defaultValue: '打开到画布' })}
            </Button>
            <Button
              size='small'
              type='outline'
              loading={publishing}
              icon={<Share theme='outline' size={14} />}
              onClick={() => void handlePublish()}
            >
              {t('videoGeneration.tvShow.publish.action', { defaultValue: '发布到 TV Show' })}
            </Button>
            <Button
              size='small'
              type='outline'
              loading={exporting}
              icon={<Export theme='outline' size={14} />}
              onClick={() => void handleExport()}
            >
              {t('videoGeneration.actions.exportProject', { defaultValue: '导出' })}
            </Button>
            <Button
              size='small'
              type='text'
              icon={<Refresh theme='outline' size={14} />}
              onClick={() => void loadCore()}
            >
              {t('videoGeneration.list.retry', { defaultValue: '刷新' })}
            </Button>
            <Popconfirm
              title={t('videoGeneration.actions.deleteConfirm', {
                defaultValue: '确定删除该任务？产物将一并清除。',
              })}
              onOk={() => void handleDelete()}
            >
              <Button size='small' status='danger' loading={deleting} icon={<Delete theme='outline' size={14} />}>
                {t('videoGeneration.actions.delete', { defaultValue: '删除' })}
              </Button>
            </Popconfirm>
          </div>
        </header>

        <StudioStageRail
          stages={board?.stages ?? []}
          currentStage={board?.current_stage}
          awaitingHumanStage={awaitingStage}
          selectedStage={
            board?.stages.find((s) => s.produces.includes(selectedArtifact ?? ''))?.name ??
            selectedArtifact
          }
          onSelectStage={selectStage}
        />

        {errorText ? (
          <div className='rd-12px border border-solid border-[rgba(var(--danger-6),0.35)] bg-[rgba(var(--danger-6),0.06)] px-14px py-12px text-13px text-[rgb(var(--danger-6))]'>
            {errorText}
          </div>
        ) : null}

        {board?.final_video || runStatus?.final_video ? (
          <section className='rd-14px border border-solid border-[rgba(var(--success-6),0.35)] bg-[rgba(var(--success-6),0.06)] p-14px'>
            <ProjectMediaPlayer
              projectId={projectId}
              relPath={(board?.final_video || runStatus?.final_video)!}
              title={t('videoGeneration.workspace.finalVideo.title', {
                defaultValue: '成片预览',
              })}
            />
          </section>
        ) : status === 'succeeded' ? (
          <section className='rd-14px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-14px py-16px text-13px text-[var(--color-text-3)]'>
            {t('videoGeneration.workspace.finalVideo.missing', {
              defaultValue:
                '任务已完成，但尚未找到 renders/final.mp4。可打开产物中的 render_report 核对成片路径，或导出工程包查看。',
            })}
          </section>
        ) : null}

        {board?.media_clips && board.media_clips.length > 1 ? (
          <section className='rd-14px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-14px'>
            <div className='mb-10px text-12px font-650 text-[var(--color-text-2)]'>
              {t('videoGeneration.workspace.mediaClips.title', {
                defaultValue: '镜头与渲染片段',
              })}
            </div>
            <div className='flex flex-col gap-14px'>
              {board.media_clips
                .filter((p) => p !== board.final_video)
                .slice(0, 8)
                .map((clip) => (
                  <ProjectMediaPlayer key={clip} projectId={projectId} relPath={clip} title={clip} />
                ))}
            </div>
          </section>
        ) : null}

        {awaitingStage ? (
          <section className='flex flex-col gap-10px rd-14px border border-solid border-[rgba(var(--warning-6),0.4)] bg-[rgba(var(--warning-6),0.08)] p-14px'>
            <div className='text-14px font-650 text-[var(--color-text-1)]'>
              {t('videoGeneration.workspace.approval.title', {
                stage: stageLabel(awaitingStage, t),
                defaultValue: '等待人工审批：{{stage}}',
              })}
            </div>
            <TextArea
              value={approvalNote}
              onChange={setApprovalNote}
              placeholder={t('videoGeneration.workspace.approval.notePlaceholder', {
                defaultValue: '可选备注…',
              })}
              autoSize={{ minRows: 2, maxRows: 4 }}
            />
            {sendBackTargets.length > 0 ? (
              <div className='flex flex-wrap items-center gap-8px'>
                <span className='text-12px text-[var(--color-text-3)]'>
                  {t('videoGeneration.workspace.approval.sendBackTo', {
                    defaultValue: '打回到',
                  })}
                </span>
                <Select
                  size='small'
                  style={{ minWidth: 180 }}
                  value={sendBackTo}
                  onChange={(v) => setSendBackTo(v as string)}
                  options={sendBackTargets.map((s) => ({
                    label: stageLabel(s.name, t) || s.name,
                    value: s.name,
                  }))}
                />
              </div>
            ) : null}
            <div className='flex flex-wrap gap-8px'>
              <Button
                type='primary'
                loading={approving}
                onClick={() => void handleApproval('approve')}
              >
                {t('videoGeneration.workspace.approval.approve', { defaultValue: '批准并继续' })}
              </Button>
              <Button
                status='warning'
                loading={approving}
                disabled={sendBackTargets.length === 0}
                onClick={() => void handleApproval('send_back')}
              >
                {t('videoGeneration.workspace.approval.sendBack', { defaultValue: '打回' })}
              </Button>
              <Button
                status='danger'
                loading={approving}
                onClick={() => void handleApproval('reject')}
              >
                {t('videoGeneration.workspace.approval.reject', { defaultValue: '拒绝' })}
              </Button>
            </div>
          </section>
        ) : null}

        <div className='grid min-h-420px gap-14px lg:grid-cols-[220px_minmax(0,1fr)]'>
          <aside className='flex flex-col gap-8px rd-14px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-12px'>
            <div className='text-12px font-650 text-[var(--color-text-2)]'>
              {t('videoGeneration.artifacts.title', { defaultValue: '产物' })}
            </div>
            {artifactNames.length === 0 ? (
              <div className='text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.artifacts.noneYet', { defaultValue: '尚无产物' })}
              </div>
            ) : (
              artifactNames.map((name) => (
                <button
                  key={name}
                  type='button'
                  className={[
                    'truncate rd-8px border-none px-10px py-8px text-left text-12px cursor-pointer',
                    selectedArtifact === name
                      ? 'bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))] font-600'
                      : 'bg-transparent text-[var(--color-text-2)] hover:bg-[var(--color-fill-2)]',
                  ].join(' ')}
                  onClick={() => setSelectedArtifact(name)}
                >
                  {name}
                </button>
              ))
            )}
          </aside>

          <section className='flex min-h-0 flex-col gap-12px rd-14px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-14px'>
            <ArtifactPreviewPanel
              projectId={projectId}
              artifactName={selectedArtifact}
              preview={preview}
              previewLoading={previewLoading}
              disabled={busy}
              onChanged={() => {
                if (selectedArtifact) {
                  void getArtifact(projectId, selectedArtifact).then(setPreview);
                }
              }}
            />
          </section>
        </div>

        {record.prompt ? (
          <section className='rd-14px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-14px'>
            <div className='mb-8px text-12px font-650 text-[var(--color-text-2)]'>
              {t('videoGeneration.workspace.prompt', { defaultValue: '提示词' })}
            </div>
            <pre className='m-0 whitespace-pre-wrap break-words text-13px leading-[1.55] text-[var(--color-text-2)]'>
              {record.prompt}
            </pre>
          </section>
        ) : null}

        {recentEvents.length > 0 ? (
          <section className='rd-14px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-14px'>
            <div className='mb-8px text-12px font-650 text-[var(--color-text-2)]'>
              {t('videoGeneration.workspace.events', { defaultValue: '最近事件' })}
            </div>
            <ul className='m-0 flex list-none flex-col gap-6px p-0'>
              {recentEvents.map((ev, idx) => (
                <li
                  key={`${ev.at}-${idx}`}
                  className='truncate text-12px text-[var(--color-text-3)]'
                  title={ev.message}
                >
                  <span className='text-[var(--color-text-4)]'>{ev.at.slice(11, 19)}</span>
                  {ev.stage ? ` · ${stageLabel(ev.stage, t)}` : ''}
                  {' · '}
                  {ev.message}
                </li>
              ))}
            </ul>
          </section>
        ) : null}
      </div>
    </div>
  );
};

export default WorkspacePage;
