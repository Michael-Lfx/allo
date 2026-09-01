import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Spin } from '@arco-design/web-react';
import { ArrowLeft, Download } from '@icon-park/react';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import BriefingModelFields from '../home/BriefingModelFields';
import type { BriefingModelPick } from '../home/types';
import {
  cancelBriefing,
  getBriefing,
  getBriefingPlan,
  getBriefingScript,
  getBriefingStatus,
  briefingArtifactUrl,
  runBriefing,
  updateBriefingModels,
  type BriefingPlan,
  type BriefingScript,
  type BriefingSession,
} from './api';
import styles from './briefing.module.css';

const STAGE_ORDER = ['researching', 'scripting', 'aligning', 'composing'] as const;
const CARD_KEYS = [
  'title_desk',
  'evidence_tour',
  'highlighter',
  'number_roll',
  'source_bar',
  'yield_shrink',
  'transition_wipe',
  'subtitle_plain',
] as const;

function sessionTts(session: BriefingSession): BriefingModelPick | null {
  if (!session.tts_provider_id || !session.tts_model) return null;
  return {
    provider_id: session.tts_provider_id,
    model: session.tts_model,
    voice: session.tts_voice ?? null,
  };
}

function sessionImage(session: BriefingSession): BriefingModelPick | null {
  if (!session.image_provider_id || !session.image_model) return null;
  return {
    provider_id: session.image_provider_id,
    model: session.image_model,
  };
}

function statusIndex(status: string): number {
  const idx = STAGE_ORDER.indexOf(status as (typeof STAGE_ORDER)[number]);
  if (status === 'succeeded') return STAGE_ORDER.length;
  return idx;
}

function statusTone(status: string): string {
  if (STAGE_ORDER.includes(status as (typeof STAGE_ORDER)[number])) return styles.statusRunning;
  if (status === 'succeeded') return styles.statusOk;
  if (status === 'hold') return styles.statusHold;
  if (status === 'failed' || status === 'cancelled' || status === 'interrupted') {
    return styles.statusFail;
  }
  return '';
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return url;
  }
}

