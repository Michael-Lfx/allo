import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Broadcast, Platte, RobotOne, VideoOne } from '@icon-park/react';
import { CanvasMenuRow } from '@oc/components/canvas/canvas-overlay';
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
  descKey: string;
  descFallback: string;
}> = [
  {
    id: 'generate',
    icon: <VideoOne theme='outline' size={14} />,
    labelKey: 'videoGeneration.mode.generateLabel',
    labelFallback: '视频生成',
    descKey: 'videoGeneration.mode.generateMenuDesc',
    descFallback: '参考图 + 提示词，直接生成视频片段',
  },
  {
    id: 'agent',
    icon: <RobotOne theme='outline' size={14} />,
    labelKey: 'videoGeneration.mode.agentLabel',
    labelFallback: '短剧模式',
    descKey: 'videoGeneration.mode.agentMenuDesc',
    descFallback: '自动规划分镜并渲染成片',
  },
  {
    id: 'creation',
    icon: <Platte theme='outline' size={14} />,
    labelKey: 'videoGeneration.mode.creationLabel',
    labelFallback: '创作模式',
    descKey: 'videoGeneration.mode.creationMenuDesc',
    descFallback: '把需求发给画布 Agent，自动搭建并生成',
  },
  {
    id: 'briefing',
    icon: <Broadcast theme='outline' size={14} />,
    labelKey: 'videoGeneration.mode.briefingLabel',
    labelFallback: '资讯播报',
    descKey: 'videoGeneration.mode.briefingMenuDesc',
    descFallback: '可溯源调研 + 口播成片，不发明今日新闻',
  },
];

/** Mode-switch menu shown from the composer toolbar (视频生成 / 短剧 / 创作). */
export function ModeMenu({ mode, onSelect }: ModeMenuProps) {
  const { t } = useTranslation();

  return (
    <div role='listbox' aria-label={t('videoGeneration.create.modesMenuAria', { defaultValue: '选择 Mode' })}>
      {MODES.map((item) => (
        <CanvasMenuRow
          key={item.id}
          icon={item.icon}
          label={t(item.labelKey, { defaultValue: item.labelFallback })}
          detail={t(item.descKey, { defaultValue: item.descFallback })}
          active={mode === item.id}
          onClick={() => onSelect(item.id)}
        />
      ))}
    </div>
  );
}
