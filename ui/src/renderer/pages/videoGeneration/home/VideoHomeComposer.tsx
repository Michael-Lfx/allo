import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Input, Popover } from '@arco-design/web-react';
import {
  ArrowUp,
  BookOpen,
  CloseSmall,
  Down,
  FileText,
  MagicWand,
  Pic,
  Platte,
  RobotOne,
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
import CameoCastEditor from '../components/CameoCastEditor';
import { suggestCameoCharacterName } from '../cameoUtils';
import type { CameoDraftItem, VimaxWorkflow } from '../types';
import GenerationPreferencesPopover from './GenerationPreferencesPopover';
import {
  displayFileStem,
  isSupportedImageFile,
  isSupportedTextFile,
  readUploadedTextFile,
  VIDEO_HOME_UPLOAD_ACCEPT,
} from './documentUpload';
import type {
  AgentSkillDefinition,
  CanvasReferenceDraft,
  CreationSkillDefinition,
  CreationSkillId,
  GenerationPreferences,
  VideoCreateDraft,
  VideoHomeMode,
} from './types';
import styles from './home.module.css';

const TextArea = Input.TextArea;
const DRAFT_KEY = 'flowy.videoGeneration.homeDraft.v2';
const LEGACY_DRAFT_KEY = 'flowy.videoGeneration.draft.v1';
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
      window.sessionStorage.getItem(LEGACY_DRAFT_KEY);
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
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const [slashMenuOpen, setSlashMenuOpen] = useState(false);
  const [modelMissing, setModelMissing] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [documentName, setDocumentName] = useState<string | null>(null);
  const draftRef = useRef(draft);

  const agentSkills = useMemo<AgentSkillDefinition[]>(
    () => [
      {
        id: 'idea2video',
        label: t('videoGeneration.create.modes.idea', { defaultValue: '一个想法' }),
        description: t('videoGeneration.create.skills.ideaDesc', {
          defaultValue: '从一句灵感扩写成完整影片',
        }),
      },
      {
        id: 'script2video',
        label: t('videoGeneration.create.modes.script', { defaultValue: '完整剧本' }),
        description: t('videoGeneration.create.skills.scriptDesc', {
          defaultValue: '按剧情结构自动拆解镜头',
        }),
      },
      {
        id: 'novel2video',
        label: t('videoGeneration.create.modes.novel', { defaultValue: '小说文本' }),
        description: t('videoGeneration.create.skills.novelDesc', {
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
  const selectedSkillLabel =
    mode === 'agent'
      ? agentSkills.find((skill) => skill.id === draft.workflow)?.label
      : activeCreationSkill.label;
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
              defaultValue: '输入一个想法、故事或产品画面，支持 / 唤起技能…',
            });

  const setActiveText = (value: string) => {
    setDraft((current) =>
      mode === 'agent'
        ? { ...current, sourceText: value }
        : { ...current, creationPrompt: value }
    );
    setSlashMenuOpen(/(?:^|\s)\/$/.test(value));
  };

  const selectAgentSkill = (workflow: VimaxWorkflow) => {
    setDraft((current) => ({
      ...current,
      workflow,
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
      aria-label={t('videoGeneration.create.skillsMenuAria', {
        defaultValue: '选择技能',
      })}
    >
      <div className={styles.slashMenuTitle}>
        {t('videoGeneration.create.skillsMenuTitle', {
          defaultValue: '选择技能',
        })}
      </div>
      {(mode === 'agent' ? agentSkills : creationSkills).map((skill) => {
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
            ? agentSkills.findIndex((item) => item.id === skill.id)
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
              if (mode === 'agent') selectAgentSkill(skill.id as VimaxWorkflow);
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
      </div>

      <div
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
            }`}
            disabled={loading}
            onClick={() => fileInputRef.current?.click()}
            aria-label={t('videoGeneration.create.upload.aria', {
              defaultValue: '上传图片、剧本或其他文档',
            })}
          >
            {uploadPreview ? (
              <img src={uploadPreview} alt='' className={styles.uploadPreview} />
            ) : (
              <span className={styles.uploadPlus} aria-hidden='true'>
                +
              </span>
            )}
            {activeReferences.length > 1 ? (
              <em className={styles.uploadCount}>+{activeReferences.length - 1}</em>
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
              <TextArea
                value={activeText}
                onChange={setActiveText}
                placeholder={placeholder}
                disabled={loading}
                className={styles.promptInput}
                onKeyDown={(event) => {
                  if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
                    event.preventDefault();
                    submit();
                  }
                  if (event.key === 'Escape') setSlashMenuOpen(false);
                }}
              />
            </div>
          </div>
        </div>

        {uploadError ? <div className={styles.inlineError}>{uploadError}</div> : null}

        <div className={styles.composerToolbar}>
          <div className={styles.toolbarLeft}>
            <Popover
              trigger='click'
              position='bl'
              popupVisible={modeMenuOpen}
              onVisibleChange={(open) => {
                if (open) {
                  setPreferencesOpen(false);
                  setSlashMenuOpen(false);
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
            <GenerationPreferencesPopover
              mode={mode}
              value={draft.preferences}
              disabled={loading}
              modelMissing={modelMissing}
              open={preferencesOpen}
              onOpenChange={(open) => {
                if (open) {
                  setModeMenuOpen(false);
                  setSlashMenuOpen(false);
                }
                setPreferencesOpen(open);
              }}
              onChange={(preferences) =>
                setDraft((current) => ({ ...current, preferences }))
              }
              onOpenModelHub={() => navigate('/models')}
            />
            <Popover
              trigger='click'
              position='bl'
              popupVisible={slashMenuOpen}
              onVisibleChange={(open) => {
                if (open) {
                  setPreferencesOpen(false);
                  setModeMenuOpen(false);
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
                  selectedSkillLabel ??
                  t('videoGeneration.create.skillsMenuAria', {
                    defaultValue: '选择技能',
                  })
                }
              >
                <MagicWand size={15} />
                <span className={styles.toolbarLabel}>{selectedSkillLabel}</span>
              </button>
            </Popover>
          </div>
          <button
            type='button'
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
            {loading ? <span className={styles.submitSpinner} /> : <ArrowUp size={18} />}
          </button>
        </div>
      </div>

      {mode === 'agent' && draft.cameos.length > 0 ? (
        <div className={styles.cameoPanel}>
          <CameoCastEditor
            value={draft.cameos}
            disabled={loading}
            onChange={(cameos) => setDraft((current) => ({ ...current, cameos }))}
          />
        </div>
      ) : null}
    </section>
  );
};

export default VideoHomeComposer;
