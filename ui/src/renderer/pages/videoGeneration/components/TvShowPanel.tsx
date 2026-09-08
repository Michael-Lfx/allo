
import React, { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Result, Spin } from '@arco-design/web-react';
import { Search } from '@icon-park/react';
import { isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import {
  deleteTvShow,
  getTvShowDetail,
  likeTvShow,
  listMyTvShow,
  listTvShow,
  remixTvShow,
  unlikeTvShow,
} from '../api';
import type { TvShowVideo } from '../types';
import {
  parseTvShowTab,
  tvShowSortParam,
  uniqueVideosById,
  writeTvShowTab,
  type TvShowChannel,
  type TvShowListSort,
  type TvShowTab,
} from '../campaign';
import FilterPills from './FilterPills';
import TvShowCard from './TvShowCard';
import TvShowEmptyState from './TvShowEmptyState';
import TvShowInspector from './TvShowInspector';

const CampaignPanel = lazy(() => import('./CampaignPanel'));

const PAGE_SIZE = 28;
const FEATURED_PAGE_SIZE = 8;

interface TvShowPanelProps {
  enabled: boolean;
}

const TvShowPanel: React.FC<TvShowPanelProps> = ({ enabled }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const { status: cloudStatus, logout } = useCloudAuth();
  const [message, messageHolder] = useArcoMessage();
  const authenticated = cloudStatus === 'authenticated';

  const urlTab = parseTvShowTab(searchParams.get('tvScope'), searchParams.get('tvChannel'));
  const [tab, setTab] = useState<TvShowTab>(urlTab);
  const scope = tab === 'campaign' || tab === 'mine' ? tab : 'plaza';
  const channel: TvShowChannel = tab === 'campaign' || tab === 'mine' ? 'all' : tab;
  const [videos, setVideos] = useState<TvShowVideo[]>([]);
  const [featured, setFeatured] = useState<TvShowVideo[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [keywordInput, setKeywordInput] = useState('');
  const [keyword, setKeyword] = useState('');
  const [sort, setSort] = useState<TvShowListSort>('latest');
  const [likingId, setLikingId] = useState<number | null>(null);
  const [deletingId, setDeletingId] = useState<number | null>(null);
  const [detail, setDetail] = useState<TvShowVideo | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const videosRef = useRef(videos);
  videosRef.current = videos;

  const tabItems = useMemo(
    () =>
      [
        { key: 'all' as const, label: t('videoGeneration.tvShow.channel.all', { defaultValue: '全部' }) },
        { key: 'idea2video' as const, label: t('videoGeneration.tvShow.channel.idea2video', { defaultValue: '一句话' }) },
        { key: 'script2video' as const, label: t('videoGeneration.tvShow.channel.script2video', { defaultValue: '剧本' }) },
        { key: 'novel2video' as const, label: t('videoGeneration.tvShow.channel.novel2video', { defaultValue: '小说' }) },
        { key: 'canvas' as const, label: t('videoGeneration.tvShow.channel.canvas', { defaultValue: '画布' }) },
        { key: 'action2video' as const, label: t('videoGeneration.tvShow.channel.action2video', { defaultValue: '动作' }) },
        { key: 'campaign' as const, label: t('videoGeneration.tvShow.channel.campaign', { defaultValue: '活动' }) },
        { key: 'mine' as const, label: t('videoGeneration.tvShow.channel.mine', { defaultValue: '我的' }) },
      ] satisfies { key: TvShowTab; label: string }[],
    [t]
  );

  const consumeExpiredCloudSession = useCallback(
    async (cause: unknown): Promise<boolean> => {
      if (!isInvalidCloudSessionError(cause)) return false;
      await logout();
      return true;
    },
    [logout]
  );

  const goLogin = useCallback(() => navigate('/cloud-login'), [navigate]);

  const plazaQuery = useCallback(
    (page: number) =>
      listTvShow({
        page,
        pageSize: PAGE_SIZE,
        keyword: keyword || undefined,
        sort: tvShowSortParam(sort),
        workflow: channel === 'all' ? undefined : channel,
      }),
    [channel, keyword, sort]
  );

  const refresh = useCallback(async () => {
    if (!enabled || scope === 'campaign') return;
    if (scope === 'mine' && !authenticated) {
      setVideos([]);
      setFeatured([]);
      setTotal(0);
      setError(null);
      setLoading(false);
      return;
    }
    const showSpinner = videosRef.current.length === 0;
    if (showSpinner) setLoading(true);
    try {
      const data =
        scope === 'mine'
          ? await listMyTvShow({ page: 1, pageSize: PAGE_SIZE })
          : await plazaQuery(1);
      setVideos(data.list ?? []);
      setTotal(data.total ?? 0);
      setError(null);
    } catch (e) {
      console.error('[videoGeneration] TV Show list failed', e);
      if (await consumeExpiredCloudSession(e)) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [authenticated, consumeExpiredCloudSession, enabled, plazaQuery, scope]);

  const loadMore = useCallback(async () => {
    if (loadingMore || videos.length >= total || scope === 'campaign') return;
    if (scope === 'mine' && !authenticated) return;
    setLoadingMore(true);
    try {
      const nextPage = Math.floor(videos.length / PAGE_SIZE) + 1;
      const data =
        scope === 'mine'
          ? await listMyTvShow({ page: nextPage, pageSize: PAGE_SIZE })
          : await plazaQuery(nextPage);
      setVideos((prev) => [...prev, ...(data.list ?? [])]);
      setTotal(data.total ?? total);
    } catch (e) {
      if (await consumeExpiredCloudSession(e)) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoadingMore(false);
    }
  }, [authenticated, consumeExpiredCloudSession, loadingMore, plazaQuery, scope, total, videos.length]);

  const handleTabChange = useCallback(
    (next: TvShowTab) => {
      if (next === tab) return;
      setTab(next);
      setVideos([]);
      setSearchParams(
        (prev) => {
          const nextParams = new URLSearchParams(prev);
          writeTvShowTab(nextParams, next);
          return nextParams;
        },
        { replace: true }
      );
    },
    [setSearchParams, tab]
  );

  useEffect(() => {
    setTab(urlTab);
  }, [urlTab]);

  useEffect(() => {
    const timer = window.setTimeout(() => setKeyword(keywordInput.trim()), 300);
    return () => window.clearTimeout(timer);
  }, [keywordInput]);

  useEffect(() => {
    if (!enabled) return;
    void refresh();
  }, [enabled, refresh]);

  useEffect(() => {
    if (!enabled || scope !== 'plaza' || channel !== 'all' || keyword) {
      setFeatured([]);
      return;
    }
    let cancelled = false;
    void listTvShow({
      page: 1,
      pageSize: FEATURED_PAGE_SIZE,
      campaignId: 0,
      awardLevel: 'featured',
    })
      .then((data) => {
        if (!cancelled) setFeatured(data.list ?? []);
      })
      .catch(() => {
        if (!cancelled) setFeatured([]);
      });
    return () => {
      cancelled = true;
    };
  }, [channel, enabled, keyword, scope]);

  const featuredIds = useMemo(() => new Set(featured.map((v) => v.id)), [featured]);
  const displayed = videos.filter((v) => {
    if (scope === 'plaza' && featuredIds.has(v.id)) return false;
    if (scope !== 'mine') return true;
    const q = keywordInput.trim().toLowerCase();
    if (!q) return true;
    return (
      (v.title ?? '').toLowerCase().includes(q) ||
      String(v.workflow).toLowerCase().includes(q) ||
      String(v.status).toLowerCase().includes(q)
    );
  });

  const inspectorItems = uniqueVideosById(featured, displayed);
  const inspectorIndex = detail ? inspectorItems.findIndex((v) => v.id === detail.id) : -1;

  const openDetail = useCallback(
    async (video: TvShowVideo) => {
      setDetail(video);
      setDetailLoading(true);
      try {
        setDetail(await getTvShowDetail(video.id));
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

  const patchVideo = useCallback((id: number, liked: boolean, likeCount: number) => {
    const apply = (list: TvShowVideo[]) =>
      list.map((v) => (v.id === id ? { ...v, liked, likeCount } : v));
    setVideos(apply);
    setFeatured(apply);
    setDetail((prev) => (prev && prev.id === id ? { ...prev, liked, likeCount } : prev));
  }, []);

  const handleToggleLike = useCallback(
    async (video: TvShowVideo) => {
      if (!authenticated) {
        goLogin();
        return;
      }
      if (likingId != null) return;
      setLikingId(video.id);
      try {
        const result = video.liked ? await unlikeTvShow(video.id) : await likeTvShow(video.id);
        patchVideo(video.id, result.liked, result.likeCount);
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
    [authenticated, consumeExpiredCloudSession, goLogin, likingId, message, patchVideo, t]
  );

  const handleDelete = useCallback(
    async (video: TvShowVideo) => {
      if (deletingId != null) return;
      setDeletingId(video.id);
      try {
        await deleteTvShow(video.id);
        setVideos((prev) => prev.filter((v) => v.id !== video.id));
        setFeatured((prev) => prev.filter((v) => v.id !== video.id));
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
      const path = await remixTvShow(detail);
      message.success(
        t('videoGeneration.tvShow.actions.importOk', { defaultValue: '工程已导入到本地' })
      );
      setDetail(null);
      navigate(path);
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

  const createCta = (
    <Button type='primary' size='small' onClick={() => window.scrollTo({ top: 0, behavior: 'smooth' })}>
      {t('videoGeneration.tvShow.empty.create', { defaultValue: '去创作' })}
    </Button>
  );

  const renderGrid = (list: TvShowVideo[], mine: boolean) => (
    <div
      className='grid gap-12px'
      style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(min(280px, 100%), 1fr))' }}
    >
      {list.map((video) => (
        <TvShowCard
          key={video.id}
          video={video}
          onOpen={(v) => void openDetail(v)}
          onToggleLike={(v) => void handleToggleLike(v)}
          onDelete={mine ? (v) => void handleDelete(v) : undefined}
          liking={likingId === video.id}
          deleting={deletingId === video.id}
          showStatus={mine}
        />
      ))}
    </div>
  );

  if (cloudStatus === 'checking' && scope === 'mine') {
    return (
      <div className='flex justify-center py-38px'>
        <Spin />
      </div>
    );
  }

  return (
    <div className='flex flex-col gap-12px'>
      {messageHolder}
      <div className='flex flex-wrap items-center justify-between gap-12px'>
        <FilterPills items={tabItems} active={tab} onChange={handleTabChange} />
        {scope === 'campaign' ? null : (
          <div className='flex flex-wrap items-center gap-8px'>
            {scope === 'plaza' ? (
              <FilterPills
                items={[
                  {
                    key: 'latest',
                    label: t('videoGeneration.tvShow.sort.latest', { defaultValue: '最新' }),
                  },
                  {
                    key: 'likes',
                    label: t('videoGeneration.tvShow.sort.likes', { defaultValue: '最多赞' }),
                  },
                ]}
                active={sort}
                onChange={setSort}
              />
            ) : null}
            <div className='flex w-220px items-center gap-8px rd-10px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-11px py-7px'>
              <Search theme='outline' size={14} className='flex-none text-[var(--color-text-3)]' />
              <input
                className='w-full border-none bg-transparent text-13px text-[var(--color-text-1)] outline-none font-[inherit] placeholder:text-[var(--color-text-3)]'
                placeholder={t('videoGeneration.tvShow.searchPlaceholder', {
                  defaultValue: '搜索作品...',
                })}
                value={keywordInput}
                onChange={(event) => setKeywordInput(event.target.value)}
              />
            </div>
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
      ) : scope === 'mine' && !authenticated ? (
        <TvShowEmptyState
          title={t('videoGeneration.tvShow.authRequired.mineTitle', {
            defaultValue: '登录后查看我的发布',
          })}
          desc={t('videoGeneration.tvShow.authRequired.mineDesc', {
            defaultValue: '点赞、投稿和导入需要云端账号。',
          })}
          action={
            <Button type='primary' size='small' onClick={goLogin}>
              {t('videoGeneration.tvShow.authRequired.login', { defaultValue: '去登录' })}
            </Button>
          }
        />
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
      ) : (
        <>
          {scope === 'plaza' && featured.length > 0 ? (
            <section className='flex flex-col gap-10px'>
              <h2 className='m-0 text-15px font-650 text-[var(--color-text-1)]'>
                {t('videoGeneration.tvShow.featuredTitle', { defaultValue: '精选' })}
              </h2>
              {renderGrid(featured, false)}
            </section>
          ) : null}
          {displayed.length === 0 && featured.length === 0 ? (
            <TvShowEmptyState
              title={
                scope === 'mine'
                  ? t('videoGeneration.tvShow.empty.mineTitle')
                  : channel === 'all'
                    ? t('videoGeneration.tvShow.empty.plazaTitle')
                    : t('videoGeneration.tvShow.empty.channelTitle', {
                        defaultValue: '这个频道还没有作品',
                      })
              }
              desc={
                scope === 'mine'
                  ? t('videoGeneration.tvShow.empty.mineDesc')
                  : channel === 'all'
                    ? t('videoGeneration.tvShow.empty.plazaDesc')
                    : t('videoGeneration.tvShow.empty.channelDesc', {
                        defaultValue: '换个频道，或自己发一支。',
                      })
              }
              action={createCta}
            />
          ) : displayed.length > 0 ? (
            <>
              {renderGrid(displayed, scope === 'mine')}
            </>
          ) : null}
          {videos.length < total ? (
            <div className='flex justify-center pt-4px'>
              <Button size='small' loading={loadingMore} onClick={() => void loadMore()}>
                {t('videoGeneration.tvShow.loadMore', { defaultValue: '加载更多' })}
              </Button>
            </div>
          ) : null}
        </>
      )}

      <TvShowInspector
        video={detail}
        loading={detailLoading}
        importing={importing}
        liking={likingId === detail?.id}
        authenticated={authenticated}
        onClose={() => setDetail(null)}
        onImport={() => void handleImport()}
        onToggleLike={() => {
          if (detail) void handleToggleLike(detail);
        }}
        onLogin={goLogin}
        onPrev={
          inspectorIndex > 0
            ? () => void openDetail(inspectorItems[inspectorIndex - 1])
            : undefined
        }
        onNext={
          inspectorIndex >= 0 && inspectorIndex < inspectorItems.length - 1
            ? () => void openDetail(inspectorItems[inspectorIndex + 1])
            : undefined
        }
      />
    </div>
  );
};

export default TvShowPanel;
