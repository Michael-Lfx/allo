import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Input, Popover } from '@arco-design/web-react';
import {
  ArrowUp,
  CloseSmall,
  Down,
  FileText,
  MagicWand,
  People,
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
import { listPipelines } from '../api';
import type { PipelineSummary } from '../types';
import GenerationPreferencesPopover from './GenerationPreferencesPopover';
import {
  displayFileStem,
  isSupportedImageFile,
  isSupportedTextFile,
  readUploadedTextFile,
  VIDEO_HOME_UPLOAD_ACCEPT,
} from './documentUpload';
import type {
  AgentPipelineOption,
  CanvasReferenceDraft,
  CreationSkillDefinition,
  CreationSkillId,
  GenerationPreferences,
  VideoCreateDraft,
  VideoHomeMode,
} from './types';
import { pipelineDescription, pipelineLabel } from '../components/SessionCard';
import styles from './home.module.css';

const TextArea = Input.TextArea;
const DRAFT_KEY = 'flowy.videoGeneration.homeDraft.v3';
const LEGACY_DRAFT_KEY = 'flowy.videoGeneration.homeDraft.v2';
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
    pipeline: '',
    sourceText: '',
    creationPrompt: '',
    creationSkillId: 'cinematic',
    requirement: '',
    style: DEFAULT_VISUAL_STYLE_PROMPT,
    preferences: DEFAULT_PREFERENCES,
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
    const creationSkillId = CREATION_SKILL_IDS.includes(
      parsed.creationSkillId as CreationSkillId
    )
      ? (parsed.creationSkillId as CreationSkillId)
      : 'cinematic';
    return {
      ...fallback,
      pipeline: typeof parsed.pipeline === 'string' ? parsed.pipeline : '',
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

function pipelineModeFilter(mode: VideoHomeMode): string {
  return mode === 'avatar' ? 'avatar' : 'agent';
}

function matchesPipelineMode(pipeline: PipelineSummary, mode: VideoHomeMode): boolean {
  const want = pipelineModeFilter(mode);
  if (want === 'avatar') {
    return pipeline.mode === 'avatar' || pipeline.mode === 'talking_head';
  }
  return pipeline.mode === 'agent';
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
  const [pipelines, setPipelines] = useState<PipelineSummary[]>([]);
  const [pipelinesLoading, setPipelinesLoading] = useState(false);
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const [slashMenuOpen, setSlashMenuOpen] = useState(false);
  const [modelMissing, setModelMissing] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [documentName, setDocumentName] = useState<string | null>(null);
  const draftRef = useRef(draft);

  const isMontageMode = mode === 'agent' || mode === 'avatar';

  useEffect(() => {
    if (!isMontageMode) return;
    let cancelled = false;
    setPipelinesLoading(true);
    void listPipelines()
      .then((list) => {
        if (!cancelled) setPipelines(list);
      })
      .catch(() => {
        if (!cancelled) setPipelines([]);
      })
      .finally(() => {
        if (!cancelled) setPipelinesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [isMontageMode]);

  const agentPipelines = useMemo<AgentPipelineOption[]>(() => {
    return pipelines
      .filter((p) => matchesPipelineMode(p, mode))
      .map((p) => ({
        id: p.name,
        label: pipelineLabel(p.name, t),
        description: pipelineDescription(p.name, t, p.description || p.category || ''),
        stability: p.stability,
      }));
  }, [pipelines, mode, t]);

  useEffect(() => {
    if (!isMontageMode || agentPipelines.length === 0) return;
    setDraft((current) => {
      if (current.pipeline && agentPipelines.some((p) => p.id === current.pipeline)) {
        return current;
      }
      return { ...current, pipeline: agentPipelines[0].id };
    });
  }, [agentPipelines, isMontageMode]);

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
  const avatarModeLabel = t('videoGeneration.mode.avatarLabel', {
    defaultValue: '数字人',
  });
  const creationModeLabel = t('videoGeneration.mode.creationLabel', {
    defaultValue: '创作模式',
  });
  const modeLabel =
    mode === 'agent'
      ? agentModeLabel
      : mode === 'avatar'
        ? avatarModeLabel
        : creationModeLabel;

  useEffect(() => {
    draftRef.current = draft;
    try {
      window.sessionStorage.setItem(
        DRAFT_KEY,
        JSON.stringify({
          ...draft,
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
      for (const reference of current.canvasReferences) {
        URL.revokeObjectURL(reference.previewUrl);
      }
    },
    []
  );

  useEffect(() => {
    if (draft.preferences.models.llm_model) setModelMissing(false);
  }, [draft.preferences.models.llm_model]);

  const activeText = isMontageMode ? draft.sourceText : draft.creationPrompt;
  useEffect(() => {
    if (!activeText.trim() || draftedTracked.current) return;
    draftedTracked.current = true;
    trackFunnelEvent('task_drafted', {
      feature: 'video_generation',
      mode,
      pipeline: isMontageMode ? draft.pipeline : draft.creationSkillId,
    });
  }, [activeText, draft.creationSkillId, draft.pipeline, isMontageMode, mode]);

  const activeCreationSkill =
    creationSkills.find((skill) => skill.id === draft.creationSkillId) ??
    creationSkills[0];
  const selectedSkillLabel = isMontageMode
    ? agentPipelines.find((p) => p.id === draft.pipeline)?.label ??
      t('videoGeneration.create.pipelinePick', { defaultValue: '选择管线' })
    : activeCreationSkill.label;
  const placeholder =
    mode === 'creation'
      ? t('videoGeneration.create.composer.creationPlaceholder', {
          defaultValue: '描述你想创作的画面、镜头或氛围，支持 / 唤起风格技能…',
        })
      : mode === 'avatar'
        ? t('videoGeneration.create.composer.avatarPlaceholder', {
            defaultValue: '描述数字人口播脚本或表演内容，支持 / 选择管线…',
          })
        : t('videoGeneration.create.composer.agentPlaceholder', {
            defaultValue: '输入一个想法、故事或产品画面，支持 / 选择制片管线…',
          });

  const setActiveText = (value: string) => {
    setDraft((current) =>
      isMontageMode
        ? { ...current, sourceText: value }
        : { ...current, creationPrompt: value }
    );
    setSlashMenuOpen(/(?:^|\s)\/$/.test(value));
  };

  const selectPipeline = (pipeline: string) => {
    setDraft((current) => ({ ...current, pipeline }));
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
      if (mode === 'creation') addCanvasImages(images);
      else {
        setUploadError(
          t('videoGeneration.create.upload.montageNoImageYet', {
            defaultValue: 'Agent / 数字人模式暂不支持参考图上传，请改用文本描述。',
          })
        );
      }
    }
    if (documents[0]) {
      try {
        const text = await readUploadedTextFile(documents[0]);
        setDocumentName(documents[0].name);
        setDraft((current) =>
          isMontageMode
            ? { ...current, sourceText: text }
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
    if (isMontageMode && !draft.pipeline) {
      setUploadError(
        t('videoGeneration.create.pipelineRequired', {
          defaultValue: '请先选择一条制片管线。',
        })
      );
      setSlashMenuOpen(true);
      return;
    }
    if (isMontageMode && !draft.preferences.models.llm_model) {
      setModelMissing(true);
      setPreferencesOpen(true);
      return;
    }
    const normalized = {
      ...draft,
      sourceText: draft.sourceText.trim(),
      creationPrompt: draft.creationPrompt.trim(),
      style: mode === 'creation' ? activeCreationSkill.stylePrompt : draft.style,
    };
    if (isMontageMode) onSubmitAgent(normalized);
    else onSubmitCreation(normalized);
  };

  const modeMenu = (
    <div className={styles.modeMenu}>
      <button
        type='button'
        className={`${styles.modeMenuItem} ${mode === 'agent' ? styles.modeMenuItemActive : ''}`}
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
              defaultValue: '多阶段制片管线，自动规划并成片',
            })}
          </small>
        </span>
      </button>
      <button
        type='button'
        className={`${styles.modeMenuItem} ${mode === 'avatar' ? styles.modeMenuItemActive : ''}`}
        onClick={() => {
          onModeChange('avatar');
          setModeMenuOpen(false);
        }}
      >
        <People theme='outline' size={18} />
        <span>
          <strong>{avatarModeLabel}</strong>
          <small>
            {t('videoGeneration.mode.avatarMenuDesc', {
              defaultValue: '数字人口播 / talking-head 管线',
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
        {isMontageMode
          ? t('videoGeneration.create.pipelineMenuTitle', {
              defaultValue: '选择管线',
            })
          : t('videoGeneration.create.skillsMenuTitle', {
              defaultValue: '选择技能',
            })}
      </div>
      {isMontageMode
        ? agentPipelines.map((pipeline) => {
            const active = draft.pipeline === pipeline.id;
            return (
              <button
                key={pipeline.id}
                type='button'
                role='option'
                aria-selected={active}
                className={`${styles.slashMenuItem} ${
                  active ? styles.slashMenuItemActive : ''
                }`}
                onClick={() => {
                  removeTrailingSlash();
                  selectPipeline(pipeline.id);
                }}
              >
                <VideoOne size={15} />
                <span>
                  <strong>{pipeline.label}</strong>
                  <small>{pipeline.description || pipeline.stability || ''}</small>
                </span>
              </button>
            );
          })
        : creationSkills.map((skill) => {
            const active = draft.creationSkillId === skill.id;
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
                  selectCreationSkill(skill.id);
                }}
              >
                {skill.id === 'cinematic' ? <Pic size={15} /> : <Platte size={15} />}
                <span>
                  <strong>{skill.label}</strong>
                  <small>{skill.description}</small>
                </span>
              </button>
            );
          })}
      {isMontageMode && !pipelinesLoading && agentPipelines.length === 0 ? (
        <div className={styles.slashMenuTitle}>
          {t('videoGeneration.create.pipelineEmpty', {
            defaultValue: '暂无可用管线',
          })}
        </div>
      ) : null}
    </div>
  );

  const uploadPreview =
    mode === 'creation' ? draft.canvasReferences[0]?.previewUrl : null;

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
                defaultValue: '选择制片管线，写下故事或想法，Flowy 帮你变成成片。',
              })
            : mode === 'avatar'
              ? t('videoGeneration.create.homeHintAvatar', {
                  defaultValue: '选择数字人管线，输入口播内容即可生成 talking-head。',
                })
              : t('videoGeneration.create.homeHintCreation', {
                  defaultValue: '描述画面与镜头，在无限画布里自由编排生成。',
                })}
        </p>
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
            {mode === 'creation' && draft.canvasReferences.length > 1 ? (
              <em className={styles.uploadCount}>+{draft.canvasReferences.length - 1}</em>
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
                {mode === 'agent' ? (
                  <RobotOne size={15} />
                ) : mode === 'avatar' ? (
                  <People size={15} />
                ) : (
                  <Platte size={15} />
                )}
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
              isMontageMode
                ? t('videoGeneration.create.startPipeline', {
                    defaultValue: '开始制片',
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
    </section>
  );
};

export default VideoHomeComposer;
