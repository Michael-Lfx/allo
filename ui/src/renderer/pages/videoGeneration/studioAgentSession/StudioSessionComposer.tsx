import React from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowUp } from 'lucide-react';
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
}

const StudioSessionComposer: React.FC<StudioSessionComposerProps> = ({
  action,
  onSend,
  onStop,
  sending,
  stopping,
  assetsBlocked,
  busyKind,
}) => {
  const { t } = useTranslation();
  const busy = action === 'stop';
  const showSpinner = Boolean(sending || stopping || busy);
  const disabled = Boolean(assetsBlocked || stopping);

  const stopHint = t('videoGeneration.agentSession.action.stopHint', { defaultValue: '点击可终止' });
  const label =
    action === 'plan'
      ? t('videoGeneration.agentSession.send.plan', { defaultValue: '开始规划' })
      : action === 'continue'
        ? t('videoGeneration.agentSession.send.continue', { defaultValue: '从断点继续' })
        : action === 'render'
          ? t('videoGeneration.agentSession.send.render', { defaultValue: '确认并生成成片' })
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

  return (
    <div className={styles.composer}>
      {assetsBlocked ? (
        <p className={styles.composerHint}>
          {t('videoGeneration.agentSession.hint.needAssets', {
            defaultValue: '请先在左侧上传角色图和参考视频',
          })}
        </p>
      ) : busy ? (
        <p className={styles.composerHint}>{stopHint}</p>
      ) : (
        <p className={styles.composerHint}>{label}</p>
      )}
      <button
        type='button'
        className={`canvas-send-token ${busy ? styles.sendBusy : styles.sendIdle}`}
        disabled={disabled}
        aria-busy={showSpinner}
        aria-label={busy ? `${label}，${stopHint}` : label}
        title={busy ? `${label}，${stopHint}` : label}
        data-testid={busy ? 'studio-session-stop' : 'studio-session-send'}
        onClick={() => {
          if (disabled) return;
          if (busy) onStop();
          else onSend();
        }}
      >
        {showSpinner ? <span className={styles.actionSpinner} aria-hidden /> : <ArrowUp className={styles.sendIcon} />}
      </button>
    </div>
  );
};

export default StudioSessionComposer;
