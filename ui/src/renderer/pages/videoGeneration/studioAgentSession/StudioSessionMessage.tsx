import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Attention, PlayOne, Robot, User } from '@icon-park/react';
import { loadArtifactMediaUrlCached } from '../api';
import { loadStudioMediaPreviewUrl } from './collectStudioMedia';
import type { FailureKind } from '../classifyFailure';
import type { StudioSessionMedia, StudioSessionMessage } from './types';
import styles from './index.module.css';

const BRIEF_LIMIT = 280;

const FilmPreview: React.FC<{
  sessionId: string;
  items: StudioSessionMedia[];
  onOpen?: (item: StudioSessionMedia, gallery: StudioSessionMedia[]) => void;
}> = ({ sessionId, items, onOpen }) => {
  const { t } = useTranslation();
  const video = items.find((item) => item.kind === 'video');
  const cover = items.find((item) => item.kind === 'image');
  const videoPath = video?.path;
  const coverPath = cover?.path;
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [coverUrl, setCoverUrl] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (videoPath) {
      void loadArtifactMediaUrlCached(sessionId, videoPath)
        .then((next) => {
          if (!cancelled) setVideoUrl(next);
        })
        .catch(() => {
          if (!cancelled) setVideoUrl(null);
        });
    } else {
      setVideoUrl(null);
    }
    if (coverPath) {
      void loadArtifactMediaUrlCached(sessionId, coverPath)
        .then((next) => {
          if (!cancelled) setCoverUrl(next);
        })
        .catch(() => {
          if (!cancelled) setCoverUrl(null);
        });
    } else {
      setCoverUrl(null);
    }
    return () => {
      cancelled = true;
    };
  }, [sessionId, videoPath, coverPath]);

  const target = video ?? cover;
  if (!target) return null;

  return (
    <button
      type='button'
      className={styles.filmPreview}
      onClick={() => onOpen?.(target, items)}
      title={t('videoGeneration.agentSession.preview.open', { defaultValue: '放大预览' })}
    >
      {coverUrl ? (
        <img className={styles.filmPreviewImg} src={coverUrl} alt={cover?.label ?? ''} />
      ) : videoUrl ? (
        <video className={styles.filmPreviewVideo} src={videoUrl} muted playsInline preload='metadata' />
      ) : (
        <span className={styles.mediaPending} />
      )}
      <span className={styles.filmPreviewScrim}>
        <PlayOne theme='filled' size={22} fill='currentColor' />
      </span>
    </button>
  );
};

const MediaThumb: React.FC<{
  sessionId: string;
  item: StudioSessionMedia;
  gallery: StudioSessionMedia[];
  onOpen?: (item: StudioSessionMedia, gallery: StudioSessionMedia[]) => void;
}> = ({ sessionId, item, gallery, onOpen }) => {
  const { t } = useTranslation();
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    if (item.kind === 'file') {
      setUrl(null);
      return;
    }
    let cancelled = false;
    let blobUrl: string | null = null;
    void loadStudioMediaPreviewUrl(sessionId, item)
      .then((next) => {
        if (cancelled) {
          if (item.origin === 'cameo') URL.revokeObjectURL(next);
          return;
        }
        if (item.origin === 'cameo') blobUrl = next;
        setUrl(next);
      })
      .catch(() => {
        if (!cancelled) setUrl(null);
      });
    return () => {
      cancelled = true;
      if (blobUrl) URL.revokeObjectURL(blobUrl);
    };
  }, [sessionId, item.path, item.kind, item.origin]);

  if (item.kind === 'file') {
    return (
      <span className={styles.fileChip} title={item.label ?? item.path}>
        {item.label || item.path}
      </span>
    );
  }

  const previewable = gallery.filter((card) => card.kind !== 'file');

  return (
    <button
      type='button'
      className={styles.mediaCard}
      onClick={() => onOpen?.(item, previewable)}
      title={item.label || t('videoGeneration.agentSession.preview.open', { defaultValue: '放大预览' })}
    >
      {url && item.kind === 'video' ? (
        <video className={styles.mediaCardVideo} src={url} muted playsInline preload='metadata' />
      ) : url ? (
        <img className={styles.mediaCardImg} src={url} alt={item.label ?? ''} />
      ) : (
        <span className={styles.mediaPending} />
      )}
      {item.kind === 'video' ? (
        <span className={styles.mediaPlay} aria-hidden>
          <PlayOne theme='filled' size={14} fill='currentColor' />
        </span>
      ) : null}
    </button>
  );
};

const ISSUE_KIND_KEY: Record<FailureKind, string> = {
  credits: 'videoGeneration.agentSession.issue.kind.credits',
  llm: 'videoGeneration.agentSession.issue.kind.llm',
  image: 'videoGeneration.agentSession.issue.kind.image',
  video: 'videoGeneration.agentSession.issue.kind.video',
  moderation: 'videoGeneration.agentSession.issue.kind.moderation',
  unknown: 'videoGeneration.agentSession.issue.kind.unknown',
};

