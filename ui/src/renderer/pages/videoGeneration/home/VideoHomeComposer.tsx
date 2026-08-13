import React, { Suspense, lazy, useEffect, useMemo, useRef, useState } from 'react';
import { Input, Popover } from '@arco-design/web-react';
import {
  BookOpen,
  CloseSmall,
  Down,
  FileText,
  MagicWand,
  Pic,
  Platte,
  RobotOne,
  SettingTwo,
  Star,
  VideoOne,
} from '@icon-park/react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import { DEFAULT_VISUAL_STYLE_PROMPT, VISUAL_STYLE_PRESETS } from '../visualStylePresets';
import {
  DEFAULT_SEEDANCE_ASPECT_RATIO,
  normalizeSeedanceAspectRatio,
} from '../aspectRatios';
import {
  DEFAULT_VIDEO_FPS,
  DEFAULT_VIDEO_RESOLUTION,
  normalizeVideoFps,
  normalizeVideoResolution,
} from '../videoModelCapabilities';
import { suggestCameoCharacterName } from '../cameoUtils';
import type { CameoDraftItem, VerticalSkillSummary, VimaxWorkflow } from '../types';
import { BoldSendArrowIcon, SlantedDocIcon } from './ComposerIcons';
import {
  displayFileStem,
  isSupportedImageFile,
  isSupportedTextFile,
  readUploadedTextFile,
  VIDEO_HOME_UPLOAD_ACCEPT,
} from './documentUpload';
import type {
  AgentModeDefinition,
  CanvasReferenceDraft,
  CreationSkillDefinition,
  CreationSkillId,
  GenerationPreferences,
  VideoCreateDraft,
  VideoHomeMode,
} from './types';
import { listVerticalSkills } from '../api';
import { generationPreferencesSummary } from '../preferenceSummary';
import styles from './home.module.css';

const CameoCastEditor = lazy(() => import('../components/CameoCastEditor'));
const GenerationPreferencesPopover = lazy(() => import('./GenerationPreferencesPopover'));
const VerticalSkillMenu = lazy(() => import('./VerticalSkillMenu'));
const VerticalSkillCreateModal = lazy(() => import('./VerticalSkillCreateModal'));

const TextArea = Input.TextArea;
const DRAFT_KEY = 'flowy.videoGeneration.homeDraft.v3';
const LEGACY_DRAFT_KEY = 'flowy.videoGeneration.homeDraft.v2';
const LEGACY_DRAFT_KEY_V1 = 'flowy.videoGeneration.draft.v1';
const MAX_REFERENCES = 8;

const EMPTY_MODELS = {
  llm_model: '',
  image_model: '',
  video_model: '',
};

const DEFAULT_PREFERENCES: GenerationPreferences = {
  automatic: false,
  smartAspect: true,
  mediaKind: 'video',
  aspectRatio: DEFAULT_SEEDANCE_ASPECT_RATIO,
  resolution: DEFAULT_VIDEO_RESOLUTION,
  fps: DEFAULT_VIDEO_FPS,
  targetDurationSecs: 30,
  models: EMPTY_MODELS,
};

const CREATION_SKILL_IDS: readonly CreationSkillId[] = [
  'cinematic',
  'anime',
  'cyberpunk',
  'inkWash',
];

