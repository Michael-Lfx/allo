import React from 'react';
import { useTranslation } from 'react-i18next';
import type { StudioComposerAction } from './types';
import styles from './index.module.css';

interface StudioSessionComposerProps {
  action: StudioComposerAction;
  onSend: () => void;
  onStop: () => void;
  sending?: boolean;
  stopping?: boolean;
  assetsBlocked?: boolean;
  busyKind?: 'planning' | 'rendering' | null;
  reviewShotNumber?: number | null;
}

const StudioSessionComposer: React.FC<StudioSessionComposerProps> = ({
  action,
  onSend,
  onStop,
  sending,
  stopping,
  assetsBlocked,
  busyKind,
  reviewShotNumber,
}) => {
  const { t } = useTranslation();
  const busy = action === 'stop';
  const reviewing = action === 'approve_shot';
  const showSpinner = Boolean(sending || stopping || busy);
  const disabled = Boolean(assetsBlocked || stopping);

  const stopHint = t('videoGeneration.agentSession.action.stopHint', { defaultValue: '点击可终止' });
  const label =
    action === 'plan'
      ? t('videoGeneration.agentSession.send.plan', { defaultValue: '开始规划' })
      : action === 'continue'
        ? t('videoGeneration.agentSession.send.continue', { defaultValue: '继续' })
        : action === 'approve_shot'
          ? t('videoGeneration.agentSession.send.approve', { defaultValue: '确认生成本镜' })
          : action === 'render' || action === 'none'
            ? t('videoGeneration.agentSession.send.render', { defaultValue: '生成成片' })
            : busyKind === 'planning'
              ? t('videoGeneration.agentSession.action.planning', { defaultValue: '正在规划…' })
              : busyKind === 'rendering'
                ? t('videoGeneration.agentSession.action.rendering', { defaultValue: '正在生成成片…' })
                : t('videoGeneration.agentSession.action.working', { defaultValue: '正在生成…' });

  if (action === 'none' && !assetsBlocked) {
    return (
      <div className={styles.composer}>
        <p className={styles.composerDone}>
          {t('videoGeneration.agentSession.action.done', { defaultValue: '成片已在左侧准备好' })}
        </p>
      </div>
    );
  }

  const hint = assetsBlocked
    ? t('videoGeneration.agentSession.hint.needAssets', {
        defaultValue: '请先在左侧上传角色图和参考视频',
      })
    : reviewing
      ? t('videoGeneration.agentSession.hint.reviewWaiting', {
          number: reviewShotNumber ?? '—',
          defaultValue: '第 {{number}} 镜等待确认，也可在左侧过审条操作',
        })
      : busy
        ? stopHint
        : null;

  return (
    <div className={styles.composer}>
      {hint ? <p className={styles.composerHint}>{hint}</p> : null}
      <button
        type='button'
        className={`${styles.sendBar} ${busy ? styles.sendBusy : styles.sendIdle}`}
        disabled={disabled}
        aria-busy={showSpinner}
        aria-label={busy ? `${label}，${stopHint}` : label}
        title={busy ? `${label}，${stopHint}` : label}
        data-testid={busy ? 'studio-session-stop' : reviewing ? 'studio-session-approve' : 'studio-session-send'}
        onClick={() => {
          if (disabled) return;
          if (busy) onStop();
          else onSend();
        }}
      >
        {showSpinner ? <span className={styles.actionSpinner} aria-hidden /> : null}
        <span className={styles.sendBarLabel}>{label}</span>
      </button>
    </div>
  );
};

export default StudioSessionComposer;
