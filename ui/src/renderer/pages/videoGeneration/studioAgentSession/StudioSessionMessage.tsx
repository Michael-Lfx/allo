import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Attention, PlayOne, Robot, User } from '@icon-park/react';
import { getArtifact } from '../api';
import { seekMediaElementToFirstFrame } from '../mediaFirstFrame';
import { useArtifactMediaUrl } from '../useArtifactMediaUrl';
import {
  groupPortraitMedia,
  loadStudioMediaPreviewUrl,
  parseCastEntries,
  parseScriptScenes,
} from './collectStudioMedia';
import type { FailureKind } from '../classifyFailure';
import type { StudioSessionMedia, StudioSessionMessage } from './types';
import styles from './index.module.css';

const BRIEF_LIMIT = 280;
const DOCUMENT_LIMIT = 420;

function isPortraitPath(path: string): boolean {
  return /character_portraits/i.test(path.replace(/\\/g, '/'));
}

function partitionSessionMedia(media: StudioSessionMedia[]): {
  files: StudioSessionMedia[];
  docs: StudioSessionMedia[];
  portraits: StudioSessionMedia[];
  rest: StudioSessionMedia[];
} {
  const files: StudioSessionMedia[] = [];
  const docs: StudioSessionMedia[] = [];
  const portraits: StudioSessionMedia[] = [];
  const rest: StudioSessionMedia[] = [];
  for (const item of media) {
    if (item.kind === 'file') files.push(item);
    else if (item.kind === 'document') docs.push(item);
    else if (item.kind === 'audio' || (item.kind === 'image' && isPortraitPath(item.path))) {
      portraits.push(item);
    } else rest.push(item);
  }
  return { files, docs, portraits, rest };
}

const FilmPreview: React.FC<{
  sessionId: string;
  items: StudioSessionMedia[];
  onOpen?: (item: StudioSessionMedia, gallery: StudioSessionMedia[]) => void;
}> = ({ sessionId, items, onOpen }) => {
  const { t } = useTranslation();
  const video = items.find((item) => item.kind === 'video');
  const cover = items.find((item) => item.kind === 'image');
  const { url: videoUrl, reload: reloadVideo } = useArtifactMediaUrl(sessionId, video?.path);
  const { url: coverUrl, reload: reloadCover } = useArtifactMediaUrl(sessionId, cover?.path);

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
        <img
          className={styles.filmPreviewImg}
          src={coverUrl}
          alt={cover?.label ?? ''}
          onError={() => reloadCover()}
        />
      ) : videoUrl ? (
        <video
          className={styles.filmPreviewVideo}
          src={videoUrl}
          muted
          playsInline
          preload='metadata'
          onError={() => reloadVideo()}
          onLoadedMetadata={(event) => seekMediaElementToFirstFrame(event.currentTarget)}
        />
      ) : (
        <span className={styles.mediaPending} />
      )}
      <span className={styles.filmPreviewScrim}>
        <PlayOne theme='filled' size={22} fill='currentColor' />
      </span>
    </button>
  );
};

const AudioClip: React.FC<{ sessionId: string; item: StudioSessionMedia }> = ({
  sessionId,
  item,
}) => {
  const { t } = useTranslation();
  const { url, reload } = useArtifactMediaUrl(sessionId, item.path);

  const label = t('videoGeneration.agentSession.document.voiceLabel', {
    defaultValue: '参考音频',
  });

  return (
    <div className={styles.audioClip}>
      <span className={styles.audioClipLabel}>{label}</span>
      {url ? (
        <audio className={styles.audioPlayer} src={url} controls preload='metadata' onError={() => reload()} />
      ) : (
        <span className={styles.mediaPending} />
      )}
    </div>
  );
};