const CREATION_SKILL_PROMPTS: Record<CreationSkillId, string> = {
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

function makeLocalId(prefix: string): string {
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

function defaultDraft(): VideoCreateDraft {
  return {
    workflow: 'idea2video',
    sourceText: '',
    creationPrompt: '',
    creationSkillId: 'cinematic',
    requirement: '',
    style: DEFAULT_VISUAL_STYLE_PROMPT,
    verticalSkillIds: [],
    preferences: DEFAULT_PREFERENCES,
    cameos: [],
    canvasReferences: [],
  };
}

function loadDraft(): VideoCreateDraft {
  const fallback = defaultDraft();
  try {
    const raw =
      window.sessionStorage.getItem(DRAFT_KEY) ??
      window.sessionStorage.getItem(LEGACY_DRAFT_KEY) ??
      window.sessionStorage.getItem(LEGACY_DRAFT_KEY_V1);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const legacyModels = (parsed.models ?? {}) as Partial<GenerationPreferences['models']>;
    const parsedPreferences = (parsed.preferences ?? {}) as Partial<GenerationPreferences>;
    const models = {
      llm_model: parsedPreferences.models?.llm_model ?? legacyModels.llm_model ?? '',
      image_model: parsedPreferences.models?.image_model ?? legacyModels.image_model ?? '',
      video_model: parsedPreferences.models?.video_model ?? legacyModels.video_model ?? '',
    };
    const workflow: VimaxWorkflow =
      parsed.workflow === 'script2video' || parsed.workflow === 'novel2video'
        ? parsed.workflow
        : 'idea2video';
    const creationSkillId = CREATION_SKILL_IDS.includes(
      parsed.creationSkillId as CreationSkillId
    )
      ? (parsed.creationSkillId as CreationSkillId)
      : 'cinematic';
    const verticalSkillIds = Array.isArray(parsed.verticalSkillIds)
      ? parsed.verticalSkillIds.filter((id): id is string => typeof id === 'string')
      : [];
    return {
      ...fallback,
      workflow,
      sourceText: typeof parsed.sourceText === 'string' ? parsed.sourceText : '',
      creationPrompt:
        typeof parsed.creationPrompt === 'string' ? parsed.creationPrompt : '',
      creationSkillId,
      requirement: typeof parsed.requirement === 'string' ? parsed.requirement : '',
      style:
        typeof parsed.style === 'string' && parsed.style.trim()
          ? parsed.style
          : DEFAULT_VISUAL_STYLE_PROMPT,
      verticalSkillIds,
      preferences: {
        automatic: parsedPreferences.automatic === true,
        smartAspect: parsedPreferences.smartAspect !== false,
        mediaKind: parsedPreferences.mediaKind === 'image' ? 'image' : 'video',
        aspectRatio: normalizeSeedanceAspectRatio(
          String(parsedPreferences.aspectRatio ?? parsed.aspectRatio ?? '')
        ),
        resolution: normalizeVideoResolution(
          models.video_model,
          String(
            parsedPreferences.resolution ??
              parsed.resolution ??
              DEFAULT_VIDEO_RESOLUTION
          )
        ),
        fps: normalizeVideoFps(
          models.video_model,
          Number(parsedPreferences.fps ?? parsed.fps ?? DEFAULT_VIDEO_FPS)
        ),
        targetDurationSecs:
          typeof parsedPreferences.targetDurationSecs === 'number'
            ? parsedPreferences.targetDurationSecs
            : typeof parsed.targetDurationSecs === 'number'
              ? parsed.targetDurationSecs
              : 30,
        models,
      },
      // Files intentionally cannot survive reloads.
      cameos: [],
      canvasReferences: [],
    };
  } catch {
    return fallback;
  }
}

export function clearVideoHomeDraft(): void {
  try {
    window.sessionStorage.removeItem(DRAFT_KEY);
    window.sessionStorage.removeItem(LEGACY_DRAFT_KEY);
    window.sessionStorage.removeItem(LEGACY_DRAFT_KEY_V1);
  } catch {
    // Storage may be unavailable in hardened webviews.
  }
}

interface VideoHomeComposerProps {
  mode: VideoHomeMode;
  loading?: boolean;
  onModeChange: (mode: VideoHomeMode) => void;
  onSubmitAgent: (draft: VideoCreateDraft) => void;
  onSubmitCreation: (draft: VideoCreateDraft) => void;
}

const VideoHomeComposer: React.FC<VideoHomeComposerProps> = ({
  mode,
  loading,
  onModeChange,
  onSubmitAgent,
  onSubmitCreation,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const draftedTracked = useRef(false);
  const [draft, setDraft] = useState<VideoCreateDraft>(loadDraft);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [prefsModuleReady, setPrefsModuleReady] = useState(false);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const [slashMenuOpen, setSlashMenuOpen] = useState(false);
  const [skillHubOpen, setSkillHubOpen] = useState(false);
  const [skillCreateOpen, setSkillCreateOpen] = useState(false);
  const [skillListReloadToken, setSkillListReloadToken] = useState(0);
  const [skillCatalog, setSkillCatalog] = useState<VerticalSkillSummary[]>([]);
  const [modelMissing, setModelMissing] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [documentName, setDocumentName] = useState<string | null>(null);
  const draftRef = useRef(draft);
  const composerRef = useRef<HTMLDivElement>(null);

  const agentModes = useMemo<AgentModeDefinition[]>(
    () => [
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
    ],
    [t]
  );

  const creationSkills = useMemo<CreationSkillDefinition[]>(
    () => [
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
    ],
    [t]
  );

  const agentModeLabel = t('videoGeneration.mode.agentLabel', {
    defaultValue: 'Agent 模式',
  });
  const creationModeLabel = t('videoGeneration.mode.creationLabel', {
    defaultValue: '创作模式',
  });
  const modeLabel = mode === 'agent' ? agentModeLabel : creationModeLabel;

  useEffect(() => {
    draftRef.current = draft;
    try {
      window.sessionStorage.setItem(
        DRAFT_KEY,
        JSON.stringify({
          ...draft,
          cameos: draft.cameos.map(({ localId, characterName, description }) => ({
            localId,
            characterName,
            description,
          })),
          canvasReferences: [],
          verticalSkillIds: draft.verticalSkillIds,
        })
      );
    } catch {
      // Storage may be unavailable in hardened webviews.
    }
  }, [draft]);

  useEffect(
    () => () => {
      const current = draftRef.current;
      for (const cameo of current.cameos) {
        if (cameo.previewUrl) URL.revokeObjectURL(cameo.previewUrl);
      }
      for (const reference of current.canvasReferences) {
        URL.revokeObjectURL(reference.previewUrl);
      }
    },
    []
  );

  useEffect(() => {
    if (draft.preferences.models.llm_model) setModelMissing(false);
  }, [draft.preferences.models.llm_model]);

  const activeText = mode === 'agent' ? draft.sourceText : draft.creationPrompt;
  useEffect(() => {
    if (!activeText.trim() || draftedTracked.current) return;
    draftedTracked.current = true;
    trackFunnelEvent('task_drafted', {
      feature: 'video_generation',
      mode,
      workflow: mode === 'agent' ? draft.workflow : draft.creationSkillId,
    });
  }, [activeText, draft.creationSkillId, draft.workflow, mode]);

  const activeCreationSkill =
    creationSkills.find((skill) => skill.id === draft.creationSkillId) ??
    creationSkills[0];
  const selectedModeLabel =
    mode === 'agent'
      ? agentModes.find((item) => item.id === draft.workflow)?.label
      : activeCreationSkill.label;
  const verticalSkillLabel = t('videoGeneration.skills.mountButton', {
    defaultValue: 'Skill',
  });
  const selectedVerticalSkills = useMemo(() => {
    const byId = new Map(skillCatalog.map((skill) => [skill.id, skill]));
    return draft.verticalSkillIds.map((id) => {
      const skill = byId.get(id);
      return {
        id,
        label: skill?.display_name || skill?.name || id.replace(/^[^:]+:/, ''),
      };
    });
  }, [draft.verticalSkillIds, skillCatalog]);

  useEffect(() => {
    if (mode !== 'agent') return;
    if (skillCatalog.length > 0 && skillListReloadToken === 0) return;
    let cancelled = false;
    const loadCatalog = () => listVerticalSkills()
      .then((list) => {
        if (!cancelled) setSkillCatalog(list);
      })
      .catch(() => {
        /* catalog is best-effort for chip labels */
      });
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      const idleId = idleWindow.requestIdleCallback(loadCatalog, { timeout: 1200 });
      return () => {
        cancelled = true;
        idleWindow.cancelIdleCallback?.(idleId);
      };
    }
    const timer = window.setTimeout(loadCatalog, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [mode, skillCatalog.length, skillListReloadToken]);

  const removeVerticalSkill = (skillId: string) => {
    setDraft((current) => ({
      ...current,
      verticalSkillIds: current.verticalSkillIds.filter((id) => id !== skillId),
    }));
  };

  const removeLastVerticalSkill = () => {
    setDraft((current) => {
      if (current.verticalSkillIds.length === 0) return current;
      return {
        ...current,
        verticalSkillIds: current.verticalSkillIds.slice(0, -1),
      };
    });
  };

  const handlePromptKeyDown = (
    event: React.KeyboardEvent<HTMLTextAreaElement>
  ) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
      event.preventDefault();
      submit();
      return;
    }
    if (event.key === 'Escape') {
      setSlashMenuOpen(false);
      return;
    }
    if (
      mode !== 'agent' ||
      loading ||
      draft.verticalSkillIds.length === 0 ||
      (event.key !== 'Backspace' && event.key !== 'Delete')
    ) {
      return;
    }
    const target = event.target as HTMLTextAreaElement;
    const start = target.selectionStart ?? 0;
    const end = target.selectionEnd ?? 0;
    const atStart = start === 0 && end === 0;
    const empty = !activeText;
    // Backspace/Delete at caret start (or empty prompt) removes the trailing skill chip.
    if (event.key === 'Backspace' && (atStart || empty)) {
      event.preventDefault();
      removeLastVerticalSkill();
    } else if (event.key === 'Delete' && empty) {
      event.preventDefault();
      removeLastVerticalSkill();
    }
  };

  const getComposerPopupContainer = (node: HTMLElement) =>
    composerRef.current ?? node.parentElement ?? document.body;

  const placeholder =
    mode === 'creation'
      ? t('videoGeneration.create.composer.creationPlaceholder', {
          defaultValue: '描述你想创作的画面、镜头或氛围，支持 / 唤起风格技能…',
        })
      : draft.workflow === 'script2video'
        ? t('videoGeneration.create.composer.scriptPlaceholder', {
            defaultValue: '粘贴剧本，Flowy 会自动拆成可编辑镜头…',
          })
        : draft.workflow === 'novel2video'
          ? t('videoGeneration.create.composer.novelPlaceholder', {
              defaultValue: '粘贴小说片段，Flowy 会提炼剧情并设计分镜…',
            })
          : t('videoGeneration.create.composer.ideaPlaceholderSlash', {
              defaultValue: '输入一个想法、故事或产品画面，支持 / 切换 Mode…',
            });

  const setActiveText = (value: string) => {
    setDraft((current) =>
      mode === 'agent'
        ? { ...current, sourceText: value }
        : { ...current, creationPrompt: value }
    );
    setSlashMenuOpen(/(?:^|\s)\/$/.test(value));
  };

  const selectAgentMode = (workflow: VimaxWorkflow) => {
    setDraft((current) => ({
      ...current,
      workflow,
      verticalSkillIds: current.verticalSkillIds,
    }));
    setSlashMenuOpen(false);
  };

  const selectCreationSkill = (creationSkillId: CreationSkillId) => {
    const skill = creationSkills.find((item) => item.id === creationSkillId);
    setDraft((current) => ({
      ...current,
      creationSkillId,
      style: skill?.stylePrompt ?? current.style,
    }));
    setSlashMenuOpen(false);
  };

  const removeTrailingSlash = () => {
    setActiveText(activeText.replace(/\/\s*$/, '').trimEnd());
  };

  const addAgentImages = (files: File[]) => {
    const room = Math.max(0, MAX_REFERENCES - draft.cameos.length);
    const added: CameoDraftItem[] = files.slice(0, room).map((file, index) => ({
      localId: makeLocalId('cameo'),
      characterName: suggestCameoCharacterName(file.name, draft.cameos.length + index),
      description: '',
      file,
      previewUrl: URL.createObjectURL(file),
    }));
    setDraft((current) => ({ ...current, cameos: [...current.cameos, ...added] }));
  };

  const addCanvasImages = (files: File[]) => {
    const room = Math.max(0, MAX_REFERENCES - draft.canvasReferences.length);
    const added: CanvasReferenceDraft[] = files.slice(0, room).map((file) => ({
      localId: makeLocalId('reference'),
      file,
      previewUrl: URL.createObjectURL(file),
    }));
    setDraft((current) => ({
      ...current,
      canvasReferences: [...current.canvasReferences, ...added],
    }));
  };

  const handleFiles = async (files: File[]) => {
    if (loading || files.length === 0) return;
    setUploadError(null);
    const images = files.filter(isSupportedImageFile);
    const documents = files.filter(isSupportedTextFile);
    const unsupported = files.filter(
      (file) => !isSupportedImageFile(file) && !isSupportedTextFile(file)
    );
    if (images.length > 0) {
      if (mode === 'agent') addAgentImages(images);
      else addCanvasImages(images);
    }
    if (documents[0]) {
      try {
        const text = await readUploadedTextFile(documents[0]);
        setDocumentName(documents[0].name);
        setDraft((current) =>
          mode === 'agent'
            ? {
                ...current,
                workflow:
                  current.workflow === 'idea2video' ? 'script2video' : current.workflow,
                sourceText: text,
              }
            : { ...current, creationPrompt: text }
        );
      } catch (error) {
        setUploadError(error instanceof Error ? error.message : String(error));
      }
    }
    if (unsupported.length > 0) {
      setUploadError(
        t('videoGeneration.create.upload.unsupported', {
          defaultValue: '部分文件格式暂不支持，请上传图片、DOCX 或纯文本文档。',
        })
      );
    }
  };

  const removeCanvasReference = (localId: string) => {
    const target = draft.canvasReferences.find((item) => item.localId === localId);
    if (target) URL.revokeObjectURL(target.previewUrl);
    setDraft((current) => ({
      ...current,
      canvasReferences: current.canvasReferences.filter(
        (item) => item.localId !== localId
      ),
    }));
  };

  const submit = () => {
    if (!activeText.trim()) return;
    if (mode === 'agent' && !draft.preferences.models.llm_model) {
      setModelMissing(true);
      setPreferencesOpen(true);
      return;
    }
    if (
      mode === 'agent' &&
      draft.cameos.some((cameo) => cameo.file && !cameo.characterName.trim())
    ) {
      setUploadError(
        t('videoGeneration.create.upload.cameoNameRequired', {
          defaultValue: '请为每张角色参考图填写角色名。',
        })
      );
      return;
    }
    const normalized = {
      ...draft,
      sourceText: draft.sourceText.trim(),
      creationPrompt: draft.creationPrompt.trim(),
      style:
        mode === 'creation'
          ? activeCreationSkill.stylePrompt
          : draft.style,
    };
    if (mode === 'agent') onSubmitAgent(normalized);
    else onSubmitCreation(normalized);
  };

  const modeMenu = (
    <div className={styles.modeMenu}>
      <button
        type='button'
        className={`${styles.modeMenuItem} ${
          mode === 'agent' ? styles.modeMenuItemActive : ''
        }`}
        onClick={() => {
          onModeChange('agent');
          setModeMenuOpen(false);
        }}
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
      <button
        type='button'
        className={`${styles.modeMenuItem} ${
          mode === 'creation' ? styles.modeMenuItemActive : ''
        }`}
        onClick={() => {
          onModeChange('creation');
          setModeMenuOpen(false);
        }}
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

  const skillMenu = (
    <div
      className={styles.slashMenu}
      role='listbox'
      aria-label={
        mode === 'agent'
          ? t('videoGeneration.create.modesMenuAria', {
              defaultValue: '选择 Mode',
            })
          : t('videoGeneration.create.skillsMenuAria', {
              defaultValue: '选择技能',
            })
      }
    >
      <div className={styles.slashMenuTitle}>
        {mode === 'agent'
          ? t('videoGeneration.create.modesMenuTitle', {
              defaultValue: '选择 Mode',
            })
          : t('videoGeneration.create.skillsMenuTitle', {
              defaultValue: '选择风格技能',
            })}
      </div>
      {(mode === 'agent' ? agentModes : creationSkills).map((skill) => {
        const icons =
          mode === 'agent'
            ? [
                <VideoOne key='idea' size={15} />,
                <FileText key='script' size={15} />,
                <BookOpen key='novel' size={15} />,
              ]
            : null;
        const agentIndex =
          mode === 'agent'
            ? agentModes.findIndex((item) => item.id === skill.id)
            : -1;
        const active =
          mode === 'agent'
            ? draft.workflow === skill.id
            : draft.creationSkillId === skill.id;
        return (
          <button
            key={skill.id}
            type='button'
            role='option'
            aria-selected={active}
            className={`${styles.slashMenuItem} ${
              active ? styles.slashMenuItemActive : ''
            }`}
            onClick={() => {
              removeTrailingSlash();
              if (mode === 'agent') selectAgentMode(skill.id as VimaxWorkflow);
              else selectCreationSkill(skill.id as CreationSkillId);
            }}
          >
            {icons?.[agentIndex] ??
              (skill.id === 'cinematic' ? <Pic size={15} /> : <Platte size={15} />)}
            <span>
              <strong>{skill.label}</strong>
              <small>{skill.description}</small>
            </span>
          </button>
        );
      })}
    </div>
  );

  const activeReferences =
    mode === 'agent' ? draft.cameos : draft.canvasReferences;

  const uploadPreview =
    mode === 'creation'
      ? draft.canvasReferences[0]?.previewUrl
      : draft.cameos[0]?.previewUrl;

  const prefsSummary = generationPreferencesSummary(draft.preferences, mode, {
    automatic: t('videoGeneration.create.preferences.automatic', { defaultValue: '自动' }),
    smart: t('videoGeneration.create.preferences.smart', { defaultValue: '智能' }),
    noModel: t('videoGeneration.create.preferences.noModelSelected', { defaultValue: '未选模型' }),
  });
  const openPreferences = (open: boolean) => {
    if (open) {
      setModeMenuOpen(false);
      setSlashMenuOpen(false);
      setPrefsModuleReady(true);
    }
    setPreferencesOpen(open);
  };
  const prefsTrigger = (
    <button
      type='button'
      className={`${styles.toolbarButton} ${styles.prefsButton} ${
        preferencesOpen ? styles.toolbarButtonActive : ''
      }`}
      disabled={loading}
      aria-expanded={preferencesOpen}
      aria-haspopup='dialog'
      onClick={() => openPreferences(!preferencesOpen)}
      aria-label={t('videoGeneration.create.customize', { defaultValue: '自定义生成偏好' })}
    >
      <SettingTwo theme='outline' size={15} />
      <span className={styles.toolbarLabel}>
        {t('videoGeneration.create.preferences.customize', { defaultValue: '自定义' })}
      </span>
      <span className={styles.toolbarSummary} title={prefsSummary.title}>
        {prefsSummary.summary}
      </span>
      <Down theme='outline' size={12} />
    </button>
  );

  return (
    <section className={styles.hero}>
      <div className={styles.heroHeading}>
        <h1>
          {t('videoGeneration.create.homeHeroPrefix', {
            defaultValue: '开启你的',
          })}{' '}
          <em>{modeLabel}</em>
          {t('videoGeneration.create.homeHeroSuffix', {
            defaultValue: '，即刻造梦！',
          })}
        </h1>
        <p className={styles.heroHint}>
          {mode === 'agent'
            ? t('videoGeneration.create.homeHintAgent', {
                defaultValue: '写下故事或想法，Flowy 帮你变成成片。',
              })
            : t('videoGeneration.create.homeHintCreation', {
                defaultValue: '描述画面与镜头，在无限画布里自由编排生成。',
              })}
        </p>
      </div>

      <div
        ref={composerRef}
        className={styles.composer}
        onDragOver={(event) => event.preventDefault()}
        onDrop={(event) => {
          event.preventDefault();
          void handleFiles(Array.from(event.dataTransfer.files ?? []));
        }}
      >
        <div className={styles.composerMain}>
          <button
            type='button'
            className={`${styles.uploadSlot} ${
              uploadPreview ? styles.uploadSlotFilled : ''
            } ${documentName && !uploadPreview ? styles.uploadSlotDocument : ''}`}
            disabled={loading}
            onClick={() => fileInputRef.current?.click()}
            title={t('videoGeneration.create.upload.aria', {
              defaultValue:
                '上传角色参考图、剧本或资料文档（PNG / JPEG / WEBP / DOCX / TXT / Markdown 等）',
            })}
            aria-label={t('videoGeneration.create.upload.aria', {
              defaultValue:
                '上传角色参考图、剧本或资料文档（PNG / JPEG / WEBP / DOCX / TXT / Markdown 等）',
            })}
          >
            {uploadPreview ? (
              <img src={uploadPreview} alt='' className={styles.uploadPreview} />
            ) : (
              <span
                className={`${styles.uploadGlyph} ${
                  documentName ? styles.uploadGlyphActive : ''
                }`}
                aria-hidden='true'
              >
                <SlantedDocIcon size={24} className={styles.uploadDocIcon} />
              </span>
            )}
            {activeReferences.length > 1 ? (
              <em className={styles.uploadCount}>+{activeReferences.length - 1}</em>
            ) : documentName && uploadPreview ? (
              <em className={styles.uploadDocBadge} aria-hidden='true'>
                <FileText size={11} />
              </em>
            ) : null}
          </button>
          <input
            ref={fileInputRef}
            type='file'
            accept={VIDEO_HOME_UPLOAD_ACCEPT}
            multiple
            hidden
            disabled={loading}
            onChange={(event) => {
              void handleFiles(Array.from(event.target.files ?? []));
              event.target.value = '';
            }}
          />

          <div className={styles.promptArea}>
            <div className={styles.promptInner}>
              {(documentName ||
                (mode === 'creation' && draft.canvasReferences.length > 0)) && (
                <div className={styles.inlineAttachments}>
                  {documentName ? (
                    <span className={styles.documentChip}>
                      <FileText size={13} />
                      {displayFileStem(documentName)}
                      <button
                        type='button'
                        aria-label={t('videoGeneration.create.upload.removeDocument', {
                          defaultValue: '移除文档',
                        })}
                        onClick={() => {
                          setDocumentName(null);
                          setActiveText('');
                        }}
                      >
                        <CloseSmall size={12} />
                      </button>
                    </span>
                  ) : null}
                  {mode === 'creation'
                    ? draft.canvasReferences.slice(0, 4).map((reference) => (
                        <span key={reference.localId} className={styles.referenceThumb}>
                          <img src={reference.previewUrl} alt={reference.file.name} />
                          <button
                            type='button'
                            aria-label={t('videoGeneration.create.upload.removeReference', {
                              name: reference.file.name,
                              defaultValue: '移除 {{name}}',
                            })}
                            onClick={() => removeCanvasReference(reference.localId)}
                          >
                            <CloseSmall size={12} />
                          </button>
                        </span>
                      ))
                    : null}
                </div>
              )}
              <div className={styles.promptEditor}>
                {mode === 'agent' && selectedVerticalSkills.length > 0 ? (
                  <div className={styles.skillChips}>
                    {selectedVerticalSkills.map((skill, index) => (
                      <React.Fragment key={skill.id}>
                        {index > 0 ? (
                          <span className={styles.skillDiamond} aria-hidden='true' />
                        ) : null}
                        <button
                          type='button'
                          className={styles.skillTag}
                          disabled={loading}
                          title={skill.label}
                          aria-label={t('videoGeneration.skills.removeSelected', {
                            name: skill.label,
                            defaultValue: '移除 Skill {{name}}',
                          })}
                          onClick={() => removeVerticalSkill(skill.id)}
                          onKeyDown={(event) => {
                            if (event.key === 'Backspace' || event.key === 'Delete') {
                              event.preventDefault();
                              event.stopPropagation();
                              removeVerticalSkill(skill.id);
                            }
                          }}
                        >
                          <strong>{skill.label}</strong>
                          <CloseSmall size={11} />
                        </button>
                      </React.Fragment>
                    ))}
                  </div>
                ) : null}
                <TextArea
                  value={activeText}
                  onChange={setActiveText}
                  placeholder={
                    mode === 'agent' && selectedVerticalSkills.length > 0
                      ? ''
                      : placeholder
                  }
                  disabled={loading}
                  className={styles.promptInput}
                  onKeyDown={handlePromptKeyDown}
                />
              </div>
            </div>
          </div>
        </div>

        {uploadError ? <div className={styles.inlineError}>{uploadError}</div> : null}

        <div className={styles.composerToolbar}>
          <div className={styles.toolbarLeft}>
            <Popover
              trigger='click'
              position='bl'
              showArrow={false}
              getPopupContainer={getComposerPopupContainer}
              popupVisible={modeMenuOpen}
              onVisibleChange={(open) => {
                if (open) {
                  setPreferencesOpen(false);
                  setSlashMenuOpen(false);
                  setSkillHubOpen(false);
                }
                setModeMenuOpen(open);
              }}
              content={modeMenu}
            >
              <button type='button' className={`${styles.toolbarButton} ${styles.modeButton}`}>
                {mode === 'agent' ? <RobotOne size={15} /> : <Platte size={15} />}
                <span>{modeLabel}</span>
                <Down size={12} />
              </button>
            </Popover>
            {prefsModuleReady ? (
              <Suspense fallback={prefsTrigger}>
                <GenerationPreferencesPopover
                  mode={mode}
                  value={draft.preferences}
                  disabled={loading}
                  modelMissing={modelMissing}
                  open={preferencesOpen}
                  onOpenChange={openPreferences}
                  onChange={(preferences) =>
                    setDraft((current) => ({ ...current, preferences }))
                  }
                  onOpenModelHub={() => navigate('/models')}
                />
              </Suspense>
            ) : (
              prefsTrigger
            )}
            <Popover
              trigger='click'
              position='bl'
              showArrow={false}
              getPopupContainer={getComposerPopupContainer}
              popupVisible={slashMenuOpen}
              onVisibleChange={(open) => {
                if (open) {
                  setPreferencesOpen(false);
                  setModeMenuOpen(false);
                  setSkillHubOpen(false);
                }
                setSlashMenuOpen(open);
              }}
              content={skillMenu}
            >
              <button
                type='button'
                className={`${styles.toolbarButton} ${styles.skillToolbarButton} ${
                  slashMenuOpen ? styles.toolbarButtonActive : ''
                }`}
                aria-expanded={slashMenuOpen}
                aria-label={
                  selectedModeLabel ??
                  (mode === 'agent'
                    ? t('videoGeneration.create.modesMenuAria', {
                        defaultValue: '选择 Mode',
                      })
                    : t('videoGeneration.create.skillsMenuAria', {
                        defaultValue: '选择技能',
                      }))
                }
              >
                <MagicWand size={15} />
                <span className={styles.toolbarLabel}>{selectedModeLabel}</span>
              </button>
            </Popover>
            {mode === 'agent' ? (
              <Popover
                trigger='click'
                position='bl'
                showArrow={false}
                getPopupContainer={getComposerPopupContainer}
                triggerProps={{ autoFitPosition: false, updateOnScroll: true }}
                className={styles.skillPopover}
                style={{ maxWidth: 380, padding: 0 }}
                popupVisible={skillHubOpen}
                onVisibleChange={(open) => {
                  if (open) {
                    setPreferencesOpen(false);
                    setModeMenuOpen(false);
                    setSlashMenuOpen(false);
                  }
                  setSkillHubOpen(open);
                }}
                content={
                  skillHubOpen ? (
                    <Suspense fallback={<div className={styles.slashMenu} />}>
                      <VerticalSkillMenu
                        selectedIds={draft.verticalSkillIds}
                        reloadToken={skillListReloadToken}
                        onChangeSelected={(verticalSkillIds) =>
                          setDraft((current) => ({ ...current, verticalSkillIds }))
                        }
                        onCatalogChange={(list) => {
                          setSkillCatalog((prev) => {
                            const map = new Map(prev.map((skill) => [skill.id, skill]));
                            list.forEach((skill) => map.set(skill.id, skill));
                            return Array.from(map.values());
                          });
                        }}
                        onRequestCreate={() => {
                          setSkillHubOpen(false);
                          setSkillCreateOpen(true);
                        }}
                      />
                    </Suspense>
                  ) : null
                }
              >
                <button
                  type='button'
                  className={`${styles.toolbarButton} ${styles.skillToolbarButton} ${
                    skillHubOpen || draft.verticalSkillIds.length > 0
                      ? styles.toolbarButtonActive
                      : ''
                  }`}
                  aria-expanded={skillHubOpen}
                  aria-label={verticalSkillLabel}
                >
                  <Star size={15} />
                  <span className={styles.toolbarLabel}>{verticalSkillLabel}</span>
                </button>
              </Popover>
            ) : null}
          </div>
          <button
            type='button'
            data-button-shape='circle'
            className={styles.submitButton}
            disabled={loading || !activeText.trim()}
            onClick={submit}
            aria-label={
              mode === 'agent'
                ? t('videoGeneration.create.generateStoryboard', {
                    defaultValue: '生成分镜',
                  })
                : t('videoGeneration.create.enterCanvas', {
                    defaultValue: '进入画布',
                  })
            }
          >
            {loading ? (
              <span className={styles.submitSpinner} />
            ) : (
              <BoldSendArrowIcon size={17} className={styles.submitArrow} />
            )}
          </button>
        </div>
      </div>

      {mode === 'agent' && draft.cameos.length > 0 ? (
        <div className={styles.cameoPanel}>
          <Suspense fallback={null}>
            <CameoCastEditor
              value={draft.cameos}
              disabled={loading}
              onChange={(cameos) => setDraft((current) => ({ ...current, cameos }))}
            />
          </Suspense>
        </div>
      ) : null}

      {skillCreateOpen ? (
        <Suspense fallback={null}>
          <VerticalSkillCreateModal
            visible={skillCreateOpen}
            onClose={() => setSkillCreateOpen(false)}
            onCreated={(skillId) => {
              setDraft((current) => ({
                ...current,
                verticalSkillIds: current.verticalSkillIds.includes(skillId)
                  ? current.verticalSkillIds
                  : [...current.verticalSkillIds, skillId],
              }));
              setSkillListReloadToken((n) => n + 1);
            }}
          />
        </Suspense>
      ) : null}
    </section>
  );
};

export default VideoHomeComposer;
