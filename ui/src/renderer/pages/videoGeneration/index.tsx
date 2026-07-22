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
import { Plus, Search, VideoOne } from '@icon-park/react';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { createSession, deleteSession, listSessions } from './api';
import type { SessionSummary, VimaxWorkflow } from './types';
import SessionCard from './components/SessionCard';
import WorkflowPicker from './components/WorkflowPicker';

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
  const [pickerOpen, setPickerOpen] = useState(false);
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
    async (workflow: VimaxWorkflow, title?: string) => {
      if (creating) return;
      setCreating(true);
      try {
        const created = await createSession({ workflow, title });
        setPickerOpen(false);
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
        'size-full box-border overflow-y-auto',
        isMobile ? 'px-16px py-14px' : 'px-12px py-24px md:px-40px md:py-32px',
      ].join(' ')}
    >
      {messageHolder}
      <div className='mx-auto flex w-full max-w-1180px box-border flex-col gap-16px'>
        {/* Header */}
        <div className='flex w-full flex-wrap items-start justify-between gap-x-20px gap-y-12px'>
          <div className='flex items-start gap-12px min-w-0'>
            <span
              className='flex items-center justify-center w-40px h-40px rd-11px shrink-0 text-[rgb(var(--primary-6))]'
              style={{
                background: 'rgba(var(--primary-6),0.1)',
                border: '1px solid rgba(var(--primary-6),0.18)',
              }}
            >
              <VideoOne theme='outline' size='22' fill='currentColor' className='block' style={{ lineHeight: 0 }} />
            </span>
            <div className='min-w-0'>
              <h1 className='m-0 mb-3px text-22px font-bold text-[var(--color-text-1)] tracking-tight'>
                {t('videoGeneration.title', { defaultValue: '视频生成' })}
              </h1>
              <p className='m-0 text-13px text-[var(--color-text-3)] leading-19px max-w-560px'>
                {t('videoGeneration.subtitle', {
                  defaultValue: '从灵感、剧本或小说出发，规划产物并渲染成片。',
                })}
              </p>
            </div>
          </div>

          {!error && (sessions.length > 0 || loading) && (
            <div className='flex items-center gap-10px'>
              <div className='flex items-center gap-8px bg-[var(--color-fill-2)] border border-solid border-[var(--color-border-3)] rd-10px px-12px py-8px w-200px'>
                <Search theme='outline' size={14} className='text-[var(--color-text-3)] flex-none' />
                <input
                  className='border-none bg-transparent outline-none text-[var(--color-text-1)] text-13px w-full font-[inherit] placeholder:text-[var(--color-text-3)]'
                  placeholder={t('videoGeneration.list.searchPlaceholder', {
                    defaultValue: '搜索任务...',
                  })}
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                />
              </div>
              <Button type='primary' className='shrink-0' onClick={() => setPickerOpen(true)}>
                <span className='inline-flex items-center gap-6px'>
                  <Plus theme='outline' size='15' fill='currentColor' className='block' style={{ lineHeight: 0 }} />
                  {t('videoGeneration.list.newSession', { defaultValue: '新建任务' })}
                </span>
              </Button>
            </div>
          )}
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
          <div className='flex justify-center py-56px'>
            <Spin />
          </div>
        ) : sessions.length === 0 ? (
          <div className='flex flex-col items-center gap-14px rd-16px border border-dashed border-[var(--color-border-2)] bg-fill-1 px-20px py-52px text-center'>
            <span
              className='flex items-center justify-center w-56px h-56px rd-16px text-[rgb(var(--primary-6))]'
              style={{
                background: 'rgba(var(--primary-6),0.1)',
                border: '1px solid rgba(var(--primary-6),0.18)',
              }}
            >
              <VideoOne theme='outline' size='28' fill='currentColor' className='block' style={{ lineHeight: 0 }} />
            </span>
            <div className='flex flex-col gap-4px'>
              <span className='text-15px font-600 text-[var(--color-text-1)]'>
                {t('videoGeneration.list.empty.title', { defaultValue: '还没有视频任务' })}
              </span>
              <span className='text-13px text-[var(--color-text-3)] max-w-[440px]'>
                {t('videoGeneration.list.empty.desc', {
                  defaultValue: '选择一种工作流，开始你的第一段成片。',
                })}
              </span>
            </div>
            <Button type='primary' onClick={() => setPickerOpen(true)}>
              <span className='inline-flex items-center gap-6px'>
                <Plus theme='outline' size='15' fill='currentColor' className='block' style={{ lineHeight: 0 }} />
                {t('videoGeneration.list.createFirst', { defaultValue: '新建第一个任务' })}
              </span>
            </Button>
          </div>
        ) : (
          <>
            <div className='grid gap-12px' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(min(320px, 100%), 1fr))' }}>
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
      </div>

      <WorkflowPicker
        visible={pickerOpen}
        loading={creating}
        onCancel={() => setPickerOpen(false)}
        onConfirm={(wf, title) => void handleCreate(wf, title)}
      />
    </div>
  );
};

export default VideoGenerationListPage;
