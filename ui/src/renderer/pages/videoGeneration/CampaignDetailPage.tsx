
import React, { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Result, Spin, Tag } from '@arco-design/web-react';
import { ArrowLeft } from '@icon-park/react';
import { isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import {
  getCampaignDetail,
  getTvShowDetail,
  likeTvShow,
  listCampaignSubmissions,
  listCampaignWinners,
  listMyTvShow,
  remixTvShow,
  unlikeTvShow,
} from './api';
import {
  campaignCountdownMs,
  campaignHomeSearch,
  formatCampaignRange,
  formatCountdown,
  tvShowSortParam,
  uniqueVideosById,
  type TvShowListSort,
} from './campaign';
import type { CampaignDetail } from './types';
import type { TvShowVideo } from './types';
import TvShowCard from './components/TvShowCard';
import TvShowEmptyState from './components/TvShowEmptyState';
import TvShowInspector from './components/TvShowInspector';
import FilterPills from './components/FilterPills';
import { phaseColor } from './components/CampaignCard';
import CampaignHtmlBody from './components/CampaignHtmlBody';
import pageStyles from './index.module.css';
import styles from './campaign.module.css';

const CampaignSubmitModal = lazy(() => import('./components/CampaignSubmitModal'));

const PAGE_SIZE = 28;

const CampaignDetailPage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { id: idParam } = useParams();
  const campaignId = Number(idParam);
  const { status: cloudStatus, logout } = useCloudAuth();
  const authenticated = cloudStatus === 'authenticated';
  const [message, messageHolder] = useArcoMessage();

  const [detail, setDetail] = useState<CampaignDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [winners, setWinners] = useState<TvShowVideo[]>([]);
  const [submissions, setSubmissions] = useState<TvShowVideo[]>([]);
  const [submissionTotal, setSubmissionTotal] = useState(0);
  const [mine, setMine] = useState<TvShowVideo[]>([]);
  const [submitOpen, setSubmitOpen] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const [likingId, setLikingId] = useState<number | null>(null);
  const [videoDetail, setVideoDetail] = useState<TvShowVideo | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const [sort, setSort] = useState<TvShowListSort>('latest');
  const winnersRef = useRef<HTMLDivElement>(null);
  const mineRef = useRef<HTMLDivElement>(null);

  const consumeExpiredCloudSession = useCallback(
    async (cause: unknown): Promise<boolean> => {
      if (!isInvalidCloudSessionError(cause)) return false;
      await logout();
      return true;
    },
    [logout]
  );

  const loadCampaign = useCallback(async () => {
    if (!Number.isFinite(campaignId) || campaignId <= 0) {
      setError(t('videoGeneration.campaign.notFound', { defaultValue: '活动不存在' }));
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const [next, winnerData, mineData] = await Promise.all([
        getCampaignDetail(campaignId),
        listCampaignWinners(campaignId).catch(() => ({ list: [] as TvShowVideo[] })),
        authenticated
          ? listMyTvShow({ campaignId, page: 1, pageSize: 20 }).catch(
              () => ({ list: [] as TvShowVideo[] })
            )
          : Promise.resolve({ list: [] as TvShowVideo[] }),
      ]);
      setDetail(next);
      setWinners(winnerData.list ?? []);
      setMine(mineData.list ?? []);
      setError(null);
    } catch (e) {
      if (await consumeExpiredCloudSession(e)) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [authenticated, campaignId, consumeExpiredCloudSession, t]);

  const loadSubmissions = useCallback(async () => {
    if (!Number.isFinite(campaignId) || campaignId <= 0) return;
    const data = await listCampaignSubmissions(campaignId, {
      page: 1,
      pageSize: PAGE_SIZE,
      sort: tvShowSortParam(sort),
    }).catch(() => ({ list: [] as TvShowVideo[], total: 0 }));
    setSubmissions(data.list ?? []);
    setSubmissionTotal(data.total ?? 0);
  }, [campaignId, sort]);

  useEffect(() => {
    if (cloudStatus === 'checking') return;
    void loadCampaign();
  }, [cloudStatus, loadCampaign]);

  useEffect(() => {
    if (cloudStatus === 'checking') return;
    void loadSubmissions();
  }, [cloudStatus, loadSubmissions]);

  useEffect(() => {
    if (detail?.phase !== 'upcoming') return;
    const timer = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(timer);
  }, [detail?.phase]);

  const range = useMemo(() => {
    if (!detail) return '';
    return formatCampaignRange(detail.startAt, detail.endAt, i18n.language);
  }, [detail, i18n.language]);

  const countdown = useMemo(() => {
    if (!detail || detail.phase !== 'upcoming') return null;
    return formatCountdown(campaignCountdownMs(detail.startAt, now));
  }, [detail, now]);

  const goBack = useCallback(() => {
    const state = location.state as { fromSearch?: string } | null;
    navigate({
      pathname: '/video-generation',
      search: campaignHomeSearch(state?.fromSearch ?? location.search),
    });
  }, [location.search, location.state, navigate]);

  const openVideo = useCallback(
    async (video: TvShowVideo) => {
      setVideoDetail(video);
      setDetailLoading(true);
      try {
        setVideoDetail(await getTvShowDetail(video.id));
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

  const goLogin = useCallback(() => navigate('/cloud-login'), [navigate]);

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
        const patch = (list: TvShowVideo[]) =>
          list.map((v) =>
            v.id === video.id ? { ...v, liked: result.liked, likeCount: result.likeCount } : v
          );
        setSubmissions(patch);
        setWinners(patch);
        setMine(patch);
        setVideoDetail((prev) =>
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
    [authenticated, consumeExpiredCloudSession, goLogin, likingId, message, t]
  );

  const handleImport = useCallback(async () => {
    if (!videoDetail || importing) return;
    setImporting(true);
    try {
      const path = await remixTvShow(videoDetail);
      message.success(
        t('videoGeneration.tvShow.actions.importOk', { defaultValue: '工程已导入到本地' })
      );
      setVideoDetail(null);
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
  }, [consumeExpiredCloudSession, importing, message, navigate, t, videoDetail]);

  const loadMoreSubmissions = useCallback(async () => {
    if (submissions.length >= submissionTotal) return;
    const page = Math.floor(submissions.length / PAGE_SIZE) + 1;
    const data = await listCampaignSubmissions(campaignId, {
      page,
      pageSize: PAGE_SIZE,
      sort: tvShowSortParam(sort),
    });
    setSubmissions((prev) => [...prev, ...(data.list ?? [])]);
    setSubmissionTotal(data.total ?? submissionTotal);
  }, [campaignId, sort, submissionTotal, submissions.length]);

  const inspectable = useMemo(
    () => uniqueVideosById(winners, submissions, mine),
    [mine, submissions, winners]
  );
  const inspectorIndex = videoDetail
    ? inspectable.findIndex((v) => v.id === videoDetail.id)
    : -1;

  const renderVideoGrid = (videos: TvShowVideo[]) => (
    <div
      className='grid gap-12px'
      style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(min(280px, 100%), 1fr))' }}
    >
      {videos.map((video) => (
        <TvShowCard
          key={video.id}
          video={video}
          onOpen={(v) => void openVideo(v)}
          onToggleLike={(v) => void handleToggleLike(v)}
          liking={likingId === video.id}
          showStatus={Boolean(video.isMine && video.status !== 'published')}
        />
      ))}
    </div>
  );

  if (cloudStatus === 'checking' || (loading && !detail)) {
    return (
      <div className={`${pageStyles.page} flex-1 min-h-0 size-full box-border overflow-y-auto px-16px py-24px`}>
        <div className='flex justify-center py-60px'>
          <Spin />
        </div>
      </div>
    );
  }

  if (error || !detail) {
    return (
      <div className={`${pageStyles.page} flex-1 min-h-0 size-full box-border overflow-y-auto px-16px py-24px`}>
        {messageHolder}
        <div className='mx-auto w-full max-w-860px'>
          <Result
            status='error'
            title={t('videoGeneration.campaign.notFound', { defaultValue: '活动不存在' })}
            subTitle={error ?? ''}
            extra={
              <Button onClick={goBack}>
                {t('videoGeneration.campaign.back', { defaultValue: '返回' })}
              </Button>
            }
          />
        </div>
      </div>
    );
  }

  const phaseLabel = t(`videoGeneration.campaign.phase.${detail.phase}`, {
    defaultValue:
      detail.phase === 'ongoing'
        ? '进行中'
        : detail.phase === 'upcoming'
          ? '未开始'
          : detail.phase === 'ended'
            ? '已结束'
            : detail.phase,
  });

  return (
    <div
      className={`${pageStyles.page} flex-1 min-h-0 size-full box-border overflow-y-auto px-16px py-20px md:px-36px md:py-28px`}
    >
      {messageHolder}
      <div className='mx-auto flex w-full max-w-860px flex-col gap-18px'>
        <Button type='text' size='small' className='self-start' onClick={goBack}>
          <span className='inline-flex items-center gap-4px'>
            <ArrowLeft theme='outline' size={14} fill='currentColor' />
            {t('videoGeneration.campaign.back', { defaultValue: '返回活动' })}
          </span>
        </Button>

        {detail.coverUrl ? (
          <div className={styles.detailHero}>
            <img src={detail.coverUrl} alt='' className={styles.detailHeroMedia} />
          </div>
        ) : null}

        <div className='flex flex-col gap-8px'>
          <div className='flex flex-wrap items-center gap-8px'>
            <Tag size='small' color={phaseColor(detail.phase)}>
              {phaseLabel}
            </Tag>
            {detail.canSubmit ? (
              <Tag size='small' color='orangered'>
                {t('videoGeneration.campaign.canSubmit', { defaultValue: '可投稿' })}
              </Tag>
            ) : null}
          </div>
          <h1 className='m-0 text-22px font-700 leading-[1.25] text-[var(--color-text-1)] tracking-[-0.03em]'>
            {detail.title}
          </h1>
          {detail.summary ? (
            <p className='m-0 text-13px leading-[1.65] text-[var(--color-text-3)]'>{detail.summary}</p>
          ) : null}
          <div className='text-12px text-[var(--color-text-4)]'>
            {range}
            {countdown && (countdown.days > 0 || countdown.hours > 0 || countdown.minutes > 0) ? (
              <span>
                {' · '}
                {t('videoGeneration.campaign.countdown', {
                  days: countdown.days,
                  hours: countdown.hours,
                  minutes: countdown.minutes,
                  defaultValue: '{{days}} 天 {{hours}} 小时后开始',
                })}
              </span>
            ) : null}
          </div>
        </div>

        <div className='flex flex-wrap gap-8px'>
          {detail.canSubmit ? (
            <Button type='primary' onClick={() => (authenticated ? setSubmitOpen(true) : goLogin())}>
              {mine.length > 0
                ? t('videoGeneration.campaign.cta.submitAgain', { defaultValue: '更新投稿' })
                : t('videoGeneration.campaign.cta.submit', { defaultValue: '立即参与' })}
            </Button>
          ) : detail.phase === 'upcoming' ? (
            <Button type='primary' disabled>
              {t('videoGeneration.campaign.cta.upcoming', { defaultValue: '活动未开始' })}
            </Button>
          ) : detail.phase === 'ended' && winners.length > 0 ? (
            <Button type='primary' onClick={() => winnersRef.current?.scrollIntoView({ behavior: 'smooth' })}>
              {t('videoGeneration.campaign.cta.winners', { defaultValue: '查看获奖作品' })}
            </Button>
          ) : null}
          {mine.length > 0 ? (
            <Button
              type='outline'
              onClick={() => mineRef.current?.scrollIntoView({ behavior: 'smooth' })}
            >
              {t('videoGeneration.campaign.cta.mine', { defaultValue: '查看我的投稿' })}
            </Button>
          ) : null}
        </div>

        {detail.content ? <CampaignHtmlBody html={detail.content} /> : null}

        {winners.length > 0 ? (
          <section ref={winnersRef} className='flex flex-col gap-12px'>
            <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
              {t('videoGeneration.campaign.winnersTitle', { defaultValue: '获奖作品' })}
            </h2>
            {renderVideoGrid(winners)}
          </section>
        ) : null}

        <section className='flex flex-col gap-12px'>
          <div className='flex flex-wrap items-center justify-between gap-8px'>
            <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
              {t('videoGeneration.campaign.submissionsTitle', { defaultValue: '活动作品' })}
            </h2>
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
          </div>
          {submissions.length === 0 ? (
            <TvShowEmptyState
              title={t('videoGeneration.campaign.submissionsEmptyTitle', {
                defaultValue: '还没有上架作品',
              })}
              desc={t('videoGeneration.campaign.submissionsEmpty', {
                defaultValue: '通过审核后会出现在这里。',
              })}
              action={
                detail.canSubmit ? (
                  <Button
                    type='primary'
                    size='small'
                    onClick={() => (authenticated ? setSubmitOpen(true) : goLogin())}
                  >
                    {t('videoGeneration.campaign.cta.submit', { defaultValue: '立即参与' })}
                  </Button>
                ) : undefined
              }
            />
          ) : (
            <>
              {renderVideoGrid(submissions)}
              {submissions.length < submissionTotal ? (
                <div className='flex justify-center'>
                  <Button size='small' onClick={() => void loadMoreSubmissions()}>
                    {t('videoGeneration.campaign.loadMore', { defaultValue: '加载更多' })}
                  </Button>
                </div>
              ) : null}
            </>
          )}
        </section>

        {mine.length > 0 ? (
          <section ref={mineRef} className='flex flex-col gap-12px'>
            <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
              {t('videoGeneration.campaign.mineTitle', { defaultValue: '我的投稿' })}
            </h2>
            {renderVideoGrid(mine)}
          </section>
        ) : null}
      </div>

      <TvShowInspector
        video={videoDetail}
        loading={detailLoading}
        importing={importing}
        liking={likingId === videoDetail?.id}
        authenticated={authenticated}
        onClose={() => setVideoDetail(null)}
        onImport={() => void handleImport()}
        onToggleLike={() => {
          if (videoDetail) void handleToggleLike(videoDetail);
        }}
        onLogin={goLogin}
        onPrev={
          inspectorIndex > 0
            ? () => void openVideo(inspectable[inspectorIndex - 1])
            : undefined
        }
        onNext={
          inspectorIndex >= 0 && inspectorIndex < inspectable.length - 1
            ? () => void openVideo(inspectable[inspectorIndex + 1])
            : undefined
        }
      />

      {submitOpen ? (
        <Suspense fallback={null}>
          <CampaignSubmitModal
            campaignId={campaignId}
            visible={submitOpen}
            onClose={() => setSubmitOpen(false)}
            onSubmitted={() => {
              void loadCampaign();
              void loadSubmissions();
            }}
          />
        </Suspense>
      ) : null}
    </div>
  );
};

export default CampaignDetailPage;
