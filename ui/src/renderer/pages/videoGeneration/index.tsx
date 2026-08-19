
/**
 * Unified video creation home (`/video-generation`).
 * Agent and infinite-canvas creation share one composer while keeping their
 * skills, drafts, submissions, and project galleries independent.
 */
import React, {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Result, Spin } from '@arco-design/web-react';
import { Search, Upload, VideoOne } from '@icon-park/react';
import SegmentedTabs, { type SegmentedTabItem } from '@renderer/components/base/SegmentedTabs';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { isDesktopShell } from '@renderer/utils/platform';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import {
  createSession,
  deleteSession,
  importSession,
  listSessions,
  planSession,
  renderSession,
  uploadActionAssets,
  uploadCameo,
} from './api';
import type { PlanBody, SessionSummary } from './types';
import VideoHomeComposer, { clearVideoHomeDraft } from './home/VideoHomeComposer';
import type { VideoCreateDraft, VideoHomeMode } from './home/types';
import {
  CLIP_DURATION_DEFAULT_SECS,
  CLIP_DURATION_MAX_SECS,
  CLIP_DURATION_MIN_SECS,
  CLIP_DURATION_STEP_SECS,
  clampDuration,
  DURATION_MAX_SECS,
  DURATION_MIN_SECS,
  DURATION_STEP_SECS,
} from './durationBounds';
import {
  clearVideoGenerationSessionMemory,
  rememberVideoGenerationSession,
} from './routeMemory';
import { isInsufficientCreditsError } from './creditsError';
import styles from './index.module.css';

/** Lazily load OC bridge so the list page chunk does not pull @oc/stores. */
async function createServerBackedCanvasProject(
  ...args: Parameters<
    typeof import('../videoCanvas/lib/ocBridge').createServerBackedCanvasProject
  >
) {
  const { createServerBackedCanvasProject: create } = await import(
    '../videoCanvas/lib/ocBridge'
  );
  return create(...args);
}

type ListTab = 'recent' | 'tvShow';

const CanvasProjectGallery = lazy(() => import('./home/CanvasProjectGallery'));
const TvShowPanel = lazy(() => import('./components/TvShowPanel'));
const SessionCard = lazy(() => import('./components/SessionCard'));

function sourceBodyForDraft(draft: VideoCreateDraft): PlanBody {
  const common: PlanBody = {
    user_requirement: draft.requirement.trim() || undefined,
    style: draft.style.trim() || undefined,
    vertical_skill_ids:
      draft.verticalSkillIds.length > 0 ? draft.verticalSkillIds : undefined,
    target_duration_secs: clampDuration(
      draft.preferences.targetDurationSecs,
      DURATION_MIN_SECS,
      DURATION_MAX_SECS,
      DURATION_STEP_SECS
    ),
    aspect_ratio: draft.preferences.aspectRatio,
    resolution: draft.preferences.resolution,
    fps: draft.preferences.fps,
    llm_model: draft.preferences.models.llm_model,
    image_model: draft.preferences.models.image_model || undefined,
    video_model: draft.preferences.models.video_model || undefined,
  };
  switch (draft.workflow) {
    case 'idea2video':
      return { ...common, idea: draft.sourceText };
    case 'script2video':
      return { ...common, script: draft.sourceText };
    case 'novel2video':
      return { ...common, novel_text: draft.sourceText };
    case 'action2video':
      return {
        video_model: draft.preferences.models.video_model || undefined,
        resolution: draft.preferences.resolution,
        fps: draft.preferences.fps,
      };
    default: {
      const exhaustive: never = draft.workflow;
      return exhaustive;
    }
  }
}

function titleForDraft(draft: VideoCreateDraft): string {
  if (draft.workflow === 'action2video') {
    const fromCharacter = draft.actionCharacter?.file.name.replace(/\.[^.]+$/, '').trim();
    return fromCharacter?.slice(0, 48) || '';
  }
  return draft.sourceText.split(/\r?\n/, 1)[0]?.trim().slice(0, 48) || '';
}

const VideoGenerationListPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const [message, messageHolder] = useArcoMessage();

  const workMode: VideoHomeMode =
    searchParams.get('mode') === 'creation' || searchParams.get('mode') === 'canvas'
      ? 'creation'
      : 'agent';

  const [listTab, setListTab] = useState<ListTab>('tvShow');
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const pageScrollRef = useRef<HTMLDivElement>(null);
  const savedPageScrollTopRef = useRef(0);

  const handleListTabChange = useCallback((key: string) => {
    savedPageScrollTopRef.current = pageScrollRef.current?.scrollTop ?? 0;
    setListTab(key as ListTab);
  }, []);

  useLayoutEffect(() => {
    const page = pageScrollRef.current;
    if (!page) return;
    page.scrollTop = savedPageScrollTopRef.current;
  }, [listTab]);

  const listTabItems: SegmentedTabItem[] = useMemo(
    () => [
      {
        key: 'tvShow',
        label: t('videoGeneration.list.tabs.tvShow', { defaultValue: 'Flowy TV' }),
      },
      {
        key: 'recent',
        label: t('videoGeneration.list.tabs.recent', { defaultValue: '最近创作' }),
      },
    ],
    [t]
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setSessions(await listSessions());
      setError(null);
    } catch (e) {
      console.error('[videoGeneration] failed to load sessions', e);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (workMode === 'agent' && listTab === 'recent') void refresh();
  }, [listTab, refresh, workMode]);

  useEffect(() => {
    const prefetchGallery = () => {
      void import('./home/CanvasProjectGallery');
    };
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      const idleId = idleWindow.requestIdleCallback(prefetchGallery, { timeout: 1500 });
      return () => idleWindow.cancelIdleCallback?.(idleId);
    }
    const timer = window.setTimeout(prefetchGallery, 250);
    return () => window.clearTimeout(timer);
  }, []);

  const displayed = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter(
      (s) =>
        (s.title ?? '').toLowerCase().includes(q) ||
        s.workflow.toLowerCase().includes(q) ||
        (s.stage ?? '').toLowerCase().includes(q)
    );
  }, [sessions, searchQuery]);

  const handleCreate = useCallback(
    async (draft: VideoCreateDraft) => {
      if (creating) return;
      setCreating(true);
      try {
        const created = await createSession({
          workflow: draft.workflow,
          title: titleForDraft(draft) || undefined,
        });
        trackFunnelEvent('task_accepted', {
          feature: 'video_generation',
          workflow: draft.workflow,
          session_id: created.id,
        });
        try {
          if (draft.workflow === 'action2video') {
            if (!draft.actionCharacter?.file || !draft.actionVideo?.file) {
              throw new Error(
                t('videoGeneration.create.action.required', {
                  defaultValue: '请上传一张角色图和一个参考视频。',
                })
              );
            }
            await uploadActionAssets(created.id, {
              character: draft.actionCharacter.file,
              video: draft.actionVideo.file,
            });
            await renderSession(created.id, {
              video_model: draft.preferences.models.video_model || undefined,
              resolution: draft.preferences.resolution,
              fps: draft.preferences.fps,
            });
          } else {
            const pendingCameos = draft.cameos.filter((c) => c.file && c.characterName.trim());
            for (const cameo of pendingCameos) {
              await uploadCameo(
                created.id,
                cameo.file!,
                cameo.characterName.trim(),
                cameo.description.trim()
              );
            }
            await planSession(created.id, sourceBodyForDraft(draft));
          }
          trackFunnelEvent('first_task_started', {
            feature: 'video_generation',
            workflow: draft.workflow,
            session_id: created.id,
          });
          clearVideoHomeDraft();
        } catch (planError) {
          const raw = planError instanceof Error ? planError.message : String(planError);
          const failedLabel =
            draft.workflow === 'action2video'
              ? t('videoGeneration.workspace.renderFailed', { defaultValue: '渲染失败' })
              : t('videoGeneration.workspace.planFailed', { defaultValue: '规划失败' });
          message.error(
            isInsufficientCreditsError(raw)
              ? t('videoGeneration.workspace.failure.creditsToast', {
                  defaultValue: '积分不足，请充值或缩短时长后从断点继续。',
                })
              : `${failedLabel}: ${raw}`
          );
          navigate(`/video-generation/${created.id}`, {
            state: { launchDraft: draft, launchError: true },
          });
          rememberVideoGenerationSession(created.id, titleForDraft(draft));
          return;
        }
        rememberVideoGenerationSession(created.id, titleForDraft(draft));
        navigate(`/video-generation/${created.id}`);
      } catch (e) {
        message.error(
          `${t('videoGeneration.actions.createFailed', { defaultValue: '创建失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setCreating(false);
      }
    },
    [creating, navigate, message, t]
  );

  const handleModeChange = useCallback(
    (mode: VideoHomeMode) => {
      const next = new URLSearchParams(searchParams);
      if (mode === 'creation') next.set('mode', 'creation');
      else next.delete('mode');
      setSearchParams(next, { replace: true });
    },
    [searchParams, setSearchParams]
  );

  const handleCreateCanvas = useCallback(
    async (draft: VideoCreateDraft) => {
      if (creating) return;
      setCreating(true);
      const references: Array<{ media_id: string }> = [];
      let canvasCreated = false;
      try {
        const { uploadCanvasMedia } = await import('../videoCanvas/api');
        for (const reference of draft.canvasReferences) {
          references.push(
            await uploadCanvasMedia(reference.file, reference.file.name)
          );
        }
        const skillId = draft.creationSkillId;
        const skillDefaults = {
          cinematic: { label: '电影写实', desc: '纪实光影 · 叙事镜头' },
          anime: { label: '二次元', desc: '鲜明线稿 · 动漫质感' },
          cyberpunk: { label: '赛博霓虹', desc: '未来都市 · 高对比' },
          inkWash: { label: '水墨意境', desc: '留白构图 · 东方美学' },
        } as const;
        const defaults = skillDefaults[skillId];
        const skillLabel = t(`videoGeneration.create.skills.${skillId}.label`, {
          defaultValue: defaults.label,
        });
        const skillDescription = t(`videoGeneration.create.skills.${skillId}.desc`, {
          defaultValue: defaults.desc,
        });
        const title =
          draft.creationPrompt.split(/\r?\n/, 1)[0]?.trim().slice(0, 36) ||
          t('videoGeneration.create.canvasTitleFromSkill', {
            skill: skillLabel,
            defaultValue: '{{skill}}创作',
          });
        const id = await createServerBackedCanvasProject(title, {
          prompt: draft.creationPrompt,
          requirement: draft.requirement.trim() || undefined,
          mediaKind: draft.preferences.mediaKind,
          skill: {
            id: draft.creationSkillId,
            label: skillLabel,
            description: skillDescription,
            stylePrompt: draft.style,
          },
          preferences: {
            automatic: draft.preferences.automatic,
            aspectRatio: draft.preferences.aspectRatio,
            resolution: draft.preferences.resolution,
            fps: draft.preferences.fps,
            // Canvas nodes expect single-clip seconds (≈4–15), not Agent film length.
            targetDurationSecs: clampDuration(
              draft.preferences.targetDurationSecs,
              CLIP_DURATION_MIN_SECS,
              CLIP_DURATION_MAX_SECS,
              CLIP_DURATION_STEP_SECS
            ) || CLIP_DURATION_DEFAULT_SECS,
            imageModel: draft.preferences.models.image_model || undefined,
            videoModel: draft.preferences.models.video_model || undefined,
          },
          references,
        });
        canvasCreated = true;
        trackFunnelEvent('task_accepted', {
          feature: 'video_generation',
          mode: 'creation',
          skill: draft.creationSkillId,
          project_id: id,
        });
        clearVideoHomeDraft();
        navigate(`/video-generation/canvas/${encodeURIComponent(id)}`);
      } catch (cause) {
        if (!canvasCreated && references.length > 0) {
          const { deleteCanvasMedia } = await import('../videoCanvas/api');
          await Promise.allSettled(
            references.map((reference) => deleteCanvasMedia(reference.media_id))
          );
        }
        message.error(
          `${t('videoCanvas.actions.createFailed', { defaultValue: '创建失败' })}: ${
            cause instanceof Error ? cause.message : String(cause)
          }`
        );
      } finally {
        setCreating(false);
      }
    },
    [creating, message, navigate, t]
  );

  const openSession = useCallback(
    (s: SessionSummary) => {
      rememberVideoGenerationSession(s.id, s.title);
      navigate(`/video-generation/${s.id}`);
    },
    [navigate]
  );

  const handleDelete = useCallback(
    async (s: SessionSummary) => {
      if (deletingId) return;
      setDeletingId(s.id);
      try {
        await deleteSession(s.id);
        clearVideoGenerationSessionMemory(s.id);
        setSessions((prev) => prev.filter((x) => x.id !== s.id));
        message.success(t('videoGeneration.actions.deleteOk', { defaultValue: '已删除任务' }));
      } catch (e) {
        message.error(
          `${t('videoGeneration.actions.deleteFailed', { defaultValue: '删除失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setDeletingId(null);
      }
    },
    [deletingId, message, t]
  );

  const handleImportProject = useCallback(async () => {
    if (importing || creating) return;
    if (!isDesktopShell()) {
      message.info(
        t('videoGeneration.list.importDesktopOnly', {
          defaultValue: '导入工程仅桌面端可用。',
        })
      );
      return;
    }
    const { dialog } = await import('@/common/adapter/ipcBridge');
    const paths = await dialog.showOpen.invoke({
      properties: ['openFile'],
      filters: [
        {
          name: t('videoGeneration.actions.exportFilter', { defaultValue: 'Flowy 视频工程' }),
          extensions: ['nomivimax'],
        },
      ],
    });
    const source = paths?.[0];
    if (!source) return;
    setImporting(true);
    try {
      const imported = await importSession(source);
      trackFunnelEvent('task_accepted', {
        feature: 'video_generation',
        workflow: imported.workflow,
        session_id: imported.id,
        source: 'project_import',
      });
      message.success(t('videoGeneration.list.importOk', { defaultValue: '工程已导入' }));
      navigate(`/video-generation/${imported.id}`);
    } catch (e) {
      message.error(
        `${t('videoGeneration.list.importFailed', { defaultValue: '导入失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setImporting(false);
    }
  }, [creating, importing, message, navigate, t]);

  return (
    <div
      ref={pageScrollRef}
      className={[
        styles.page,
        'flex-1 min-h-0 size-full box-border overflow-y-auto',
        isMobile ? 'px-12px py-12px' : 'px-16px py-24px md:px-36px md:py-32px',
      ].join(' ')}
    >
      {messageHolder}
      <div className='mx-auto flex w-full max-w-1180px box-border flex-col gap-26px'>
        <VideoHomeComposer
          mode={workMode}
          loading={creating}
          onModeChange={handleModeChange}
          onSubmitAgent={(draft) => void handleCreate(draft)}
          onSubmitCreation={(draft) => void handleCreateCanvas(draft)}
        />

        {workMode === 'creation' ? (
          <Suspense
            fallback={
              <div className='flex justify-center py-38px'>
                <Spin />
              </div>
            }
          >
            <CanvasProjectGallery />
          </Suspense>
        ) : (
          <section className='flex flex-col gap-12px'>
            <div className='flex flex-wrap items-center justify-between gap-12px'>
              <div>
                <div className='mb-8px'>
                  <SegmentedTabs
                    size='sm'
                    items={listTabItems}
                    activeKey={listTab}
                    onChange={handleListTabChange}
                  />
                </div>
                <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
                  {listTab === 'tvShow'
                    ? t('videoGeneration.tvShow.title', { defaultValue: 'Flowy TV' })
                    : t('videoGeneration.list.recentTitle', {
                        defaultValue: '最近创作',
                      })}
                </h2>
                <p className='m-0 mt-3px text-12px text-[var(--color-text-3)]'>
                  {listTab === 'tvShow'
                    ? t('videoGeneration.tvShow.subtitle', {
                        defaultValue: '浏览社区已上架的作品，或查看你的发布审核状态。',
                      })
                    : t('videoGeneration.list.recentSubtitle', {
                        defaultValue: '继续分镜、渲染或查看已经完成的影片。',
                      })}
                </p>
              </div>
              {listTab === 'recent' ? (
                <div className='flex flex-wrap items-center gap-10px'>
                  <Button
                    type='outline'
                    size='small'
                    loading={importing}
                    disabled={creating || importing}
                    onClick={() => void handleImportProject()}
                  >
                    <span className='inline-flex items-center gap-4px'>
                      <Upload theme='outline' size={14} fill='currentColor' />
                      {t('videoGeneration.list.importProject', {
                        defaultValue: '导入工程',
                      })}
                    </span>
                  </Button>
                  {!error && sessions.length > 0 ? (
                    <div className='flex w-220px items-center gap-8px rd-10px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-11px py-7px'>
                      <Search
                        theme='outline'
                        size={14}
                        className='flex-none text-[var(--color-text-3)]'
                      />
                      <input
                        className='w-full border-none bg-transparent text-13px text-[var(--color-text-1)] outline-none font-[inherit] placeholder:text-[var(--color-text-3)]'
                        placeholder={t('videoGeneration.list.searchPlaceholder', {
                          defaultValue: '搜索项目...',
                        })}
                        value={searchQuery}
                        onChange={(event) => setSearchQuery(event.target.value)}
                      />
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>

            <div>
              {listTab === 'tvShow' ? (
                <Suspense
                  fallback={
                    <div className='flex justify-center py-38px'>
                      <Spin />
                    </div>
                  }
                >
                  <TvShowPanel enabled />
                </Suspense>
              ) : (
                <div>
                  {error ? (
                    <Result
                      status='error'
                      title={t('videoGeneration.list.loadError', {
                        defaultValue: '加载失败',
                      })}
                      subTitle={error}
                      extra={
                        <Button onClick={() => void refresh()}>
                          {t('videoGeneration.list.retry', { defaultValue: '重试' })}
                        </Button>
                      }
                    />
                  ) : loading ? (
                    <div className='flex justify-center py-38px'>
                      <Spin />
                    </div>
                  ) : sessions.length === 0 ? (
                    <div className='flex items-center gap-12px rd-14px border border-dashed border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-16px py-18px'>
                      <span className='flex h-38px w-38px shrink-0 items-center justify-center rd-11px bg-[rgba(var(--primary-6),0.1)] text-[rgb(var(--primary-6))]'>
                        <VideoOne theme='outline' size={19} fill='currentColor' />
                      </span>
                      <div>
                        <div className='text-13px font-600 text-[var(--color-text-1)]'>
                          {t('videoGeneration.list.empty.title', {
                            defaultValue: '你的第一支影片从上方开始',
                          })}
                        </div>
                        <div className='mt-2px text-12px text-[var(--color-text-3)]'>
                          {t('videoGeneration.list.empty.desc', {
                            defaultValue: '写下一个画面或故事，Flowy 会先给你一版可编辑分镜。',
                          })}
                        </div>
                      </div>
                    </div>
                  ) : (
                    <>
                      <Suspense
                        fallback={
                          <div className='flex justify-center py-38px'>
                            <Spin />
                          </div>
                        }
                      >
                        <div
                          className='grid gap-12px'
                          style={{
                            gridTemplateColumns:
                              'repeat(auto-fill, minmax(min(300px, 100%), 1fr))',
                          }}
                        >
                          {displayed.map((session) => (
                            <SessionCard
                              key={session.id}
                              session={session}
                              onOpen={openSession}
                              onDelete={(s) => void handleDelete(s)}
                              deleting={deletingId === session.id}
                            />
                          ))}
                        </div>
                      </Suspense>
                      {displayed.length === 0 && (
                        <div className='flex flex-col items-center gap-8px py-40px text-[var(--color-text-3)] text-13px'>
                          {t('videoGeneration.list.filterEmpty', {
                            defaultValue: '没有匹配的任务',
                          })}
                        </div>
                      )}
                    </>
                  )}
                </div>
              )}
            </div>
          </section>
        )}
      </div>
    </div>
  );
};

export default VideoGenerationListPage;