const ISSUE_KIND_FALLBACK: Record<FailureKind, string> = {
  credits: '积分',
  llm: '规划模型',
  image: '图片模型',
  video: '视频模型',
  moderation: '内容审核',
  unknown: '任务',
};

interface StudioSessionMessageViewProps {
  sessionId: string;
  item: StudioSessionMessage;
  title: string;
  body?: string;
  meta?: string;
  detail?: string;
  issueKind?: FailureKind;
  onOpenMedia?: (item: StudioSessionMedia, gallery: StudioSessionMedia[]) => void;
}

const StudioSessionMessageView: React.FC<StudioSessionMessageViewProps> = ({
  sessionId,
  item,
  title,
  body,
  meta,
  detail,
  issueKind,
  onOpenMedia,
}) => {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const isUser = item.role === 'user';
  const isGate = item.kind === 'gate_render' || item.kind === 'gate_action';
  const isIssue = item.kind === 'failure' || item.kind === 'cancelled';
  const longBrief = item.kind === 'user_brief' && (item.text?.length ?? 0) > BRIEF_LIMIT;
  const shownText =
    longBrief && !expanded ? `${item.text!.slice(0, BRIEF_LIMIT).trimEnd()}…` : item.text;
  const media = item.media ?? [];

  if (isIssue) {
    const kind = issueKind ?? 'unknown';
    const warn = item.kind === 'cancelled';
    return (
      <div className={styles.row}>
        <span className={`${styles.avatar} ${warn ? styles.avatarWarn : styles.avatarDanger}`} aria-hidden>
          <Attention theme='filled' size={13} fill='currentColor' />
        </span>
        <div className={`${styles.issueCard} ${warn ? styles.issueWarn : styles.issueDanger}`}>
          <div className={styles.issueKicker}>
            {warn
              ? t('videoGeneration.agentSession.issue.kind.cancelled', { defaultValue: '已停止' })
              : t(ISSUE_KIND_KEY[kind], { defaultValue: ISSUE_KIND_FALLBACK[kind] })}
          </div>
          {title ? <p className={styles.issueTitle}>{title}</p> : null}
          {body ? <p className={styles.issueBody}>{body}</p> : null}
          {detail ? (
            <details className={styles.issueDetail}>
              <summary>
                {t('videoGeneration.agentSession.issue.detail', { defaultValue: '问题详情' })}
              </summary>
              <pre className={styles.issuePre}>{detail}</pre>
            </details>
          ) : null}
        </div>
      </div>
    );
  }

  return (
    <div className={`${styles.row} ${isUser ? styles.rowUser : ''}`}>
      {!isUser ? (
        <span className={styles.avatar} aria-hidden>
          <Robot theme='outline' size={13} fill='currentColor' />
        </span>
      ) : null}
      <div
        className={[
          styles.bubble,
          isUser ? styles.bubbleUser : styles.bubbleAssistant,
          isGate ? styles.bubbleGate : '',
        ].join(' ')}
      >
        {title ? <p className={styles.bubbleTitle}>{title}</p> : null}
        {shownText ? <p className={styles.bubbleBody}>{shownText}</p> : null}
        {body && body !== title && body !== shownText ? (
          <p className={styles.bubbleBody}>{body}</p>
        ) : null}
        {longBrief ? (
          <button
            type='button'
            className={styles.bubbleMeta}
            onClick={() => setExpanded((v) => !v)}
          >
            {expanded
              ? t('videoGeneration.agentSession.briefCollapse', { defaultValue: '收起' })
              : t('videoGeneration.agentSession.briefExpand', { defaultValue: '展开全文' })}
          </button>
        ) : null}
        {item.kind === 'film_ready' && media.length > 0 ? (
          <FilmPreview sessionId={sessionId} items={media} onOpen={onOpenMedia} />
        ) : media.length > 0 ? (
          <>
            {media
              .filter((card) => card.kind === 'file')
              .map((card) => (
                <MediaThumb
                  key={card.id}
                  sessionId={sessionId}
                  item={card}
                  gallery={media}
                  onOpen={onOpenMedia}
                />
              ))}
            {media.some((card) => card.kind !== 'file') ? (
              <div className={styles.mediaGrid}>
                {media
                  .filter((card) => card.kind !== 'file')
                  .map((card) => (
                    <MediaThumb
                      key={card.id}
                      sessionId={sessionId}
                      item={card}
                      gallery={media}
                      onOpen={onOpenMedia}
                    />
                  ))}
              </div>
            ) : null}
          </>
        ) : null}
        {meta || item.live ? (
          <div className={styles.bubbleMeta}>
            {item.live ? <span className={styles.liveDot} /> : null}
            {meta}
          </div>
        ) : null}
      </div>
      {isUser ? (
        <span className={styles.avatar} aria-hidden>
          <User theme='outline' size={13} fill='currentColor' />
        </span>
      ) : null}
    </div>
  );
};

export default StudioSessionMessageView;
