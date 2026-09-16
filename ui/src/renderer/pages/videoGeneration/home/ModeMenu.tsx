import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Broadcast, Platte, RobotOne, VideoOne } from '@icon-park/react';
import { CanvasMenuRow } from '@oc/components/canvas/canvas-overlay';
import styles from './home.module.css';
import type { VideoHomeMode } from './types';

export interface ModeMenuProps {
  mode: VideoHomeMode;
  onSelect: (mode: VideoHomeMode) => void;
}

const MODES: Array<{
  id: Exclude<VideoHomeMode, 'action'>;
  icon: ReactNode;
  labelKey: string;
  labelFallback: string;
}> = [
  {
    id: 'generate',
    icon: <VideoOne theme='outline' size={14} />,
    labelKey: 'videoGeneration.mode.generateLabel',
    labelFallback: '即刻出片',
  },
  {
    id: 'agent',
    icon: <RobotOne theme='outline' size={14} />,
    labelKey: 'videoGeneration.mode.agentLabel',
    labelFallback: '短剧工坊',
  },
  {
    id: 'creation',
    icon: <Platte theme='outline' size={14} />,
    labelKey: 'videoGeneration.mode.creationLabel',
    labelFallback: '无限画布',
  },
  {
    id: 'briefing',
    icon: <Broadcast theme='outline' size={14} />,
    labelKey: 'videoGeneration.mode.briefingLabel',
    labelFallback: '资讯口播',
  },
];

/** Mode-switch menu shown from the composer toolbar. */
export function ModeMenu({ mode, onSelect }: ModeMenuProps) {
  const { t } = useTranslation();

  return (
    <div
      role='listbox'
      className={styles.modeMenu}
      aria-label={t('videoGeneration.create.modesMenuAria', { defaultValue: '选择 Mode' })}
    >
      {MODES.map((item) => (
        <CanvasMenuRow
          key={item.id}
          icon={item.icon}
          label={t(item.labelKey, { defaultValue: item.labelFallback })}
          active={mode === item.id}
          onClick={() => onSelect(item.id)}
        />
      ))}
    </div>
  );
}
