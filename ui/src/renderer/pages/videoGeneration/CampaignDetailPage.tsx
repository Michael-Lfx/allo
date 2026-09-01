
import React, { Suspense, lazy, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Drawer, Result, Spin, Tag } from '@arco-design/web-react';
import { ArrowLeft, Download, Like, Trophy } from '@icon-park/react';
import { isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import {
  getCampaignDetail,
  getTvShowDetail,
  importTvShow,
  likeTvShow,
  listCampaignSubmissions,
  listCampaignWinners,
  listMyTvShow,
  unlikeTvShow,
} from './api';
import { importCanvasTvShow } from '../videoCanvas/api';
import {
  campaignCountdownMs,
  campaignHomeSearch,
  formatCampaignRange,
  formatCountdown,
} from './campaign';
import type { CampaignDetail } from './types';
import type { TvShowVideo } from './types';
import { isCanvasTvShow, tvShowWorkflowLabel } from './components/SessionCard';
import TvShowCard from './components/TvShowCard';
import { phaseColor } from './components/CampaignCard';
import CampaignHtmlBody from './components/CampaignHtmlBody';
import pageStyles from './index.module.css';
import styles from './campaign.module.css';

const CampaignSubmitModal = lazy(() => import('./components/CampaignSubmitModal'));

const PAGE_SIZE = 16;

function awardClass(level: string | null | undefined): string {
  if (level === 'first') return styles.awardFirst;
  if (level === 'second') return styles.awardSecond;
  if (level === 'third') return styles.awardThird;
  return '';
}

const CampaignDetailPage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { id: idParam } = useParams();
  const campaignId = Number(idParam);
  const { status: cloudStatus, logout } = useCloudAuth();
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

  const loadCore = useCallback(async () => {
    if (!Number.isFinite(campaignId) || campaignId <= 0) {
      setError(t('videoGeneration.campaign.notFound', { defaultValue: '活动不存在' }));
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const [next, winnerData, mineData, submissionData] = await Promise.all([
        getCampaignDetail(campaignId),
        listCampaignWinners(campaignId).catch(() => ({ list: [] as TvShowVideo[], total: 0 })),
        listMyTvShow({ campaignId, page: 1, pageSize: 20 }).catch(
          () => ({ list: [] as TvShowVideo[], total: 0 })
        ),
        listCampaignSubmissions(campaignId, {
          page: 1,
          pageSize: PAGE_SIZE,
          sort: 'publishedAtDesc',
        }).catch(() => ({ list: [] as TvShowVideo[], total: 0 })),
      ]);
      setDetail(next);
      setWinners(winnerData.list ?? []);
      setMine(mineData.list ?? []);
      setSubmissions(submissionData.list ?? []);
      setSubmissionTotal(submissionData.total ?? 0);
      setError(null);
    } catch (e) {
      if (await consumeExpiredCloudSession(e)) return;
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [campaignId, consumeExpiredCloudSession, t]);

  useEffect(() => {
    if (cloudStatus !== 'authenticated') {
      setLoading(false);
      return;
    }
    void loadCore();
  }, [cloudStatus, loadCore]);

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

  const handleToggleLike = useCallback(
    async (video: TvShowVideo) => {
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
    [consumeExpiredCloudSession, likingId, message, t]
  );

  const handleImport = useCallback(async () => {
    if (!videoDetail || importing) return;
    setImporting(true);
    try {
      if (isCanvasTvShow(videoDetail)) {
        const imported = await importCanvasTvShow(videoDetail.id);
        message.success(
          t('videoGeneration.tvShow.actions.importOk', { defaultValue: '工程已导入到本地' })
        );
        setVideoDetail(null);
        navigate(`/video-generation/canvas/${encodeURIComponent(imported.project_id)}`);
        return;
      }
      const imported = await importTvShow(videoDetail.id);
      message.success(
        t('videoGeneration.tvShow.actions.importOk', { defaultValue: '工程已导入到本地' })
      );
      setVideoDetail(null);
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
  }, [consumeExpiredCloudSession, importing, message, navigate, t, videoDetail]);

  const loadMoreSubmissions = useCallback(async () => {
    if (submissions.length >= submissionTotal) return;
    const page = Math.floor(submissions.length / PAGE_SIZE) + 1;
    const data = await listCampaignSubmissions(campaignId, {
      page,
      pageSize: PAGE_SIZE,
      sort: 'publishedAtDesc',
    });
    setSubmissions((prev) => [...prev, ...(data.list ?? [])]);
    setSubmissionTotal(data.total ?? submissionTotal);
  }, [campaignId, submissionTotal, submissions.length]);

  const renderVideoGrid = (videos: TvShowVideo[], withAward: boolean) => (
    <div
      className='grid gap-12px'
      style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(min(300px, 100%), 1fr))' }}
    >
      {videos.map((video) => (
        <div key={video.id} className='relative'>
          {withAward && (video.awardLabel || video.awardLevel) ? (
            <span className={`${styles.awardBadge} ${awardClass(video.awardLevel)}`}>
              {video.awardLabel ||
                t(`videoGeneration.campaign.award.${video.awardLevel}`, {
                  defaultValue: video.awardLevel ?? '',
                })}
            </span>
          ) : null}
          <TvShowCard
            video={video}
            onOpen={(v) => void openVideo(v)}
            onToggleLike={(v) => void handleToggleLike(v)}
            liking={likingId === video.id}
            showStatus={Boolean(video.isMine && video.status !== 'published')}
          />
        </div>
      ))}
    </div>
  );

  if (cloudStatus === 'checking' || loading) {
    return (
      <div className={`${pageStyles.page} flex-1 min-h-0 size-full box-border overflow-y-auto px-16px py-24px`}>
        <div className='flex justify-center py-60px'>
          <Spin />
        </div>
      </div>
    );
  }

  if (cloudStatus !== 'authenticated') {
    return (
      <div className={`${pageStyles.page} flex-1 min-h-0 size-full box-border overflow-y-auto px-16px py-24px`}>
        {messageHolder}
        <div className='mx-auto flex w-full max-w-860px flex-col gap-16px'>
          <Button type='text' size='small' className='self-start' onClick={goBack}>
            <span className='inline-flex items-center gap-4px'>
              <ArrowLeft theme='outline' size={14} fill='currentColor' />
              {t('videoGeneration.campaign.back', { defaultValue: '返回' })}
            </span>
          </Button>
          <div className='flex items-center gap-12px rd-14px border border-dashed border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-16px py-18px'>
            <div className='min-w-0 flex-1'>
              <div className='text-13px font-600 text-[var(--color-text-1)]'>
                {t('videoGeneration.campaign.authRequired.title', {
                  defaultValue: '登录后查看活动',
                })}
              </div>
              <div className='mt-2px text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.campaign.authRequired.desc', {
                  defaultValue: '活动浏览与投稿需要云端账号。',
                })}
              </div>
            </div>
            <Button type='primary' size='small' onClick={() => navigate('/cloud-login')}>
              {t('videoGeneration.tvShow.authRequired.login', { defaultValue: '去登录' })}
            </Button>
          </div>
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
            <Button type='primary' onClick={() => setSubmitOpen(true)}>
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
            {renderVideoGrid(winners, true)}
          </section>
        ) : null}

        <section className='flex flex-col gap-12px'>
          <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
            {t('videoGeneration.campaign.submissionsTitle', { defaultValue: '活动作品' })}
          </h2>
          {submissions.length === 0 ? (
            <div className='flex items-center gap-12px rd-14px border border-dashed border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-16px py-18px'>
              <span className='flex h-38px w-38px shrink-0 items-center justify-center rd-11px bg-[rgba(var(--primary-6),0.1)] text-primary-6'>
                <Trophy theme='outline' size={19} fill='currentColor' />
              </span>
              <div className='text-13px text-[var(--color-text-3)]'>
                {t('videoGeneration.campaign.submissionsEmpty', {
                  defaultValue: '还没有上架作品，通过审核后会出现在这里。',
                })}
              </div>
            </div>
          ) : (
            <>
              {renderVideoGrid(submissions, false)}
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
            {renderVideoGrid(mine, false)}
          </section>
        ) : null}
      </div>

      <Drawer
        width={420}
        title={
          videoDetail?.title ||
          t('videoGeneration.tvShow.detail.title', { defaultValue: '作品详情' })
        }
        visible={videoDetail != null}
        onCancel={() => setVideoDetail(null)}
        footer={null}
      >
        {detailLoading && !videoDetail?.packageUrl ? (
          <div className='flex justify-center py-40px'>
            <Spin />
          </div>
        ) : videoDetail ? (
          <div className='flex flex-col gap-14px'>
            {videoDetail.coverUrl ? (
              <img
                src={videoDetail.coverUrl}
                alt=''
                className='w-full rd-12px object-contain aspect-video bg-[var(--color-fill-2)]'
              />
            ) : null}
            <div className='text-13px text-[var(--color-text-3)]'>
              {tvShowWorkflowLabel(videoDetail, t)}
              {videoDetail.author?.name ? ` · ${videoDetail.author.name}` : ''}
            </div>
            {videoDetail.description ? (
              <p className='m-0 text-13px leading-[1.6] text-[var(--color-text-2)] whitespace-pre-wrap'>
                {videoDetail.description}
              </p>
            ) : null}
            {videoDetail.rejectReason && videoDetail.status === 'offline' ? (
              <div className='text-12px text-danger-6'>{videoDetail.rejectReason}</div>
            ) : null}
            <div className='flex flex-wrap gap-8px'>
              {videoDetail.status === 'published' ? (
                <Button
                  type='outline'
                  size='small'
                  loading={likingId === videoDetail.id}
                  onClick={() => void handleToggleLike(videoDetail)}
                >
                  <span className='inline-flex items-center gap-4px'>
                    <Like
                      theme={videoDetail.liked ? 'filled' : 'outline'}
                      size={14}
                      fill='currentColor'
                    />
                    {videoDetail.likeCount ?? 0}
                  </span>
                </Button>
              ) : null}
              <Button
                type='primary'
                size='small'
                loading={importing}
                disabled={!videoDetail.packageUrl && detailLoading}
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

      {submitOpen ? (
        <Suspense fallback={null}>
          <CampaignSubmitModal
            campaignId={campaignId}
            visible={submitOpen}
            onClose={() => setSubmitOpen(false)}
            onSubmitted={() => void loadCore()}
          />
        </Suspense>
      ) : null}
    </div>
  );
};

export default CampaignDetailPage;
