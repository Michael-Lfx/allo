import React, {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { Popover } from '@arco-design/web-react';
import {
  Down,
  MagicWand,
  People,
  Platte,
  RobotOne,
  SettingTwo,
  Star,
  VideoOne,
} from '@icon-park/react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import { isActionImitationWorkflow } from '../workflowKind';
import { AttachPlusIcon, BoldSendArrowIcon } from './ComposerIcons';
import { ActionUploadSlots } from './ActionUploadSlots';
import { ModeMenu } from './ModeMenu';
import { PromptComposer } from './PromptComposer';
import { SlashSkillMenu } from './SlashSkillMenu';
import {
  filesFromClipboardData,
  VIDEO_HOME_UPLOAD_ACCEPT,
} from './documentUpload';
import { agentModesFor, creationSkillsFor } from './modeCatalog';
import { useHomeDraft } from './useHomeDraft';
import { useHomeUpload } from './useHomeUpload';
import { useVerticalSkillHub } from './useVerticalSkillHub';
import type { VimaxWorkflow } from '../types';
import type {
  CreationSkillId,
  VideoCreateDraft,
  VideoHomeMode,
} from './types';
import { usesCanvasReferences } from './types';
import { generationPreferencesSummary } from '../preferenceSummary';
import {
  CLIP_DURATION_DEFAULT_SECS,
  CLIP_DURATION_MAX_SECS,
  CLIP_DURATION_MIN_SECS,
  CLIP_DURATION_STEP_SECS,
  clampDuration,
} from '../durationBounds';
import {
  prefetchCanvasAssistantPanel,
  prefetchGenerationPreferencesPanel,
  prefetchVerticalSkillMenu,
} from '../prefetch';
import styles from './home.module.css';

const CameoCastEditor = lazy(() => import('../components/CameoCastEditor'));
const loadGenerationPreferencesPopover = () => import('./GenerationPreferencesPopover');
const GenerationPreferencesPopover = lazy(loadGenerationPreferencesPopover);
const VerticalSkillMenu = lazy(() => import('./VerticalSkillMenu'));
const VerticalSkillCreateModal = lazy(() => import('./VerticalSkillCreateModal'));
const CampaignCarousel = lazy(() => import('../components/CampaignCarousel'));

function GenerationPreferencesMount({
  onMounted,
  children,
}: {
  onMounted: () => void;
  children: React.ReactNode;
}) {
  useLayoutEffect(() => {
    onMounted();
  }, [onMounted]);
  return children;
}

export { clearVideoHomeDraft } from './homeDraft';

interface VideoHomeComposerProps {
  mode: VideoHomeMode;
  loading?: boolean;
  onModeChange: (mode: VideoHomeMode) => void;
  onSubmitAgent: (draft: VideoCreateDraft) => void;
  onSubmitCreation: (draft: VideoCreateDraft) => void;
  onSubmitGenerate: (draft: VideoCreateDraft) => void;
}

const VideoHomeComposer: React.FC<VideoHomeComposerProps> = ({
  mode,
  loading,
  onModeChange,
  onSubmitAgent,
  onSubmitCreation,
  onSubmitGenerate,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const draftedTracked = useRef(false);
  const { draft, setDraft } = useHomeDraft();
  const isAction = mode === 'action';
  const isGenerate = mode === 'generate';
  const {
    handleFiles,
    uploadError,
    setUploadError,
    documentName,
    setDocumentName,
    setActionCharacter,
    setActionVideo,
    removeCanvasReference,
    removeCameo,
  } = useHomeUpload({
    draft,
    setDraft,
    mode,
    isAction,
    loading: loading ?? false,
    t,
  });
  const [preferencesOpen, setPreferencesOpen] = useState(false);
  const [prefsModuleReady, setPrefsModuleReady] = useState(false);
  const [prefsHydrated, setPrefsHydrated] = useState(false);
  const [modeMenuOpen, setModeMenuOpen] = useState(false);
  const [slashMenuOpen, setSlashMenuOpen] = useState(false);
  const [skillHubOpen, setSkillHubOpen] = useState(false);
  const [skillCreateOpen, setSkillCreateOpen] = useState(false);
  const [fileDragOver, setFileDragOver] = useState(false);
  const [modelMissing, setModelMissing] = useState(false);
  const composerRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const {
    skillCatalog,
    mergeCatalog,
    reloadToken: skillListReloadToken,
    bumpReloadToken,
    selectedVerticalSkills,
  } = useVerticalSkillHub(mode, draft.verticalSkillIds);

  const agentModes = useMemo(() => agentModesFor(t), [t]);
  const creationSkills = useMemo(() => creationSkillsFor(t), [t]);

  const generateModeLabel = t('videoGeneration.mode.generateLabel', {
    defaultValue: '视频生成',
  });
  const agentModeLabel = t('videoGeneration.mode.agentLabel', {
    defaultValue: '短剧模式',
  });
  const creationModeLabel = t('videoGeneration.mode.creationLabel', {
    defaultValue: '创作模式',
  });
  const actionModeLabel = t('videoGeneration.mode.actionLabel', {
    defaultValue: '动作模仿',
  });
  const modeLabel =
    mode === 'generate'
      ? generateModeLabel
      : mode === 'agent'
        ? agentModeLabel
        : mode === 'creation'
          ? creationModeLabel
          : actionModeLabel;

  useEffect(() => {
    setDraft((current) => {
      if (mode === 'action') {
        if (current.workflow === 'action2video') return current;
        return {
          ...current,
          workflow: 'action2video',
          verticalSkillIds: [],
          preferences: { ...current.preferences, mediaKind: 'video' },
        };
      }
      if (mode === 'generate') {
        const clipped = clampDuration(
          current.preferences.targetDurationSecs,
          CLIP_DURATION_MIN_SECS,
          CLIP_DURATION_MAX_SECS,
          CLIP_DURATION_STEP_SECS
        );
        const nextDuration =
          current.preferences.targetDurationSecs > CLIP_DURATION_MAX_SECS ||
          current.preferences.targetDurationSecs < CLIP_DURATION_MIN_SECS
            ? CLIP_DURATION_DEFAULT_SECS
            : clipped;
        if (
          current.preferences.mediaKind === 'video' &&
          !current.preferences.automatic &&
          current.preferences.targetDurationSecs === nextDuration &&
          !isActionImitationWorkflow(current.workflow)
        ) {
          return current;
        }
        return {
          ...current,
          workflow: isActionImitationWorkflow(current.workflow)
            ? 'idea2video'
            : current.workflow,
          preferences: {
            ...current.preferences,
            mediaKind: 'video',
            automatic: false,
            targetDurationSecs: nextDuration,
          },
        };
      }
      if (isActionImitationWorkflow(current.workflow)) {
        return { ...current, workflow: 'idea2video' };
      }
      return current;
    });
    setSlashMenuOpen(false);
    setSkillHubOpen(false);
    setPreferencesOpen(false);
    setUploadError(null);
  }, [mode]);

  useEffect(() => {
    if (isAction || isGenerate) {
      if (draft.preferences.models.video_model) setModelMissing(false);
      return;
    }
    if (draft.preferences.models.llm_model) setModelMissing(false);
  }, [
    draft.preferences.models.llm_model,
    draft.preferences.models.video_model,
    isAction,
    isGenerate,
  ]);

  const activeText = usesCanvasReferences(mode)
    ? draft.creationPrompt
    : draft.sourceText;
  useEffect(() => {
    if (isAction) {
      if (!draft.actionCharacter || !draft.actionVideo || draftedTracked.current) return;
      draftedTracked.current = true;
      trackFunnelEvent('task_drafted', {
        feature: 'video_generation',
        mode,
        workflow: draft.workflow,
      });
      return;
    }
    if (!activeText.trim() || draftedTracked.current) return;
    draftedTracked.current = true;
    trackFunnelEvent('task_drafted', {
      feature: 'video_generation',
      mode,
      workflow:
        mode === 'creation'
          ? draft.creationSkillId
          : mode === 'generate'
            ? 'clip'
            : draft.workflow,
    });
  }, [
    activeText,
    draft.actionCharacter,
    draft.actionVideo,
    draft.creationSkillId,
    draft.workflow,
    isAction,
    mode,
  ]);

  const activeCreationSkill =
    creationSkills.find((skill) => skill.id === draft.creationSkillId) ??
    creationSkills[0];
  const selectedModeLabel =
    mode === 'creation'
      ? activeCreationSkill.label
      : agentModes.find((item) => item.id === draft.workflow)?.label;
  const verticalSkillLabel = t('videoGeneration.skills.mountButton', {
    defaultValue: 'Skill',
  });

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

  const openFilePicker = () => {
    fileInputRef.current?.click();
  };

  const placeholder =
    mode === 'generate'
      ? t('videoGeneration.create.composer.generatePlaceholder', {
          defaultValue: '描述你想生成的画面与运动，可上传参考图…',
        })
      : mode === 'creation'
        ? t('videoGeneration.create.composer.creationPlaceholder', {
            defaultValue: '描述你想创作的画面、镜头或氛围，支持 / 唤起风格技能…',
          })
        : draft.workflow === 'script2video'
          ? t('videoGeneration.create.composer.scriptPlaceholder', {
              defaultValue:
                '粘贴剧本；自动按集/场拆分，默认拍全集。需求可写「拍第N集」「前N场」',
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
      usesCanvasReferences(mode)
        ? { ...current, creationPrompt: value }
        : { ...current, sourceText: value }
    );
    // Slash skill menu is Agent / Creation only.
    if (mode === 'agent' || mode === 'creation') {
      setSlashMenuOpen(/(?:^|\s)\/$/.test(value));
    }
  };

  const selectAgentMode = (workflow: VimaxWorkflow) => {
    setDraft((current) => ({
      ...current,
      workflow,
      verticalSkillIds:
        workflow === 'action2video' ? [] : current.verticalSkillIds,
      preferences: {
        ...current.preferences,
        mediaKind: 'video',
      },
    }));
    setSlashMenuOpen(false);
    setUploadError(null);
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

  const markPrefsHydrated = useCallback(() => {
    setPrefsHydrated(true);
  }, []);

  useEffect(() => {
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    const warm = () => {
      setPrefsModuleReady(true);
      prefetchGenerationPreferencesPanel();
      if (mode === 'agent') prefetchVerticalSkillMenu();
      if (mode === 'creation') prefetchCanvasAssistantPanel();
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      const idleId = idleWindow.requestIdleCallback(warm, { timeout: 800 });
      return () => idleWindow.cancelIdleCallback?.(idleId);
    }
    const timer = window.setTimeout(warm, 120);
    return () => window.clearTimeout(timer);
  }, [mode]);

  const openPreferences = (open: boolean) => {
    if (open) {
      setModeMenuOpen(false);
      setSlashMenuOpen(false);
      setSkillHubOpen(false);
      setPrefsModuleReady(true);
      prefetchGenerationPreferencesPanel();
      setPreferencesOpen(true);
      return;
    }
    setPreferencesOpen(false);
  };

  const submit = () => {
    if (isAction) {
      if (!draft.actionCharacter?.file || !draft.actionVideo?.file) {
        setUploadError(
          t('videoGeneration.create.action.required', {
            defaultValue: '请上传一张角色图和一个参考视频。',
          })
        );
        return;
      }
      if (!draft.preferences.models.video_model) {
        setModelMissing(true);
        openPreferences(true);
        return;
      }
      openPreferences(false);
      setModelMissing(false);
      onSubmitAgent({
        ...draft,
        workflow: 'action2video',
        sourceText: '',
        creationPrompt: '',
        verticalSkillIds: [],
        preferences: { ...draft.preferences, mediaKind: 'video' },
      });
      return;
    }
    if (!activeText.trim()) return;
    if (isGenerate && !draft.preferences.models.video_model) {
      setModelMissing(true);
      openPreferences(true);
      return;
    }
    if (mode === 'agent' && !draft.preferences.models.llm_model) {
      setModelMissing(true);
      openPreferences(true);
      return;
    }
    const cameosWithLabels =
      mode === 'agent'
        ? draft.cameos.map((cameo, index) =>
            cameo.file && !cameo.characterName.trim()
              ? { ...cameo, characterName: `参考图${index + 1}` }
              : cameo
          )
        : draft.cameos;
    const normalized = {
      ...draft,
      cameos: cameosWithLabels,
      sourceText: draft.sourceText.trim(),
      creationPrompt: draft.creationPrompt.trim(),
      style:
        mode === 'creation'
          ? activeCreationSkill.stylePrompt
          : draft.style,
      preferences: isGenerate
        ? {
            ...draft.preferences,
            mediaKind: 'video' as const,
            targetDurationSecs: clampDuration(
              draft.preferences.targetDurationSecs,
              CLIP_DURATION_MIN_SECS,
              CLIP_DURATION_MAX_SECS,
              CLIP_DURATION_STEP_SECS
            ),
          }
        : draft.preferences,
    };
    openPreferences(false);
    setModelMissing(false);
    if (mode === 'agent') onSubmitAgent(normalized);
    else if (mode === 'generate') onSubmitGenerate(normalized);
    else onSubmitCreation(normalized);
  };

  const handleModeSelect = (nextMode: VideoHomeMode) => {
    onModeChange(nextMode);
    setModeMenuOpen(false);
  };

  const skillMenu =
    mode === 'agent' ? (
      <SlashSkillMenu
        mode='agent'
        items={agentModes}
        selectedId={draft.workflow}
        onSelect={(id) => {
          removeTrailingSlash();
          selectAgentMode(id as VimaxWorkflow);
        }}
      />
    ) : mode === 'creation' ? (
      <SlashSkillMenu
        mode='creation'
        items={creationSkills}
        selectedId={draft.creationSkillId}
        onSelect={(id) => {
          removeTrailingSlash();
          selectCreationSkill(id as CreationSkillId);
        }}
      />
    ) : null;

  const prefsSummary = generationPreferencesSummary(
    draft.preferences,
    mode,
    {
      automatic: t('videoGeneration.create.preferences.automatic', { defaultValue: '自动' }),
      smart: t('videoGeneration.create.preferences.smart', { defaultValue: '智能' }),
      noModel: t('videoGeneration.create.preferences.noModelSelected', { defaultValue: '未选模型' }),
    },
    isAction ? 'action2video' : undefined
  );
  const prefsTrigger = (
    <button
      type='button'
      className={`${styles.toolbarButton} ${styles.prefsButton} ${
        preferencesOpen ? styles.toolbarButtonActive : ''
      }`}
      disabled={loading}
      aria-expanded={preferencesOpen}
      aria-haspopup='dialog'
      onMouseEnter={prefetchGenerationPreferencesPanel}
      onFocus={prefetchGenerationPreferencesPanel}
      onClick={() => {
        if (!prefsHydrated) {
          openPreferences(true);
          return;
        }
        openPreferences(!preferencesOpen);
      }}
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
          {mode === 'action'
            ? t('videoGeneration.create.homeHintAction', {
                defaultValue: '上传角色图和参考视频，让角色模仿动作生成成片。',
              })
            : mode === 'generate'
              ? t('videoGeneration.create.homeHintGenerate', {
                  defaultValue: '上传参考图并填写提示词，直接生成视频片段。',
                })
              : mode === 'agent'
                ? t('videoGeneration.create.homeHintAgent', {
                    defaultValue: '写下故事或想法，Flowy 帮你变成成片。',
                  })
                : t('videoGeneration.create.homeHintCreation', {
                    defaultValue: '描述画面与镜头，在无限画布里自由编排生成。',
                  })}
        </p>
      </div>

      <Suspense fallback={null}>
        <CampaignCarousel />
      </Suspense>

      <div
        ref={composerRef}
        className={styles.composer}
        onDragEnter={(event) => {
          event.preventDefault();
          setFileDragOver(true);
        }}
        onDragOver={(event) => event.preventDefault()}
        onDragLeave={(event) => {
          const next = event.relatedTarget as Node | null;
          if (next && composerRef.current?.contains(next)) return;
          setFileDragOver(false);
        }}
        onDrop={(event) => {
          event.preventDefault();
          setFileDragOver(false);
          void handleFiles(filesFromClipboardData(event.dataTransfer));
        }}
        // Capture so pasted images are treated as uploads before the textarea
        // inserts a filename / binary placeholder as text.
        onPasteCapture={(event) => {
          const files = filesFromClipboardData(event.clipboardData);
          if (files.length === 0) return;
          event.preventDefault();
          event.stopPropagation();
          void handleFiles(files);
        }}
      >
        {fileDragOver ? (
          <div className={styles.composerDropOverlay}>
            <span className={styles.composerDropHint}>
              <AttachPlusIcon size={16} />
              {t('videoGeneration.create.upload.dropHint', {
                defaultValue: '松开以上传文件',
              })}
            </span>
          </div>
        ) : null}
        {isAction ? (
          <ActionUploadSlots
            loading={loading ?? false}
            actionCharacter={draft.actionCharacter}
            actionVideo={draft.actionVideo}
            setActionCharacter={setActionCharacter}
            setActionVideo={setActionVideo}
            handleFiles={handleFiles}
          />
        ) : (
          <PromptComposer
            mode={mode}
            loading={loading ?? false}
            documentName={documentName}
            setDocumentName={setDocumentName}
            canvasReferences={draft.canvasReferences}
            removeCanvasReference={removeCanvasReference}
            cameos={draft.cameos}
            removeCameo={removeCameo}
            selectedVerticalSkills={selectedVerticalSkills}
            removeVerticalSkill={removeVerticalSkill}
            activeText={activeText}
            setActiveText={setActiveText}
            placeholder={placeholder}
            handlePromptKeyDown={handlePromptKeyDown}
            onRequestUpload={openFilePicker}
          />
        )}
        {isAction ? null : (
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
        )}

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
              content={
                <ModeMenu mode={mode} onSelect={handleModeSelect} />
              }
            >
              <button type='button' className={`${styles.toolbarButton} ${styles.modeButton}`}>
                {mode === 'generate' ? (
                  <VideoOne size={15} />
                ) : mode === 'agent' ? (
                  <RobotOne size={15} />
                ) : mode === 'creation' ? (
                  <Platte size={15} />
                ) : (
                  <People size={15} />
                )}
                <span>{modeLabel}</span>
                <Down size={12} />
              </button>
            </Popover>
            {prefsModuleReady ? (
              <Suspense fallback={prefsTrigger}>
                <GenerationPreferencesMount onMounted={markPrefsHydrated}>
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
                  workflow={isAction ? 'action2video' : draft.workflow}
                />
                </GenerationPreferencesMount>
              </Suspense>
            ) : (
              prefsTrigger
            )}
            {skillMenu ? (
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
            ) : null}
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
                  <Suspense fallback={<div className={styles.slashMenu} />}>
                    <VerticalSkillMenu
                      selectedIds={draft.verticalSkillIds}
                      initialSkills={skillCatalog}
                      reloadToken={skillListReloadToken}
                      onChangeSelected={(verticalSkillIds) =>
                        setDraft((current) => ({ ...current, verticalSkillIds }))
                      }
                      onCatalogChange={mergeCatalog}
                      onRequestCreate={() => {
                        setSkillHubOpen(false);
                        setSkillCreateOpen(true);
                      }}
                    />
                  </Suspense>
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
                  onMouseEnter={prefetchVerticalSkillMenu}
                  onFocus={prefetchVerticalSkillMenu}
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
            data-video-home-submit=''
            className={styles.submitButton}
            disabled={
              loading ||
              (isAction
                ? !draft.actionCharacter?.file || !draft.actionVideo?.file
                : !activeText.trim())
            }
            onMouseEnter={() => {
              if (mode === 'creation') prefetchCanvasAssistantPanel();
            }}
            onFocus={() => {
              if (mode === 'creation') prefetchCanvasAssistantPanel();
            }}
            onClick={submit}
            aria-label={
              isAction
                ? t('videoGeneration.create.generateActionVideo', {
                    defaultValue: '生成视频',
                  })
                : mode === 'generate'
                  ? t('videoGeneration.create.generateClip', {
                      defaultValue: '生成视频',
                    })
                  : mode === 'agent'
                    ? t('videoGeneration.create.generateStoryboard', {
                        defaultValue: '生成分镜',
                      })
                    : t('videoGeneration.create.enterCanvas', {
                        defaultValue: '发给画布 Agent',
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
              bumpReloadToken();
            }}
          />
        </Suspense>
      ) : null}
    </section>
  );
};

export default VideoHomeComposer;
