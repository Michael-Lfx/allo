
import React, { Suspense, lazy, useCallback, useEffect, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Drawer, Result, Spin } from '@arco-design/web-react';
import { Download, Like, Search, VideoOne } from '@icon-park/react';
import SegmentedTabs, { type SegmentedTabItem } from '@renderer/components/base/SegmentedTabs';
import { isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import {
  deleteTvShow,
  getTvShowDetail,
  importTvShow,
  likeTvShow,
  listMyTvShow,
  listTvShow,
  unlikeTvShow,
} from '../api';
import { importCanvasTvShow } from '../../videoCanvas/api';
import type { TvShowVideo } from '../types';
import { isCanvasTvShow, tvShowWorkflowLabel } from './SessionCard';
import TvShowCard from './TvShowCard';

const CampaignPanel = lazy(() => import('./CampaignPanel'));

type TvShowScope = 'plaza' | 'campaign' | 'mine';

const TV_SHOW_INITIAL_PAGE_SIZE = 16;

interface TvShowPanelProps {
  enabled: boolean;
}

const TvShowPanel: React.FC<TvShowPanelProps> = ({ enabled }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const { status: cloudStatus, logout } = useCloudAuth();
  const [message, messageHolder] = useArcoMessage();

  const [scope, setScope] = useState<TvShowScope>(() =>
    searchParams.get('tvScope') === 'campaign' ? 'campaign' : 'plaza'
  );
  const [videos, setVideos] = useState<TvShowVideo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [keywordInput, setKeywordInput] = useState('');
  const [keyword, setKeyword] = useState('');
  const [likingId, setLikingId] = useState<number | null>(null);
  const [deletingId, setDeletingId] = useState<number | null>(null);
  const [detail, setDetail] = useState<TvShowVideo | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const videosRef = useRef(videos);
  videosRef.current = videos;

  const scopeItems: SegmentedTabItem[] = [
    {
      key: 'plaza',
      label: t('videoGeneration.tvShow.scope.plaza', { defaultValue: '广场' }),
    },
    {
      key: 'campaign',
      label: t('videoGeneration.tvShow.scope.campaign', { defaultValue: '活动' }),
    },
    {
      key: 'mine',
      label: t('videoGeneration.tvShow.scope.mine', { defaultValue: '我的发布' }),
    },
  ];

  const consumeExpiredCloudSession = useCallback(
    async (error: unknown): Promise<boolean> => {
      if (!isInvalidCloudSessionError(error)) return false;
      await logout();
      return true;
    },
    [logout]
  );

  const refresh = useCallback(async () => {
    if (!enabled) return;
    if (scope === 'campaign') return;
    if (cloudStatus !== 'authenticated') {
      setVideos([]);
      setError(null);
      setLoading(false);
      return;
    }
    // Keep existing cards visible while refreshing so the parent page height
    // (and scrollTop) does not collapse when switching back to this tab.
    const showSpinner = videosRef.current.length === 0;
    if (showSpinner) setLoading(true);
    try {
      const data =
        scope === 'mine'
          ? await listMyTvShow({ page: 1, pageSize: TV_SHOW_INITIAL_PAGE_SIZE })
          : await listTvShow({
              page: 1,
              pageSize: TV_SHOW_INITIAL_PAGE_SIZE,
              keyword: keyword.trim() || undefined,
              sort: 'publishedAtDesc',
            });
      setVideos(data.list ?? []);
      setError(null);
    } catch (e) {
      console.error('[videoGeneration] TV Show list failed', e);
      if (await consumeExpiredCloudSession(e)) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [cloudStatus, consumeExpiredCloudSession, enabled, keyword, scope]);

  const handleScopeChange = useCallback(
    (key: string) => {
      const next = key as TvShowScope;
      setScope(next);
      setSearchParams(
        (prev) => {
          const nextParams = new URLSearchParams(prev);
          if (next === 'campaign') nextParams.set('tvScope', 'campaign');
          else nextParams.delete('tvScope');
          return nextParams;
        },
        { replace: true }
      );
    },
    [setSearchParams]
  );

  useEffect(() => {
    if (searchParams.get('tvScope') === 'campaign' && scope !== 'campaign') {
      setScope('campaign');
    }
  }, [searchParams, scope]);

  useEffect(() => {
    if (!enabled) return;
    void refresh();
  }, [enabled, refresh]);

  const displayed = videos.filter((v) => {
    if (scope !== 'mine') return true;
    const q = searchQuery.trim().toLowerCase();
    if (!q) return true;
    return (
      (v.title ?? '').toLowerCase().includes(q) ||
      String(v.workflow).toLowerCase().includes(q) ||
      String(v.status).toLowerCase().includes(q)
    );
  });

  const openDetail = useCallback(
    async (video: TvShowVideo) => {
      setDetail(video);
      setDetailLoading(true);
      try {
        const full = await getTvShowDetail(video.id);
        setDetail(full);
      } catch (e) {
        if (await consumeExpiredCloudSession(e)) return;
        message.error(
          `${t('videoGeneration.tvShow.detail.loadFailed', { defaultValue: '加载详情失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setDetailLoading(false);
      }
    },
    [consumeExpiredCloudSession, message, t]
  );

  const handleToggleLike = useCallback(
    async (video: TvShowVideo) => {
      if (likingId != null) return;
      setLikingId(video.id);
      try {
        const result = video.liked ? await unlikeTvShow(video.id) : await likeTvShow(video.id);
        setVideos((prev) =>
          prev.map((v) =>
            v.id === video.id
              ? { ...v, liked: result.liked, likeCount: result.likeCount }
              : v
          )
        );
        setDetail((prev) =>
          prev && prev.id === video.id
            ? { ...prev, liked: result.liked, likeCount: result.likeCount }
            : prev
        );
      } catch (e) {
        if (await consumeExpiredCloudSession(e)) return;
        message.error(
          `${t('videoGeneration.tvShow.actions.likeFailed', { defaultValue: '点赞失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setLikingId(null);
      }
    },
    [consumeExpiredCloudSession, likingId, message, t]
  );

  const handleDelete = useCallback(
    async (video: TvShowVideo) => {
      if (deletingId != null) return;
      setDeletingId(video.id);
      try {
        await deleteTvShow(video.id);
        setVideos((prev) => prev.filter((v) => v.id !== video.id));
        if (detail?.id === video.id) setDetail(null);
        message.success(
          t('videoGeneration.tvShow.actions.deleteOk', { defaultValue: '已删除发布' })
        );
      } catch (e) {
        if (await consumeExpiredCloudSession(e)) return;
        message.error(
          `${t('videoGeneration.tvShow.actions.deleteFailed', { defaultValue: '删除失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setDeletingId(null);
      }
    },
    [consumeExpiredCloudSession, deletingId, detail?.id, message, t]
  );

  const handleImport = useCallback(async () => {
    if (!detail || importing) return;
    setImporting(true);
    try {
      if (isCanvasTvShow(detail)) {
        const imported = await importCanvasTvShow(detail.id);
        message.success(
          t('videoGeneration.tvShow.actions.importOk', { defaultValue: '工程已导入到本地' })
        );
        setDetail(null);
        navigate(`/video-generation/canvas/${encodeURIComponent(imported.project_id)}`);
        return;
      }
      const imported = await importTvShow(detail.id);
      message.success(
        t('videoGeneration.tvShow.actions.importOk', { defaultValue: '工程已导入到本地' })
      );
      setDetail(null);
      navigate(`/video-generation/${imported.id}`);
    } catch (e) {
      if (await consumeExpiredCloudSession(e)) return;
      message.error(
        `${t('videoGeneration.tvShow.actions.importFailed', { defaultValue: '导入失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setImporting(false);
    }
  }, [consumeExpiredCloudSession, detail, importing, message, navigate, t]);

  // Fetch only while visible; keep rendering so a hidden parent can still
  // reserve layout height and avoid scroll jumps on tab switches.
  if (cloudStatus === 'checking') {
    return (
      <div className='flex justify-center py-38px'>
        <Spin />
      </div>
    );
  }

  if (cloudStatus !== 'authenticated') {
    return (
      <div className='flex items-center gap-12px rd-14px border border-dashed border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-16px py-18px'>
        <span className='flex h-38px w-38px shrink-0 items-center justify-center rd-11px bg-[rgba(var(--primary-6),0.1)] text-[rgb(var(--primary-6))]'>
          <VideoOne theme='outline' size={19} fill='currentColor' />
        </span>
        <div className='min-w-0 flex-1'>
          <div className='text-13px font-600 text-[var(--color-text-1)]'>
            {t('videoGeneration.tvShow.authRequired.title', {
              defaultValue: '登录后观看 Flowy TV',
            })}
          </div>
          <div className='mt-2px text-12px text-[var(--color-text-3)]'>
            {t('videoGeneration.tvShow.authRequired.desc', {
              defaultValue: 'Flowy TV 广场与发布需要云端账号。',
            })}
          </div>
        </div>
        <Button type='primary' size='small' onClick={() => navigate('/cloud-login')}>
          {t('videoGeneration.tvShow.authRequired.login', { defaultValue: '去登录' })}
        </Button>
      </div>
    );
  }

  return (
    <div className='flex flex-col gap-12px'>
      {messageHolder}
      <div className='flex flex-wrap items-center justify-between gap-12px'>
        <SegmentedTabs
          size='sm'
          items={scopeItems}
          activeKey={scope}
          onChange={handleScopeChange}
        />
        {scope === 'campaign' ? null : (
        <div className='flex w-220px items-center gap-8px rd-10px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-11px py-7px'>
          <Search theme='outline' size={14} className='flex-none text-[var(--color-text-3)]' />
          <input
            className='w-full border-none bg-transparent text-13px text-[var(--color-text-1)] outline-none font-[inherit] placeholder:text-[var(--color-text-3)]'
            placeholder={t('videoGeneration.tvShow.searchPlaceholder', {
              defaultValue: '搜索作品...',
            })}
            value={scope === 'plaza' ? keywordInput : searchQuery}
            onChange={(event) => {
              if (scope === 'plaza') setKeywordInput(event.target.value);
              else setSearchQuery(event.target.value);
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && scope === 'plaza') {
                setKeyword(keywordInput.trim());
              }
            }}
            onBlur={() => {
              if (scope === 'plaza') setKeyword(keywordInput.trim());
            }}
          />
        </div>
        )}
      </div>

      {scope === 'campaign' ? (
        <Suspense
          fallback={
            <div className='flex justify-center py-38px'>
              <Spin />
            </div>
          }
        >
          <CampaignPanel />
        </Suspense>
      ) : error ? (
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
      ) : displayed.length === 0 ? (
        <div className='flex items-center gap-12px rd-14px border border-dashed border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-16px py-18px'>
          <span className='flex h-38px w-38px shrink-0 items-center justify-center rd-11px bg-[rgba(var(--primary-6),0.1)] text-[rgb(var(--primary-6))]'>
            <VideoOne theme='outline' size={19} fill='currentColor' />
          </span>
          <div>
            <div className='text-13px font-600 text-[var(--color-text-1)]'>
              {scope === 'mine'
                ? t('videoGeneration.tvShow.empty.mineTitle', {
                    defaultValue: '还没有发布作品',
                  })
                : t('videoGeneration.tvShow.empty.plazaTitle', {
                    defaultValue: '广场暂无作品',
                  })}
            </div>
            <div className='mt-2px text-12px text-[var(--color-text-3)]'>
              {scope === 'mine'
                ? t('videoGeneration.tvShow.empty.mineDesc', {
                    defaultValue: '在短剧工作区或创作画布里点击「发布到 Flowy TV」。',
                  })
                : t('videoGeneration.tvShow.empty.plazaDesc', {
                    defaultValue: '审核通过的作品会出现在这里。',
                  })}
            </div>
          </div>
        </div>
      ) : (
        <div
          className='grid gap-12px'
          style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(min(300px, 100%), 1fr))' }}
        >
          {displayed.map((video) => (
            <TvShowCard
              key={video.id}
              video={video}
              onOpen={(v) => void openDetail(v)}
              onToggleLike={scope === 'plaza' ? (v) => void handleToggleLike(v) : undefined}
              onDelete={scope === 'mine' ? (v) => void handleDelete(v) : undefined}
              liking={likingId === video.id}
              deleting={deletingId === video.id}
              showStatus={scope === 'mine'}
            />
          ))}
        </div>
      )}

      <Drawer
        width={420}
        title={detail?.title || t('videoGeneration.tvShow.detail.title', { defaultValue: '作品详情' })}
        visible={detail != null}
        onCancel={() => setDetail(null)}
        footer={null}
      >
        {detailLoading && !detail?.packageUrl ? (
          <div className='flex justify-center py-40px'>
            <Spin />
          </div>
        ) : detail ? (
          <div className='flex flex-col gap-14px'>
            {detail.coverUrl ? (
              <img
                src={detail.coverUrl}
                alt=''
                className='w-full rd-12px object-contain aspect-video bg-[var(--color-fill-2)]'
              />
            ) : null}
            <div className='text-13px text-[var(--color-text-3)]'>
              {tvShowWorkflowLabel(detail, t)}
              {detail.author?.name ? ` · ${detail.author.name}` : ''}
            </div>
            {detail.description ? (
              <p className='m-0 text-13px leading-[1.6] text-[var(--color-text-2)] whitespace-pre-wrap'>
                {detail.description}
              </p>
            ) : null}
            {detail.rejectReason && detail.status === 'offline' ? (
              <div className='text-12px text-[rgb(var(--danger-6))]'>{detail.rejectReason}</div>
            ) : null}
            <div className='flex flex-wrap gap-8px'>
              {detail.status === 'published' ? (
                <Button
                  type='outline'
                  size='small'
                  loading={likingId === detail.id}
                  onClick={() => void handleToggleLike(detail)}
                >
                  <span className='inline-flex items-center gap-4px'>
                    <Like
                      theme={detail.liked ? 'filled' : 'outline'}
                      size={14}
                      fill='currentColor'
                    />
                    {detail.likeCount ?? 0}
                  </span>
                </Button>
              ) : null}
              <Button
                type='primary'
                size='small'
                loading={importing}
                disabled={!detail.packageUrl && detailLoading}
                onClick={() => void handleImport()}
              >
                <span className='inline-flex items-center gap-4px'>
                  <Download theme='outline' size={14} fill='currentColor' />
                  {t('videoGeneration.tvShow.actions.import', {
                    defaultValue: '导入到本地',
                  })}
                </span>
              </Button>
            </div>
          </div>
        ) : null}
      </Drawer>
    </div>
  );
};

export default TvShowPanel;