const DocumentExcerpt: React.FC<{
  sessionId: string;
  item: StudioSessionMedia;
  onSelect?: (item: StudioSessionMedia) => void;
}> = ({ sessionId, item, onSelect }) => {
  const { t } = useTranslation();
  const [text, setText] = useState<string | null>(null);
  const [scriptScenes, setScriptScenes] = useState<string[]>([]);
  const [expanded, setExpanded] = useState(false);
  const role = item.role ?? 'story';
  const title = t(`videoGeneration.agentSession.document.${role}`, {
    defaultValue: item.label ?? role,
  });
  const filesKey = (item.paths && item.paths.length > 0 ? item.paths : [item.path]).join('\0');

  useEffect(() => {
    let cancelled = false;
    const files = filesKey.split('\0').filter(Boolean);
    setText(null);
    setScriptScenes([]);
    setExpanded(false);
    void Promise.all(files.map((path) => getArtifact(sessionId, path)))
      .then((contents) => {
        if (cancelled) return;
        const bodies = contents.map((content) => content.text ?? '');
        setText(bodies.join('\n\n').trim());
        setScriptScenes(role === 'script' ? bodies.flatMap((body) => parseScriptScenes(body)) : []);
      })
      .catch(() => {
        if (cancelled) return;
        setText('');
        setScriptScenes([]);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, filesKey, role]);

  const cast = role === 'cast' && text ? parseCastEntries(text) : [];
  const scenes = role === 'script' ? scriptScenes : [];
  const sceneLong = scenes.some((scene) => scene.length > DOCUMENT_LIMIT);
  const body = text ?? '';
  const long = scenes.length > 0 ? sceneLong : body.length > DOCUMENT_LIMIT;
  const shown = long && !expanded ? `${body.slice(0, DOCUMENT_LIMIT).trimEnd()}…` : body;

  return (
    <div className={styles.documentCard}>
      <button
        type='button'
        className={styles.documentTitle}
        onClick={() => onSelect?.(item)}
        title={item.path}
      >
        {title}
      </button>
      {text == null ? (
        <span className={styles.mediaPending} />
      ) : cast.length > 0 ? (
        <ul className={styles.castList}>
          {cast.map((entry) => (
            <li key={entry.name}>
              <span className={styles.castName}>{entry.name}</span>
              {entry.features ? (
                <span className={styles.castFeatures}>{entry.features}</span>
              ) : null}
            </li>
          ))}
        </ul>
      ) : scenes.length > 0 ? (
        <div className={styles.scriptScenes}>
          {scenes.map((scene, index) => {
            const clipped = sceneLong && !expanded && scene.length > DOCUMENT_LIMIT;
            return (
              <section key={`${index}:${scene.slice(0, 24)}`} className={styles.scriptScene}>
                {scenes.length > 1 ? (
                  <h4 className={styles.scriptSceneHeading}>
                    {t('videoGeneration.agentSession.document.scriptScene', {
                      n: index + 1,
                      defaultValue: `第 ${index + 1} 场`,
                    })}
                  </h4>
                ) : null}
                <p className={styles.documentBody}>
                  {clipped ? `${scene.slice(0, DOCUMENT_LIMIT).trimEnd()}…` : scene}
                </p>
              </section>
            );
          })}
        </div>
      ) : shown ? (
        <p className={styles.documentBody}>{shown}</p>
      ) : null}
      {cast.length === 0 && long ? (
        <button type='button' className={styles.bubbleMeta} onClick={() => setExpanded((v) => !v)}>
          {expanded
            ? t('videoGeneration.agentSession.briefCollapse', { defaultValue: '收起' })
            : t('videoGeneration.agentSession.briefExpand', { defaultValue: '展开全文' })}
        </button>
      ) : null}
    </div>
  );
};

const MediaThumb: React.FC<{
  sessionId: string;
  item: StudioSessionMedia;
  gallery: StudioSessionMedia[];
  onOpen?: (item: StudioSessionMedia, gallery: StudioSessionMedia[]) => void;
}> = ({ sessionId, item, gallery, onOpen }) => {
  const { t } = useTranslation();
  const artifactPath =
    item.origin === 'cameo' || item.kind === 'file' || item.kind === 'document' || item.kind === 'audio'
      ? null
      : item.path;
  const { url: artifactUrl, reload } = useArtifactMediaUrl(sessionId, artifactPath);
  const [cameoUrl, setCameoUrl] = useState<string | null>(null);

  useEffect(() => {
    if (item.origin !== 'cameo' || item.kind === 'file' || item.kind === 'document' || item.kind === 'audio') {
      setCameoUrl(null);
      return;
    }
    let cancelled = false;
    let blobUrl: string | null = null;
    void loadStudioMediaPreviewUrl(sessionId, item)
      .then((next) => {
        if (cancelled) {
          URL.revokeObjectURL(next);
          return;
        }
        blobUrl = next;
        setCameoUrl(next);
      })
      .catch(() => {
        if (!cancelled) setCameoUrl(null);
      });
    return () => {
      cancelled = true;
      if (blobUrl) URL.revokeObjectURL(blobUrl);
    };
  }, [sessionId, item.path, item.kind, item.origin]);

  const url = item.origin === 'cameo' ? cameoUrl : artifactUrl;

  if (item.kind === 'file') {
    return (
      <span className={styles.fileChip} title={item.label ?? item.path}>
        {item.label || item.path}
      </span>
    );
  }

  const previewable = gallery.filter(
    (card) => card.kind === 'image' || card.kind === 'video'
  );

  return (
    <button
      type='button'
      className={styles.mediaCard}
      onClick={() => onOpen?.(item, previewable)}
      title={item.label || t('videoGeneration.agentSession.preview.open', { defaultValue: '放大预览' })}
    >
      {url && item.kind === 'video' ? (
        <video
          className={styles.mediaCardVideo}
          src={url}
          muted
          playsInline
          preload='metadata'
          onError={() => reload()}
          onLoadedMetadata={(event) => seekMediaElementToFirstFrame(event.currentTarget)}
        />
      ) : url ? (
        <img className={styles.mediaCardImg} src={url} alt={item.label ?? ''} onError={() => reload()} />
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

const SessionMedia: React.FC<{
  sessionId: string;
  media: StudioSessionMedia[];
  onOpenMedia?: (item: StudioSessionMedia, gallery: StudioSessionMedia[]) => void;
}> = ({ sessionId, media, onOpenMedia }) => {
  const { files, docs, portraits, rest } = partitionSessionMedia(media);
  const portraitGroups = groupPortraitMedia(portraits);
  return (
    <>
      {files.map((card) => (
        <MediaThumb
          key={card.id}
          sessionId={sessionId}
          item={card}
          gallery={media}
          onOpen={onOpenMedia}
        />
      ))}
      {docs.map((card) => (
        <DocumentExcerpt
          key={card.id}
          sessionId={sessionId}
          item={card}
          onSelect={(item) => onOpenMedia?.(item, [item])}
        />
      ))}
      {portraitGroups.map((group) => (
        <div key={group.dir} className={styles.characterPack}>
          {group.label ? <div className={styles.characterPackLabel}>{group.label}</div> : null}
          {group.images.length > 0 ? (
            <div className={styles.mediaGrid}>
              {group.images.map((card) => (
                <MediaThumb
                  key={card.id}
                  sessionId={sessionId}
                  item={card}
                  gallery={group.images}
                  onOpen={onOpenMedia}
                />
              ))}
            </div>
          ) : null}
          {group.audios.map((card) => (
            <AudioClip key={card.id} sessionId={sessionId} item={card} />
          ))}
        </div>
      ))}
      {rest.length > 0 ? (
        <div className={styles.mediaGrid}>
          {rest.map((card) => (
            <MediaThumb
              key={card.id}
              sessionId={sessionId}
              item={card}
              gallery={rest}
              onOpen={onOpenMedia}
            />
          ))}
        </div>
      ) : null}
    </>
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
          <SessionMedia
            sessionId={sessionId}
            media={media}
            onOpenMedia={onOpenMedia}
          />
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
