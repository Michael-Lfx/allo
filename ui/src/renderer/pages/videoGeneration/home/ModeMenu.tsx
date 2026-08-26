import { useTranslation } from 'react-i18next';
import { Platte, RobotOne, VideoOne } from '@icon-park/react';
import type { VideoHomeMode } from './types';
import styles from './home.module.css';

export interface ModeMenuProps {
  mode: VideoHomeMode;
  onSelect: (mode: VideoHomeMode) => void;
}

/** Mode-switch menu shown from the composer toolbar (视频生成 / 短剧 / 创作). */
export function ModeMenu({ mode, onSelect }: ModeMenuProps) {
  const { t } = useTranslation();
  const generateModeLabel = t('videoGeneration.mode.generateLabel', {
    defaultValue: '视频生成',
  });
  const agentModeLabel = t('videoGeneration.mode.agentLabel', {
    defaultValue: '短剧模式',
  });
  const creationModeLabel = t('videoGeneration.mode.creationLabel', {
    defaultValue: '创作模式',
  });

  return (
    <div className={styles.modeMenu}>
      <button
        type='button'
        className={`${styles.modeMenuItem} ${
          mode === 'generate' ? styles.modeMenuItemActive : ''
        }`}
        onClick={() => onSelect('generate')}
      >
        <VideoOne theme='outline' size={18} />
        <span>
          <strong>{generateModeLabel}</strong>
          <small>
            {t('videoGeneration.mode.generateMenuDesc', {
              defaultValue: '参考图 + 提示词，直接生成视频片段',
            })}
          </small>
        </span>
      </button>
      <button
        type='button'
        className={`${styles.modeMenuItem} ${
          mode === 'agent' ? styles.modeMenuItemActive : ''
        }`}
        onClick={() => onSelect('agent')}
      >
        <RobotOne theme='outline' size={18} />
        <span>
          <strong>{agentModeLabel}</strong>
          <small>
            {t('videoGeneration.mode.agentMenuDesc', {
              defaultValue: '自动规划分镜并渲染成片',
            })}
          </small>
        </span>
      </button>
      {/* TODO: Re-enable action mode when feature is ready
      <button
        type='button'
        className={`${styles.modeMenuItem} ${
          mode === 'action' ? styles.modeMenuItemActive : ''
        }`}
        onClick={() => onSelect('action')}
      >
        <People theme='outline' size={18} />
        <span>
          <strong>{actionModeLabel}</strong>
          <small>
            {t('videoGeneration.mode.actionMenuDesc', {
              defaultValue: '角色图 + 参考视频，模仿动作成片',
            })}
          </small>
        </span>
      </button>
      */}
      <button
        type='button'
        className={`${styles.modeMenuItem} ${
          mode === 'creation' ? styles.modeMenuItemActive : ''
        }`}
        onClick={() => onSelect('creation')}
      >
        <Platte theme='outline' size={18} />
        <span>
          <strong>{creationModeLabel}</strong>
          <small>
            {t('videoGeneration.mode.creationMenuDesc', {
              defaultValue: '进入无限画布自由编排',
            })}
          </small>
        </span>
      </button>
    </div>
  );
}
