import type { TFunction } from 'i18next';
import { DEFAULT_VISUAL_STYLE_PROMPT, VISUAL_STYLE_PRESETS } from '../visualStylePresets';
import type {
  AgentModeDefinition,
  CreationSkillDefinition,
  CreationSkillId,
} from './types';

export const CREATION_SKILL_IDS: readonly CreationSkillId[] = [
  'cinematic',
  'anime',
  'cyberpunk',
  'inkWash',
];

export const CREATION_SKILL_PROMPTS: Record<CreationSkillId, string> = {
  cinematic:
    VISUAL_STYLE_PRESETS.find((preset) => preset.key === 'cinematic')?.prompt ??
    DEFAULT_VISUAL_STYLE_PROMPT,
  anime:
    VISUAL_STYLE_PRESETS.find((preset) => preset.key === 'anime')?.prompt ??
    DEFAULT_VISUAL_STYLE_PROMPT,
  cyberpunk:
    VISUAL_STYLE_PRESETS.find((preset) => preset.key === 'cyberpunk')?.prompt ??
    DEFAULT_VISUAL_STYLE_PROMPT,
  inkWash:
    VISUAL_STYLE_PRESETS.find((preset) => preset.key === 'inkWash')?.prompt ??
    DEFAULT_VISUAL_STYLE_PROMPT,
};

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

/** Creation style-skill catalog, localized per render. */
export function creationSkillsFor(t: TFunction): CreationSkillDefinition[] {
  return [
    {
      id: 'cinematic',
      label: t('videoGeneration.create.skills.cinematic.label', {
        defaultValue: '电影写实',
      }),
      description: t('videoGeneration.create.skills.cinematic.desc', {
        defaultValue: '纪实光影 · 叙事镜头',
      }),
      stylePrompt: CREATION_SKILL_PROMPTS.cinematic,
    },
    {
      id: 'anime',
      label: t('videoGeneration.create.skills.anime.label', {
        defaultValue: '二次元',
      }),
      description: t('videoGeneration.create.skills.anime.desc', {
        defaultValue: '鲜明线稿 · 动漫质感',
      }),
      stylePrompt: CREATION_SKILL_PROMPTS.anime,
    },
    {
      id: 'cyberpunk',
      label: t('videoGeneration.create.skills.cyberpunk.label', {
        defaultValue: '赛博霓虹',
      }),
      description: t('videoGeneration.create.skills.cyberpunk.desc', {
        defaultValue: '未来都市 · 高对比',
      }),
      stylePrompt: CREATION_SKILL_PROMPTS.cyberpunk,
    },
    {
      id: 'inkWash',
      label: t('videoGeneration.create.skills.inkWash.label', {
        defaultValue: '水墨意境',
      }),
      description: t('videoGeneration.create.skills.inkWash.desc', {
        defaultValue: '留白构图 · 东方美学',
      }),
      stylePrompt: CREATION_SKILL_PROMPTS.inkWash,
    },
  ];
}
