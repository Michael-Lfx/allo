import type { TFunction } from 'i18next';
import { FEATURED_LOOK_IDS } from '../styleCatalog/looks';
import type { AgentModeDefinition } from './types';

/** Featured look ids kept for draft migration (`creationSkillId`). */
export const CREATION_SKILL_IDS = FEATURED_LOOK_IDS;

/** Agent Mode catalog (idea / script / novel), localized per render. */
export function agentModesFor(t: TFunction): AgentModeDefinition[] {
  return [
    {
      id: 'idea2video',
      label: t('videoGeneration.create.modes.idea', { defaultValue: '一个想法' }),
      description: t('videoGeneration.create.modes.ideaDesc', {
        defaultValue: '从一句灵感扩写成完整影片',
      }),
    },
    {
      id: 'script2video',
      label: t('videoGeneration.create.modes.script', { defaultValue: '完整剧本' }),
      description: t('videoGeneration.create.modes.scriptDesc', {
        defaultValue: '按剧情结构自动拆解镜头',
      }),
    },
    {
      id: 'novel2video',
      label: t('videoGeneration.create.modes.novel', { defaultValue: '小说文本' }),
      description: t('videoGeneration.create.modes.novelDesc', {
        defaultValue: '提炼长文情节并设计分镜',
      }),
    },
  ];
}