export default function BriefingWorkspacePage() {
  const { id = '' } = useParams();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [session, setSession] = useState<BriefingSession | null>(null);
  const [plan, setPlan] = useState<BriefingPlan | null>(null);
  const [script, setScript] = useState<BriefingScript | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [tts, setTts] = useState<BriefingModelPick | null>(null);
  const [image, setImage] = useState<BriefingModelPick | null>(null);
  const autoStartedRef = useRef(false);

  const load = useCallback(async () => {
    if (!id) return;
    const [nextSession, nextPlan] = await Promise.all([
      getBriefing(id),
      getBriefingPlan(id).catch(() => null),
    ]);
    setSession(nextSession);
    setPlan(nextPlan);
    setTts(sessionTts(nextSession));
    setImage(sessionImage(nextSession));
    try {
      const nextScript = await getBriefingScript(id);
      setScript(nextScript.beats.length > 0 ? nextScript : null);
    } catch {
      setScript(null);
    }
  }, [id]);

  useEffect(() => {
    void load().catch((err: unknown) => {
      setError(err instanceof Error ? err.message : String(err));
    });
  }, [load]);

  useEffect(() => {
    if (!id || !session) return;
    const active = STAGE_ORDER.includes(session.status as (typeof STAGE_ORDER)[number]);
    if (!active) return;
    const timer = window.setInterval(() => {
      void getBriefingStatus(id)
        .then((snapshot) => {
          setSession((current) =>
            current
              ? {
                  ...current,
                  status: snapshot.status,
                  stage: snapshot.stage,
                  summary: snapshot.message,
                  final_video: snapshot.final_video,
                }
              : current
          );
          if (
            snapshot.status === 'succeeded' ||
            snapshot.status === 'hold' ||
            snapshot.status === 'failed' ||
            snapshot.status === 'cancelled'
          ) {
            void load();
          }
        })
        .catch(() => undefined);
    }, 1500);
    return () => window.clearInterval(timer);
  }, [id, load, session?.status]);

  useEffect(() => {
    if (!id || !session || script) return;
    const status = String(session.status);
    if (status !== 'aligning' && status !== 'composing' && status !== 'succeeded') return;
    void getBriefingScript(id)
      .then((next) => {
        if (next.beats.length > 0) setScript(next);
      })
      .catch(() => undefined);
  }, [id, session?.status, script]);

  const running = STAGE_ORDER.includes(String(session?.status) as (typeof STAGE_ORDER)[number]);
  const busyRef = useRef(false);

  const kickoff = useCallback(
    async (persistModels: boolean) => {
      if (!id || busyRef.current) return;
      busyRef.current = true;
      setBusy(true);
      setError(null);
      try {
        if (persistModels) {
          await updateBriefingModels(id, {
            tts_provider_id: tts?.provider_id ?? null,
            tts_model: tts?.model ?? null,
            tts_voice: tts?.voice ?? null,
            image_provider_id: image?.provider_id ?? null,
            image_model: image?.model ?? null,
          });
        }
        await runBriefing(id);
        trackFunnelEvent('render_started', {
          feature: 'video_generation',
          mode: 'briefing',
          workflow: 'news_briefing',
          briefing_id: id,
          session_id: id,
        });
        await load();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        busyRef.current = false;
        setBusy(false);
      }
    },
    [id, image, load, tts]
  );

  useEffect(() => {
    autoStartedRef.current = false;
  }, [id]);

  useEffect(() => {
    if (!id || !session || session.id !== id || autoStartedRef.current) return;
    if (String(session.status) !== 'idle') return;
    autoStartedRef.current = true;
    void kickoff(false);
  }, [id, kickoff, session]);

  const onRun = () => {
    void kickoff(true);
  };

  const onCancel = async () => {
    if (!id) return;
    setBusy(true);
    try {
      await cancelBriefing(id);
      await load();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const videoSrc = session?.final_video ? briefingArtifactUrl(id) : null;
  const currentStage = statusIndex(String(session?.status ?? 'idle'));
  const stageCopy = useMemo(
    () =>
      STAGE_ORDER.map((key) => ({
        key,
        label: t(`videoGeneration.briefing.stage.${key}`),
        hint: t(`videoGeneration.briefing.stageHint.${key}`),
      })),
    [t]
  );

  const emptyCopy = (() => {
    const status = String(session?.status ?? 'idle');
    if (status === 'hold') {
      return {
        kicker: t('videoGeneration.briefing.deskLive'),
        title: t('videoGeneration.briefing.holdTitle'),
        body: t('videoGeneration.briefing.holdBody'),
      };
    }
    if (status === 'failed') {
      return {
        kicker: t('videoGeneration.briefing.deskLive'),
        title: t('videoGeneration.briefing.failedTitle'),
        body: session?.summary || t('videoGeneration.briefing.failedBody'),
      };
    }
    if (status === 'cancelled' || status === 'interrupted') {
      return {
        kicker: t('videoGeneration.briefing.deskLive'),
        title: t('videoGeneration.briefing.cancelledTitle'),
        body: t('videoGeneration.briefing.cancelledBody'),
      };
    }
    if (running) {
      return {
        kicker: t('videoGeneration.briefing.deskLive'),
        title: t('videoGeneration.briefing.runningTitle'),
        body: session?.summary || t('videoGeneration.briefing.noVideo'),
      };
    }
    return {
      kicker: t('videoGeneration.briefing.deskLive'),
      title: t('videoGeneration.briefing.waitingTitle'),
      body: t('videoGeneration.briefing.noVideo'),
    };
  })();

  if (error && !session) {
    return (
      <div className={styles.errorPage}>
        <p>{error}</p>
        <Button onClick={() => navigate('/video-generation?mode=briefing')}>
          {t('videoGeneration.briefing.back')}
        </Button>
      </div>
    );
  }

  if (!session) {
    return (
      <div className={styles.loadingPage}>
        <Spin />
      </div>
    );
  }

  const status = String(session.status);

  return (
    <div className={styles.page}>
      <div className={styles.shell}>
        <header className={styles.masthead}>
          <div className={styles.mastheadCopy}>
            <p className={styles.kicker}>{t('videoGeneration.briefing.kicker')}</p>
            <h1 className={styles.title}>{session.title}</h1>
            <div className={styles.metaRow}>
              <span className={`${styles.statusPill} ${statusTone(status)}`}>
                {t(`videoGeneration.briefing.status.${status}`, { defaultValue: status })}
              </span>
              <span className={styles.chip}>
                {t('videoGeneration.briefing.formatSecs', { secs: session.format_secs })}
              </span>
              <span className={styles.chip}>
                {t(`videoGeneration.briefing.${session.research_depth === 'deep' ? 'deep' : 'fast'}`)}
              </span>
              <span className={styles.chip}>
                {t('videoGeneration.briefing.windowChip', { hours: session.time_window_hours })}
              </span>
              <span className={styles.chip}>
                {t('videoGeneration.briefing.sourceCount', { count: session.source_urls.length })}
              </span>
            </div>
            {session.summary ? <p className={styles.summary}>{session.summary}</p> : null}
          </div>
          <div className={styles.actions}>
            <Button
              size='small'
              className='flowy-icon-text-btn'
              icon={<ArrowLeft theme='outline' size='14' />}
              onClick={() => navigate('/video-generation?mode=briefing')}
            >
              {t('videoGeneration.briefing.back')}
            </Button>
            {videoSrc ? (
              <Button
                size='small'
                className='flowy-icon-text-btn'
                icon={<Download theme='outline' size='14' />}
                href={videoSrc}
                target='_blank'
              >
                {t('videoGeneration.briefing.download')}
              </Button>
            ) : null}
            {running ? (
              <Button
                size='small'
                disabled={!running && !busy}
                loading={busy && running}
                onClick={() => void onCancel()}
              >
                {t('videoGeneration.briefing.cancel')}
              </Button>
            ) : (
              <Button
                type='primary'
                size='small'
                loading={busy}
                onClick={() => void onRun()}
              >
                {t(
                  status === 'idle'
                    ? 'videoGeneration.briefing.run'
                    : 'videoGeneration.briefing.runAgain'
                )}
              </Button>
            )}
          </div>
        </header>

        {error ? <p className={styles.errorBanner}>{error}</p> : null}

        <div className={styles.stageRail} aria-label={t('videoGeneration.briefing.pipeline')}>
          {stageCopy.map((stage, index) => {
            const done = currentStage > index;
            const active = currentStage === index;
            return (
              <div
                key={stage.key}
                className={`${styles.stage} ${done ? styles.stageDone : ''} ${
                  active ? styles.stageActive : ''
                }`}
              >
                <span className={styles.stageIndex}>{String(index + 1).padStart(2, '0')}</span>
                <span className={styles.stageLabel}>{stage.label}</span>
                <span className={styles.stageHint}>{stage.hint}</span>
              </div>
            );
          })}
        </div>

        <div className={styles.layout}>
          <section className={styles.playerCard}>
            {videoSrc ? (
              <video className={styles.player} controls src={videoSrc} />
            ) : (
              <div className={styles.playerEmpty}>
                <p className={styles.playerEmptyKicker}>{emptyCopy.kicker}</p>
                <p className={styles.playerEmptyTitle}>{emptyCopy.title}</p>
                <p className={styles.playerEmptyBody}>{emptyCopy.body}</p>
              </div>
            )}
          </section>

          <aside className={styles.panel}>
            <h2 className={styles.panelTitle}>{t('videoGeneration.briefing.models')}</h2>
            <BriefingModelFields
              tts={tts}
              image={image}
              disabled={running || busy}
              onTts={setTts}
              onImage={setImage}
            />
            <h2 className={styles.panelTitle}>{t('videoGeneration.briefing.intent')}</h2>
            <p className={styles.intent}>{session.intent}</p>
            {session.source_urls.length > 0 ? (
              <>
                <h2 className={styles.panelTitle}>{t('videoGeneration.briefing.sourcesTitle')}</h2>
                <div className={styles.sourceList}>
                  {session.source_urls.map((url) => (
                    <a
                      key={url}
                      className={styles.sourceChip}
                      href={url}
                      target='_blank'
                      rel='noreferrer'
                    >
                      {hostOf(url)}
                    </a>
                  ))}
                </div>
              </>
            ) : null}
            {plan ? (
              <>
                <h2 className={styles.panelTitle}>{t('videoGeneration.briefing.plan')}</h2>
                <ul className={styles.list}>
                  {plan.questions.map((question) => (
                    <li key={question}>{question}</li>
                  ))}
                </ul>
              </>
            ) : null}
          </aside>
        </div>

        {script ? (
          <section className={styles.rundown}>
            <div className={styles.rundownHead}>
              <h2 className={styles.panelTitle}>{t('videoGeneration.briefing.beats')}</h2>
              <span className={styles.beatCount}>
                {t('videoGeneration.briefing.beatCount', { count: script.beats.length })}
              </span>
            </div>
            {script.beats.map((beat, index) => {
              const cardKey = CARD_KEYS.includes(beat.card as (typeof CARD_KEYS)[number])
                ? beat.card
                : null;
              return (
                <article key={beat.id} className={styles.beat}>
                  <span className={styles.beatIndex}>{String(index + 1).padStart(2, '0')}</span>
                  <span className={styles.beatCard}>
                    {cardKey
                      ? t(`videoGeneration.briefing.card.${cardKey}`)
                      : beat.card}
                  </span>
                  <p className={styles.beatText}>
                    <strong>{t('videoGeneration.briefing.spoken')} · </strong>
                    {beat.spoken_text}
                  </p>
                  {beat.on_screen ? (
                    <p className={styles.beatScreen}>
                      {t('videoGeneration.briefing.onScreen')} · {beat.on_screen}
                    </p>
                  ) : null}
                  {beat.citations.length > 0 ? (
                    <div className={styles.cite}>
                      {beat.citations.map((citation) => (
                        <a key={citation.url} href={citation.url} target='_blank' rel='noreferrer'>
                          {citation.domain}
                        </a>
                      ))}
                    </div>
                  ) : null}
                </article>
              );
            })}
          </section>
        ) : null}
      </div>
    </div>
  );
}
