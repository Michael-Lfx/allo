/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * VideoGeneration list page (`/video-generation`) — session gallery with
 * workflow-picker create flow. Visual language mirrors Knowledge / Workshop
 * (theme tokens, rd-*, fill/primary).
 */
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Result, Spin } from '@arco-design/web-react';
import { Search, VideoOne } from '@icon-park/react';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { createSession, deleteSession, listSessions, planSession } from './api';
import type { PlanBody, SessionSummary } from './types';
import SessionCard from './components/SessionCard';
import VideoCreateComposer, {
  clearVideoCreateDraft,
  type VideoCreateDraft,
} from './components/VideoCreateComposer';
import styles from './index.module.css';

function sourceBodyForDraft(draft: VideoCreateDraft): PlanBody {
  const common: PlanBody = {
    user_requirement: draft.requirement.trim() || undefined,
    style: draft.style.trim() || undefined,
    target_duration_secs: draft.targetDurationSecs,
    llm_model: draft.models.llm_model,
    image_model: draft.models.image_model || undefined,
    video_model: draft.models.video_model || undefined,
  };
  switch (draft.workflow) {
    case 'idea2video':
      return { ...common, idea: draft.sourceText };
    case 'script2video':
      return { ...common, script: draft.sourceText };
    case 'novel2video':
      return { ...common, novel_text: draft.sourceText };
    default: {
      const exhaustive: never = draft.workflow;
      return exhaustive;
    }
  }
}

function titleForDraft(draft: VideoCreateDraft): string {
  return draft.sourceText.split(/\r?\n/, 1)[0]?.trim().slice(0, 48) || '';
}

const VideoGenerationListPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const [message, messageHolder] = useArcoMessage();

  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);

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
    void refresh();
  }, [refresh]);

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
        try {
          await planSession(created.id, sourceBodyForDraft(draft));
          clearVideoCreateDraft();
        } catch (planError) {
          message.error(
            `${t('videoGeneration.workspace.planFailed', { defaultValue: '规划失败' })}: ${
              planError instanceof Error ? planError.message : String(planError)
            }`
          );
          navigate(`/video-generation/${created.id}`, {
            state: { launchDraft: draft, launchError: true },
          });
          return;
        }
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

  const openSession = useCallback(
    (s: SessionSummary) => navigate(`/video-generation/${s.id}`),
    [navigate]
  );

  const handleDelete = useCallback(
    async (s: SessionSummary) => {
      if (deletingId) return;
      setDeletingId(s.id);
      try {
        await deleteSession(s.id);
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

  return (
    <div
      className={[
        styles.page,
        'size-full box-border overflow-y-auto',
        isMobile ? 'px-12px py-12px' : 'px-16px py-24px md:px-36px md:py-32px',
      ].join(' ')}
    >
      {messageHolder}
      <div className='mx-auto flex w-full max-w-1180px box-border flex-col gap-26px'>
        <VideoCreateComposer loading={creating} onSubmit={(draft) => void handleCreate(draft)} />

        <section className='flex flex-col gap-12px'>
          <div className='flex flex-wrap items-center justify-between gap-12px'>
            <div>
              <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
                {t('videoGeneration.list.recentTitle', { defaultValue: '最近创作' })}
              </h2>
              <p className='m-0 mt-3px text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.list.recentSubtitle', {
                  defaultValue: '继续分镜、渲染或查看已经完成的影片。',
                })}
              </p>
            </div>
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

          {error ? (
          <Result
            status='error'
            title={t('videoGeneration.list.loadError', { defaultValue: '加载失败' })}
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
                {t('videoGeneration.list.empty.title', { defaultValue: '你的第一支影片从上方开始' })}
              </div>
              <div className='mt-2px text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.list.empty.desc', {
                  defaultValue: '写下一个画面或故事，Nomi 会先给你一版可编辑分镜。',
                })}
              </div>
            </div>
          </div>
        ) : (
          <>
            <div
              className='grid gap-12px'
              style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(min(300px, 100%), 1fr))' }}
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
            {displayed.length === 0 && (
              <div className='flex flex-col items-center gap-8px py-40px text-[var(--color-text-3)] text-13px'>
                {t('videoGeneration.list.filterEmpty', { defaultValue: '没有匹配的任务' })}
              </div>
            )}
          </>
        )}
        </section>
      </div>
    </div>
  );
};

export default VideoGenerationListPage;
