import { FlowyIcon } from '@renderer/components/base/FlowyIcon';
import { ipcBridge } from '@/common';
import { configService } from '@/common/config/configService';
import type { IMcpServer } from '@/common/config/storage';
import { parseAgentId, type AgentId, type McpServerId } from '@/common/types/ids';
import {
  MAX_AGENT_EXECUTION_MODELS,
  type TDecisionPolicy,
  type TDelegationPolicy,
  type TExecutionModelPool,
  type TExecutionModelRef,
} from '@/common/types/agentExecution/agentExecutionTypes';
import { resolveLocaleKey } from '@/common/utils';
import { abandonPendingVoiceCallIntent, discardPendingVoiceCallIntent, getVoiceCapabilities, getVoiceCompanionStatus, prepareVoiceSession, retryAbandonedVoiceCallIntent, startVoiceCallIntent, voiceStartNeedsPrepare, reportVoiceCandidate, type VoiceCallTarget } from '@/common/adapter/voiceCompanion';
import { useVoiceSnapshot } from '@/renderer/pages/voiceSession/useVoiceSnapshot';
import { useClawAsrAvailable } from '@/renderer/hooks/system/useClawAsrAvailable';
import type { DictationActivationResult, DictationActivationRequest } from '@/renderer/components/chat/SpeechInputButton';
import { consumeDictationIntent, revokeDictationIntent, subscribeDictationIntent, type DictationIntent } from './dictationIntent';
import { GUID_DRAFT_KEY, useComposerDraftStore } from '@/renderer/stores/composerDraftStore';

import { useInputFocusRing } from '@/renderer/hooks/chat/useInputFocusRing';
import { isSubmitGesture } from '@/renderer/hooks/chat/useCompositionInput';
import { useSlashLauncherController } from '@/renderer/hooks/chat/useSlashLauncherController';
import { useOpenFileSelector } from '@/renderer/hooks/file/useOpenFileSelector';
import { installedPickerSkills, useSkillCatalog } from '@/renderer/hooks/skills/useSkillCatalog';
import { appendSpeechTranscript } from '@/renderer/hooks/system/useSpeechInput';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import { resolveExtensionAssetUrl } from '@/renderer/utils/platform';
import GuidPresetEditorHost from './components/GuidPresetEditorHost';
import GuidActionRow from './components/GuidActionRow';
import GuidInputCard from './components/GuidInputCard';
import SlashCommandMenu, { type SlashCommandMenuItem } from '@/renderer/components/chat/SlashCommandMenu';
import FeatureCards from './components/FeatureCards';
import GuidHomeBrand from './components/GuidHomeBrand';
import ShowcaseCards from './components/ShowcaseCards';
import HomeInputDrawer from './components/HomeInputDrawer';
import BuildAmbientWall, { type BuildAmbientVariant } from './components/BuildAmbientWall';
import HomeVideoComposer from './components/HomeVideoComposer';
import buildWebsiteMain from './components/assets/build/website-main.jpg';
import buildWebsiteExhibition1 from './components/assets/build/website-exhibition-1.jpg';
import buildWebsiteExhibition2 from './components/assets/build/website-exhibition-2.jpg';
import buildAppMain from './components/assets/build/app-main.jpg';
import buildAppSpending from './components/assets/build/app-spending.jpg';
import buildAppHabits from './components/assets/build/app-habits.jpg';
import buildGameMain from './components/assets/build/game-main.jpg';
import buildGameSokoban from './components/assets/build/game-sokoban.jpg';
import buildGameNeon from './components/assets/build/game-neon.jpg';
import showcaseVideoCover from './components/assets/showcase-video-cover.png';
import type { HomeCapability, HomeStageOption } from './components/homeStage';
import SlideOptionsControls from './presentation/SlideOptionsControls';
import SlideCreatePanel from './presentation/SlideCreatePanel';
import SlideCardWall from './presentation/SlideCardWall';
import SlideThemeThumbnail from './presentation/SlideThemeThumbnail';
import type { SlideSuggestion } from './presentation/slideGallery';
import {
  DEFAULT_SLIDE_OPTIONS,
  SLIDE_TEMPLATE_IMPORT_EXTENSIONS,
  applySlideHeader,
  type SlideOptions,
  type SlideTemplate,
} from './presentation/slideOptions';
import { PRESENTATION_SKILL_ID } from '@/common/adapter/ipcBridge';
import { composerChipFromCatalog, type ComposerSkillChip } from '@/renderer/components/chat/composerSkill';
import SkillAvatar from '@/renderer/pages/settings/skill/SkillAvatar';
import { resolveSkillDisplay } from '@/renderer/pages/settings/skill/skillDisplay';
import type {
  ComposerSkillTokenInputHandle,
  ComposerTokenInputState,
} from '@/renderer/components/chat/ComposerSkillTokenInput';
import GuidModelSelector from './components/GuidModelSelector';
import GuidAddProviderModal, { type GuidAddProviderHandle } from './components/GuidAddProviderModal';
import AutoTierSelector from '@/renderer/components/agent/AutoTierSelector';
import { createWorkspaceDialogGate } from './workspaceDialogGate';
import MentionDropdown, { MentionSelectorBadge } from './components/MentionDropdown';
import GuidEntryFlyout, { GuidAnchoredFlyout, type GuidEntryFlyoutMode } from './components/GuidEntryFlyout';
import ReasoningEffortSelector from '@/renderer/components/agent/ReasoningEffortSelector';
import {
  catalogReasoningEffortForModel,
  resolveReasoningEffortForLevels,
} from '@/renderer/utils/model/reasoningEffort';
import {
  readPreferredReasoningEffort,
  savePreferredReasoningEffort,
} from './hooks/agentSelectionUtils';
import { consumeKnowledgeActivation } from '@/renderer/pages/knowledge/knowledgeActivation';
import KnowledgeControl from '@/renderer/pages/conversation/components/KnowledgeControl';
import { useGuidAgentSelection } from './hooks/useGuidAgentSelection';
import { useGuidAdvancedConfig } from './hooks/useGuidAdvancedConfig';
import { hasGuidInitialPayload, isAutoWorkEntry } from './hooks/autoWorkEntry';
import { useGuidInput } from './hooks/useGuidInput';
import { useGuidMention } from './hooks/useGuidMention';
import { useGuidModelSelection } from './hooks/useGuidModelSelection';
import { useGuidSend } from './hooks/useGuidSend';
import { useExecutionModelPool } from '@/renderer/pages/conversation/execution/useExecutionModelPool';
import { reconcileModelRefs, sameModelRefs } from '@/renderer/pages/conversation/execution/executionModelRefs';
import { usePendingConversation } from '@/renderer/pages/conversation/components/ConversationShell/PendingConversationContext';
import { preloadCommercialPathChunks } from '@/renderer/utils/motion/flowyMotion';
import { ensureBackendMcpCatalog, isMcpServerSelectable } from '@/renderer/hooks/mcp/catalog';
import { resolveAgentLogo } from '@/renderer/utils/model/agentLogo';
import { findChatModelOption } from '@/renderer/utils/model/chatModelPicker';
import { addRecentWorkspace } from '@/renderer/components/workspace';
import { trackFunnelEvent, hasFunnelEvent } from '@/renderer/utils/analytics/productFunnel';
import { markLaunchInteractive } from '@/renderer/utils/analytics/launchTelemetry';
import { resolveGuidReadiness } from './readiness/guidReadiness';
import { ConfigProvider } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';
import { mutate as swrMutate } from 'swr';
import type { Preset } from '@/common/types/agent/presetTypes';
import type { TProviderWithModel } from '@/common/config/storage';
import { replaceActiveSlashToken, type SlashLauncherItem } from '@/common/chat/slash/launcher';
import { PRESET_CATALOG_SWR_KEY } from '@/renderer/hooks/preset/presetCatalog';
import {
  createGenerationTask,
  deleteCanvasMedia,
  listGenerationTasks,
  uploadCanvasMedia,
  type GenerationTaskView,
} from '../videoCanvas/api';
import { isBackendHttpError, isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { clampClipDurationForModel, clipDurationBoundsForModel } from '../videoGeneration/durationBounds';
import { pickDefaultVideoModel } from '../videoGeneration/components/modelPreferenceDefaults';
import { refreshMediaModelsCatalog, useMediaModels } from '@/renderer/hooks/agent/useMediaModels';
import { useEffectiveModelTier } from '@renderer/hooks/cloud/useEffectiveModelTier';
import { isPlanLocked } from '@/renderer/utils/model/planGate';
import { formatClipOperationError } from '../videoGeneration/clipFailure';
import { rememberVideoGenerationTask } from '../videoGeneration/routeMemory';
import { composeClipPrompt } from '../videoGeneration/styleCatalog/looks';
import { materializeHomeImageMentions } from '../videoGeneration/home/imageMentions';
import type { CanvasReferenceDraft } from '../videoGeneration/home/types';
import {
  DEFAULT_VIDEO_RESOLUTION,
  normalizeVideoResolution,
  videoModelCapabilities,
  type VideoResolution,
} from '@renderer/services/videoModelCapabilities';
import styles from './index.module.css';

type ActiveChatPopup = 'model' | 'strategy' | null;
type HomeFlyout = GuidEntryFlyoutMode | 'knowledge';

/** A stuck create POST must not pin the homepage send guard forever. */
const HOME_VIDEO_CREATE_TIMEOUT_MS = 60_000;
/** The empty-catalog probe is only a classifier; never let it hang the picker. */
const HOME_VIDEO_CATALOG_PROBE_TIMEOUT_MS = 15_000;
const HOME_VIDEO_CLAIM_PAGE_SIZE = 50;
/** Bound the recovery scan so a huge history cannot turn one check into N requests. */
const HOME_VIDEO_CLAIM_MAX_TASKS = 500;

function withHomeVideoTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message = 'home video operation timed out',
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = window.setTimeout(() => {
      reject(new Error(message));
    }, timeoutMs);
    promise.then(
      (value) => {
        window.clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        window.clearTimeout(timer);
        reject(error);
      },
    );
  });
}

/**
 * A 4xx from the create endpoint is a deterministic rejection: the backend did
 * not accept the task, so this attempt's uploads are safe to clean. 5xx,
 * timeouts, disconnects and parse failures are all ambiguous.
 */
function isDefinitelyNotCreatedCreateError(error: unknown): boolean {
  return isBackendHttpError(error) && error.status >= 400 && error.status < 500;
}

/**
 * Recovery scans the standalone task list in pages so a unique media set is not
 * missed just because newer tasks pushed it past the first page.
 */
async function listHomeVideoClaimCandidates(): Promise<GenerationTaskView[]> {
  const tasks: GenerationTaskView[] = [];
  let offset = 0;
  while (tasks.length < HOME_VIDEO_CLAIM_MAX_TASKS) {
    const page = await listGenerationTasks(HOME_VIDEO_CLAIM_PAGE_SIZE, offset, {
      standalone: true,
    });
    const batch = page.tasks ?? [];
    tasks.push(...batch);
    offset += batch.length;
    if (batch.length === 0 || offset >= (page.total ?? 0)) break;
  }
  return tasks;
}

const asHomeFlyout = (key: string | undefined): HomeFlyout | null =>
  key === 'preset' || key === 'skills' || key === 'knowledge' ? key : null;

type HomeBuildKind = 'website' | 'app' | 'game';

const HOME_BUILD_KINDS: readonly HomeBuildKind[] = ['website', 'app', 'game'];

const HOME_BUILD_COPY = {
  website: {
    title: 'guid.home.buildChoices.website.title',
    description: 'guid.home.buildChoices.website.description',
    firstLabel: 'guid.home.buildPrompts.website.firstLabel',
    secondLabel: 'guid.home.buildPrompts.website.secondLabel',
    first: 'guid.home.buildPrompts.website.first',
    second: 'guid.home.buildPrompts.website.second',
    thirdLabel: 'guid.home.buildPrompts.website.thirdLabel',
    third: 'guid.home.buildPrompts.website.third',
  },
  app: {
    title: 'guid.home.buildChoices.app.title',
    description: 'guid.home.buildChoices.app.description',
    firstLabel: 'guid.home.buildPrompts.app.secondLabel',
    secondLabel: 'guid.home.buildPrompts.app.firstLabel',
    first: 'guid.home.buildPrompts.app.second',
    second: 'guid.home.buildPrompts.app.first',
    thirdLabel: 'guid.home.buildPrompts.app.thirdLabel',
    third: 'guid.home.buildPrompts.app.third',
  },
  game: {
    title: 'guid.home.buildChoices.game.title',
    description: 'guid.home.buildChoices.game.description',
    firstLabel: 'guid.home.buildPrompts.game.firstLabel',
    secondLabel: 'guid.home.buildPrompts.game.secondLabel',
    first: 'guid.home.buildPrompts.game.first',
    second: 'guid.home.buildPrompts.game.second',
    thirdLabel: 'guid.home.buildPrompts.game.thirdLabel',
    third: 'guid.home.buildPrompts.game.third',
  },
} as const;

const BUILD_DRAWER_COLLAPSE_MS = 340;
const BUILD_DRAWER_EXPAND_MS = 420;

type BuildDrawerMotion = 'idle' | 'closing' | 'opening';

type HomeBuildExample = {
  labelKey: string;
  promptKey: string;
  cover: string;
};

const HOME_BUILD_EXAMPLES: Record<
  HomeBuildKind,
  readonly [HomeBuildExample, HomeBuildExample, HomeBuildExample]
> = {
  website: [
    { labelKey: 'guid.home.buildPrompts.website.firstLabel', promptKey: 'guid.home.buildPrompts.website.first', cover: buildWebsiteMain },
    { labelKey: 'guid.home.buildPrompts.website.secondLabel', promptKey: 'guid.home.buildPrompts.website.second', cover: buildWebsiteExhibition1 },
    { labelKey: 'guid.home.buildPrompts.website.thirdLabel', promptKey: 'guid.home.buildPrompts.website.third', cover: buildWebsiteExhibition2 },
  ],
  app: [
    { labelKey: 'guid.home.buildPrompts.app.secondLabel', promptKey: 'guid.home.buildPrompts.app.second', cover: buildAppSpending },
    { labelKey: 'guid.home.buildPrompts.app.firstLabel', promptKey: 'guid.home.buildPrompts.app.first', cover: buildAppMain },
    { labelKey: 'guid.home.buildPrompts.app.thirdLabel', promptKey: 'guid.home.buildPrompts.app.third', cover: buildAppHabits },
  ],
  game: [
    { labelKey: 'guid.home.buildPrompts.game.firstLabel', promptKey: 'guid.home.buildPrompts.game.first', cover: buildGameMain },
    { labelKey: 'guid.home.buildPrompts.game.secondLabel', promptKey: 'guid.home.buildPrompts.game.second', cover: buildGameSokoban },
    { labelKey: 'guid.home.buildPrompts.game.thirdLabel', promptKey: 'guid.home.buildPrompts.game.third', cover: buildGameNeon },
  ],
};

const HOME_BUILD_EXAMPLE_SLOTS = ['first', 'second', 'third'] as const;

const homeBuildKindOfOption = (key: string): HomeBuildKind | null => {
  if (key === 'develop-website') return 'website';
  if (key === 'develop-app') return 'app';
  if (key === 'develop-game') return 'game';
  return null;
};

const GuidPage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const pendingConversation = usePendingConversation();

  // Warm high-probability commercial path chunks while composing.
  useEffect(() => {
    preloadCommercialPathChunks();
  }, []);

  useEffect(() => {
    if (!hasFunnelEvent('home_interactive')) {
      trackFunnelEvent('home_interactive', { source: 'guid' });
    }
    markLaunchInteractive({ source: 'guid' });
    trackFunnelEvent('home_viewed', { feature: 'guid', source: 'guid' });
  }, []);

  const location = useLocation();
  const guidContainerRef = useRef<HTMLDivElement>(null);
  // Ref on the GuidInputCard root for Tauri native drag-drop hit-testing,
  // mirroring the SendBox `.sendbox-panel` containerRef pattern.
  const guidInputCardRef = useRef<HTMLDivElement>(null);
  const addProviderRef = useRef<GuidAddProviderHandle>(null);
  const { activeBorderColor, inactiveBorderColor } = useInputFocusRing();

  const localeKey = resolveLocaleKey(i18n.language);

  const [entryFlyout, setEntryFlyout] = useState<HomeFlyout | null>(null);
  const homeAddMenuPopoverRef = useRef<HTMLDivElement>(null);
  const homeEntryFlyoutRef = useRef<HTMLDivElement>(null);
  const entryFlyoutSuppressedKeyRef = useRef<string | null>(null);
  const entryFlyoutPausedKeyRef = useRef<string | null>(null);
  const delegationPolicy: TDelegationPolicy = 'automatic';
  const decisionPolicy: TDecisionPolicy = 'automatic';
  const [collaborationModels, setCollaborationModels] = useState<TExecutionModelRef[]>(
    () => configService.get('nomi.collaborationModels') ?? [],
  );
  // `/goal` arms the first message as the conversation goal. The state is
  // intentionally command-driven rather than exposed as a persistent toolbar toggle.
  const [goalMode, setGoalMode] = useState(false);
  // "Slides" work mode (Nomi only, home page only): the composer keeps the
  // user's own text; the ppt-master option line and the managed Skill are
  // added when the first message is sent.
  const [slideOptions, setSlideOptions] = useState<SlideOptions | null>(null);
  // "Create presentation" panel above the composer (topics + templates).
  // It opens with slides mode. Dismissing the panel exits slides mode, and
  // leaving slides mode closes the panel.
  const [slidePanelOpen, setSlidePanelOpen] = useState(false);
  const [selectedReasoningEffort, setSelectedReasoningEffort] = useState<string | undefined>(() =>
    readPreferredReasoningEffort()
  );
  const [activeChatPopup, setActiveChatPopup] = useState<ActiveChatPopup>(null);
  const handleChatPopupVisibleChange = useCallback((popup: Exclude<ActiveChatPopup, null>, visible: boolean) => {
    setActiveChatPopup((current) => (visible ? popup : current === popup ? null : current));
  }, []);
  const handleModelPopupVisibleChange = useCallback(
    (visible: boolean) => handleChatPopupVisibleChange('model', visible),
    [handleChatPopupVisibleChange]
  );
  const handleStrategyPopupVisibleChange = useCallback(
    (visible: boolean) => handleChatPopupVisibleChange('strategy', visible),
    [handleChatPopupVisibleChange]
  );
  // Home stage state machine: `home` shows the two entries + membership card;
  // a directory replaces them; build-examples is a drawer layer, not a
  // capability prep page.
  const [homeView, setHomeView] = useState<'home' | 'develop-directory' | 'build-examples' | 'create-directory' | 'capability'>('home');
  // Mirrors data-home-compact so React children (action row) can compact too.
  const [homeCompact, setHomeCompact] = useState(false);
  const [homeCapability, setHomeCapability] = useState<HomeCapability>('office');
  // Drafts are isolated per capability. Text, workspace, attachments and the
  // slides template/options all travel with their own draft, so an attachment
  // imported for slides never leaks into an office send and returning to a
  // capability restores both its body and its reference files.
  const homeDraftsRef = useRef<
    Record<HomeCapability, { text: string; dir: string; files: string[]; slideOptions: SlideOptions | null }>
  >({
    office: { text: '', dir: '', files: [], slideOptions: null },
    develop: { text: '', dir: '', files: [], slideOptions: null },
    slides: { text: '', dir: '', files: [], slideOptions: null },
    video: { text: '', dir: '', files: [], slideOptions: null },
  });
  // Build directory category; drives the prepared prompts and the preview copy.
  const [homeBuildKind, setHomeBuildKind] = useState<HomeBuildKind | null>(null);
  const [selectedBuildExample, setSelectedBuildExample] = useState<string | null>(null);
  const [buildDrawerMotion, setBuildDrawerMotion] = useState<BuildDrawerMotion>('idle');
  const [buildTransitionDirection, setBuildTransitionDirection] = useState<'forward' | 'back' | 'home' | null>(null);
  const [buildDirectoryMotion, setBuildDirectoryMotion] = useState<'enter' | 'expand' | 'exit' | 'idle'>('enter');
  const [pendingBuildKind, setPendingBuildKind] = useState<HomeBuildKind | null>(null);
  // Video capability: local homepage draft parameters. Nothing is uploaded or
  // created until the user presses send. The prompt, model and explicit
  // references live with this draft and survive capability switches.
  const [homeVideoText, setHomeVideoText] = useState('');
  const [homeVideoRefs, setHomeVideoRefs] = useState<CanvasReferenceDraft[]>([]);
  const [homeVideoModel, setHomeVideoModel] = useState('');
  const [homeVideoDuration, setHomeVideoDuration] = useState(6);
  const [homeVideoResolution, setHomeVideoResolution] = useState<VideoResolution>(DEFAULT_VIDEO_RESOLUTION);
  const [homeVideoSubmitting, setHomeVideoSubmitting] = useState(false);
  const effectiveModelTier = useEffectiveModelTier();
  const videoModelCatalog = useMediaModels({ enabled: homeCapability === 'video' });
  /**
   * `useMediaModels` collapses a failed catalog request into an empty list, so an
   * empty picker alone cannot tell "catalog failed, retry" apart from "no model
   * available for this plan". Probe the same endpoint once after an empty load to
   * classify it; the probe only runs in the empty case.
   */
  const [homeVideoCatalogProbe, setHomeVideoCatalogProbe] = useState<
    'idle' | 'checking' | 'ok' | 'failed'
  >('idle');
  const homeVideoCatalogProbeSeqRef = useRef(0);
  // Match the video page contract: never under-grant because one of the two tier
  // sources is stale or missing (profile/seat tier vs catalog effective tier).
  const catalogTier = Math.max(
    effectiveModelTier,
    typeof videoModelCatalog.effectiveModelTier === 'number'
      ? videoModelCatalog.effectiveModelTier
      : 0,
  );
  // No fallback to the raw catalog: a model the current plan cannot call must
  // never become selectable or sendable.
  const homeVideoOpenModels = useMemo(
    () =>
      videoModelCatalog.videoModels.filter(
        (model) => !isPlanLocked(model.min_tier_level ?? 0, catalogTier),
      ),
    [videoModelCatalog.videoModels, catalogTier],
  );
  const homeVideoLockedModelCount =
    videoModelCatalog.videoModels.length - homeVideoOpenModels.length;
  const homeVideoModelOptions = useMemo(
    () => homeVideoOpenModels.map((model) => ({ value: model.id, label: model.name || model.id })),
    [homeVideoOpenModels],
  );
  // The submit closure can run for seconds while uploads are in flight; keep the
  // latest eligibility in a ref so the pre-create re-check sees a catalog refresh.
  const homeVideoEligibilityRef = useRef({ model: homeVideoModel, openModels: homeVideoOpenModels });
  homeVideoEligibilityRef.current = { model: homeVideoModel, openModels: homeVideoOpenModels };
  const homeVideoCatalogLoading =
    homeCapability === 'video' && (videoModelCatalog.isLoading || homeVideoCatalogProbe === 'checking');
  const homeVideoCatalogFailed =
    homeCapability === 'video' &&
    !homeVideoCatalogLoading &&
    videoModelCatalog.videoModels.length === 0 &&
    homeVideoCatalogProbe === 'failed';
  const homeVideoCatalogState: 'loading' | 'error' | 'no-model' | 'ready' = homeVideoCatalogLoading
    ? 'loading'
    : homeVideoCatalogFailed
      ? 'error'
      : homeVideoModelOptions.length === 0
        ? 'no-model'
        : 'ready';
  useEffect(() => {
    if (homeCapability !== 'video') {
      homeVideoCatalogProbeSeqRef.current += 1;
      setHomeVideoCatalogProbe('idle');
      return;
    }
    if (videoModelCatalog.isLoading) return;
    if (videoModelCatalog.videoModels.length > 0) {
      setHomeVideoCatalogProbe('ok');
      return;
    }
    const seq = ++homeVideoCatalogProbeSeqRef.current;
    setHomeVideoCatalogProbe('checking');
    void withHomeVideoTimeout(
      ipcBridge.media.listModels.invoke(),
      HOME_VIDEO_CATALOG_PROBE_TIMEOUT_MS,
      'home video catalog probe timed out',
    )
      .then(() => {
        if (seq === homeVideoCatalogProbeSeqRef.current) setHomeVideoCatalogProbe('ok');
      })
      .catch(() => {
        if (seq === homeVideoCatalogProbeSeqRef.current) setHomeVideoCatalogProbe('failed');
      });
  }, [homeCapability, videoModelCatalog.isLoading, videoModelCatalog.videoModels.length]);
  useEffect(() => {
    if (homeVideoCatalogLoading) return;
    if (homeVideoModelOptions.length === 0) {
      setHomeVideoModel((current) => (current ? '' : current));
      return;
    }
    if (homeVideoModel && homeVideoModelOptions.some((option) => option.value === homeVideoModel)) return;
    const preferred = pickDefaultVideoModel(homeVideoOpenModels);
    setHomeVideoModel(preferred ?? '');
  }, [homeVideoCatalogLoading, homeVideoModel, homeVideoModelOptions, homeVideoOpenModels]);
  // Model switches clamp duration and normalize resolution to that model's own
  // table; submit sends these normalized values.
  useEffect(() => {
    if (!homeVideoModel) return;
    setHomeVideoDuration((current) => {
      const ticks = clipDurationBoundsForModel(homeVideoModel).ticks;
      const clamped = clampClipDurationForModel(homeVideoModel, current);
      if (ticks.includes(clamped)) return clamped;
      return ticks.includes(current) ? current : (ticks[ticks.length - 1] ?? clamped);
    });
    setHomeVideoResolution((current) => {
      const normalized = normalizeVideoResolution(homeVideoModel, current);
      const caps = videoModelCapabilities(homeVideoModel).resolutions;
      return caps.includes(normalized) ? normalized : (caps[caps.length - 1] ?? current);
    });
  }, [homeVideoModel]);
  const retryHomeVideoCatalog = useCallback(() => {
    homeVideoCatalogProbeSeqRef.current += 1;
    setHomeVideoCatalogProbe('idle');
    // revalidate() alone can reuse the 120s module cache; a user-triggered retry
    // must actually re-read the catalog (plan upgrades, tier changes).
    void refreshMediaModelsCatalog().then(() => videoModelCatalog.revalidate());
  }, [videoModelCatalog]);
  // R3: the attempt that owns the in-flight uploads/POST. `postIssued` is the
  // only switch between "definitely not created" and "outcome unknown".
  type HomeVideoAttemptSnapshot = {
    uploadedMediaIds: string[];
    prompt: string;
    title: string;
    model: string;
    resolution: VideoResolution;
    durationSecs: number;
  };
  type HomeVideoRecovery = HomeVideoAttemptSnapshot & { postIssued: true };
  const homeVideoAttemptRef = useRef<(HomeVideoAttemptSnapshot & { postIssued: boolean }) | null>(null);
  const homeVideoSendGuardRef = useRef(false);
  const homeVideoRecoveryCheckingRef = useRef(false);
  const homeVideoOpenGuardRef = useRef(false);
  const [homeVideoRecovery, setHomeVideoRecovery] = useState<HomeVideoRecovery | null>(null);
  const [homeVideoRecoveryChecking, setHomeVideoRecoveryChecking] = useState(false);
  const [homeVideoNavFailure, setHomeVideoNavFailure] = useState<{
    taskId: string;
    title: string;
  } | null>(null);
  // Example cards only replace a visible draft after an explicit confirmation.
  const [pendingExample, setPendingExample] = useState<{ title: string; run: () => void } | null>(null);
  const exampleConfirmCancelRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!pendingExample) return;
    const frame = window.requestAnimationFrame(() => exampleConfirmCancelRef.current?.focus());
    return () => window.cancelAnimationFrame(frame);
  }, [pendingExample]);
  const [isHomeAddMenuOpen, setIsHomeAddMenuOpen] = useState(false);
  const [homeAddMenuActiveIndex, setHomeAddMenuActiveIndex] = useState(0);
  const pendingAutoSendRef = useRef(false);
  const sendRef = useRef<(() => void) | null>(null);
  const inputSnapshotRef = useRef('');
  const workspaceDialogGateRef = useRef(createWorkspaceDialogGate());

  // Explicit Skill selections are scoped to this draft. Preset bindings are
  // resolved separately by the backend when the conversation is created.
  const [homeSkillChips, setHomeSkillChips] = useState<ComposerSkillChip[]>(() => {
    const raw = useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY]?.skillChips;
    if (!Array.isArray(raw)) return [];
    return raw
      .filter((s): s is { skillId: string; name: string; source?: string; avatar?: string } =>
        Boolean(s && typeof s.skillId === 'string' && typeof s.name === 'string')
      )
      .map((s) => ({
        skillId: s.skillId,
        name: s.name,
        source: s.source || '',
        ...(s.avatar ? { avatar: s.avatar } : {}),
      }));
  });

  const homeSkillChipsRef = useRef(homeSkillChips);
  useEffect(() => {
    homeSkillChipsRef.current = homeSkillChips;
    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, {
      skillChips: homeSkillChips.map((s) => ({
        skillId: s.skillId,
        name: s.name,
        source: s.source,
        avatar: s.avatar,
      })),
    });
  }, [homeSkillChips]);
  const homeTokenInputRef = useRef<ComposerSkillTokenInputHandle>(null);
  const [homeTokenInputState, setHomeTokenInputState] = useState<ComposerTokenInputState>({
    projection: '',
    selection: { start: 0, end: 0 },
    textSelection: { start: 0, end: 0 },
  });
  const [availableMcpServers, setAvailableMcpServers] = useState<IMcpServer[]>([]);
  const [guidSelectedMcpServerIds, setGuidSelectedMcpServerIds] = useState<McpServerId[] | undefined>(undefined);

  useEffect(() => {
    void ensureBackendMcpCatalog()
      .then(({ allServers }) => {
        setAvailableMcpServers(allServers.filter(isMcpServerSelectable));
        setGuidSelectedMcpServerIds((prev) => prev ?? []);
      })
      .catch((error) => {
        console.error('[GuidPage] Failed to load MCP catalog:', error);
        setAvailableMcpServers([]);
        setGuidSelectedMcpServerIds((prev) => prev ?? []);
      });
  }, []);

  const handleToggleMcpServer = useCallback((serverId: McpServerId) => {
    setGuidSelectedMcpServerIds((prev) => {
      const current = prev ?? [];
      return current.includes(serverId) ? current.filter((id) => id !== serverId) : [...current, serverId];
    });
  }, []);

  // --- Hooks ---
  // Only nomi uses this provider-based model picker now (Gemini runs as a
  // regular ACP backend with its own model selector).
  const modelSelection = useGuidModelSelection('nomi');
  const { configuredPairs, allPairs, isLoading: isModelCatalogLoading } = useExecutionModelPool();
  const collaboratorReconciliation = useMemo(
    () => (isModelCatalogLoading ? null : reconcileModelRefs(collaborationModels, configuredPairs, allPairs)),
    [allPairs, collaborationModels, configuredPairs, isModelCatalogLoading],
  );
  const activeCollaborators = collaboratorReconciliation?.active ?? [];
  const mainModelRef = useMemo<TExecutionModelRef | null>(
    () =>
      modelSelection.current_model
        ? {
            provider_id: modelSelection.current_model.id,
            model: modelSelection.current_model.use_model,
          }
        : null,
    [modelSelection.current_model?.id, modelSelection.current_model?.use_model],
  );
  const persistCollaborationModels = useCallback((next: TExecutionModelRef[]) => {
    setCollaborationModels(next);
    void configService.set('nomi.collaborationModels', next).catch((error) => {
      console.error('[GuidPage] Failed to save collaboration models:', error);
    });
  }, []);
  useEffect(() => {
    if (!collaboratorReconciliation || collaboratorReconciliation.removed.length === 0) return;
    if (sameModelRefs(collaborationModels, collaboratorReconciliation.retained)) return;
    persistCollaborationModels(collaboratorReconciliation.retained);
  }, [collaborationModels, collaboratorReconciliation, persistCollaborationModels]);
  const executionModelPool = useMemo<TExecutionModelPool | undefined>(() => {
    if (!mainModelRef) return undefined;
    const models = [
      mainModelRef,
      ...activeCollaborators.filter(
        (item) => item.provider_id !== mainModelRef.provider_id || item.model !== mainModelRef.model,
      ),
    ].slice(0, MAX_AGENT_EXECUTION_MODELS);
    return models.length === 1 ? { mode: 'single', model: models[0] } : { mode: 'range', models };
  }, [activeCollaborators, mainModelRef]);

  const navState = location.state as {
    resetPreset?: boolean;
    selectedAgentKey?: string;
  } | null;
  const resetPresetRequested = navState?.resetPreset === true;
  const preselectAgentKey = navState?.selectedAgentKey;
  const agentSelection = useGuidAgentSelection({
    modelList: modelSelection.modelList,
    localeKey,
    resetPreset: resetPresetRequested,
    preselectAgentKey,
    locationKey: location.key,
  });

  const handleStalePreset = useCallback(() => {
    const fallbackAgent = agentSelection.availableAgents?.find((agent) => !agent.is_preset);
    const fallbackKey = fallbackAgent ? agentSelection.getAgentKey(fallbackAgent) : 'nomi';
    agentSelection.setSelectedAgentKey(fallbackKey);
    void swrMutate(PRESET_CATALOG_SWR_KEY);
  }, [agentSelection.availableAgents, agentSelection.getAgentKey, agentSelection.setSelectedAgentKey]);

  // Only the Nomi provider-based agent owns the FlowY model catalog. ACP
  // agents keep their own model surface and must not inherit Auto controls.
  const effectiveAgentType = agentSelection.is_presetAgent
    ? agentSelection.currentEffectiveAgentInfo.agent_type
    : agentSelection.selectedAgent;
  const isNomiAgent = effectiveAgentType === 'nomi';
  const isGeminiMode =
    isNomiAgent &&
    (!agentSelection.is_presetAgent || agentSelection.currentEffectiveAgentInfo.isAvailable);

  const guidInput = useGuidInput({
    locationState: location.state as { workspace?: string } | null,
    containerRef: guidInputCardRef,
  });
  const focusHomeComposer = useCallback(() => {
    window.requestAnimationFrame(() => homeTokenInputRef.current?.focus());
  }, []);
  const switchHomeCapability = useCallback(
    (next: HomeCapability) => {
      if (next === homeCapability) return;
      setPendingExample(null);
      const leaving = homeDraftsRef.current[homeCapability];
      homeDraftsRef.current[homeCapability] = {
        text: guidInput.input,
        dir: guidInput.dir,
        files: guidInput.files,
        slideOptions: homeCapability === 'slides' ? slideOptions : leaving.slideOptions,
      };
      const draft = homeDraftsRef.current[next];
      guidInput.setInput(draft.text);
      guidInput.setDir(draft.dir);
      guidInput.setFiles(draft.files);
      // Only slides owns template options; every other capability must not show
      // or send a template imported for slides.
      setSlideOptions(next === 'slides' ? (draft.slideOptions ?? DEFAULT_SLIDE_OPTIONS) : null);
      if (next !== 'slides') setSlidePanelOpen(false);
      setHomeCapability(next);
      focusHomeComposer();
    },
    [
      homeCapability,
      guidInput.input,
      guidInput.dir,
      guidInput.files,
      guidInput.setInput,
      guidInput.setDir,
      guidInput.setFiles,
      slideOptions,
      focusHomeComposer,
    ],
  );
  const openHomeDirectory = useCallback((group: 'develop' | 'create') => {
    setBuildDrawerMotion('idle');
    setBuildTransitionDirection(null);
    setBuildDirectoryMotion('enter');
    setPendingBuildKind(null);
    setHomeView(group === 'develop' ? 'develop-directory' : 'create-directory');
  }, []);
  const closeHomePanel = useCallback(() => {
    if (
      homeView === 'develop-directory' &&
      buildDrawerMotion === 'idle' &&
      !window.matchMedia('(prefers-reduced-motion: reduce)').matches
    ) {
      setBuildDirectoryMotion('exit');
      setBuildTransitionDirection('home');
      setBuildDrawerMotion('closing');
      return;
    }
    setBuildDrawerMotion('idle');
    setBuildTransitionDirection(null);
    setBuildDirectoryMotion('idle');
    setPendingBuildKind(null);
    setHomeView('home');
    if (homeCapability !== 'office') switchHomeCapability('office');
    focusHomeComposer();
  }, [buildDrawerMotion, focusHomeComposer, homeCapability, homeView, switchHomeCapability]);
  /** A capability draft counts as occupied when it has text, attachments or an imported template. */
  const capabilityHasDraft = useCallback(
    (capability: HomeCapability) => {
      if (capability === 'video') return homeVideoText.trim().length > 0 || homeVideoRefs.length > 0;
      if (capability === homeCapability) {
        return Boolean(
          guidInput.input.trim() || guidInput.files.length > 0 || slideOptions?.template.kind === 'import',
        );
      }
      const draft = homeDraftsRef.current[capability];
      return Boolean(draft.text.trim() || draft.files.length > 0 || draft.slideOptions?.template.kind === 'import');
    },
    [guidInput.input, guidInput.files, homeCapability, homeVideoText, homeVideoRefs, slideOptions],
  );
  /** The single example-application path shared by covers and the dev prep starter. */
  const applyHomeExample = useCallback(
    (kind: 'app' | 'image' | 'video') => {
      if (kind === 'app') {
        setHomeBuildKind('app');
        switchHomeCapability('develop');
        guidInput.setInput(t('guid.home.showcaseAppPrompt'));
        setHomeView('capability');
      } else if (kind === 'image') {
        switchHomeCapability('office');
        guidInput.setInput(t('guid.home.showcaseImagePrompt'));
        setHomeView('home');
      } else {
        switchHomeCapability('video');
        // The confirmed video example must replace the old body; it used to keep it.
        setHomeVideoText(t('guid.home.showcaseVideoTitle'));
        setHomeView('capability');
      }
      setPendingExample(null);
      window.requestAnimationFrame(() => homeTokenInputRef.current?.focus());
    },
    [guidInput.setInput, switchHomeCapability, t],
  );
  const requestHomeExample = useCallback(
    (kind: 'app' | 'image' | 'video', title: string) => {
      const target: HomeCapability = kind === 'app' ? 'develop' : kind === 'image' ? 'office' : 'video';
      if (capabilityHasDraft(target)) {
        setPendingExample({ title, run: () => applyHomeExample(kind) });
        return;
      }
      applyHomeExample(kind);
    },
    [applyHomeExample, capabilityHasDraft],
  );
  const applyBuildPrompt = useCallback(
    (kind: HomeBuildKind, promptKey: string) => {
      // Build examples are exploratory: overwrite the current build draft
      // immediately and keep the picker open so another example can be tried.
      setHomeBuildKind(kind);
      switchHomeCapability('develop');
      guidInput.setInput(t(promptKey));
      setPendingExample(null);
      window.requestAnimationFrame(() => homeTokenInputRef.current?.focus());
    },
    [guidInput.setInput, switchHomeCapability, t],
  );
  const selectBuildExample = useCallback(
    (option: HomeStageOption) => {
      const kind = homeBuildKind ?? 'website';
      const promptKey = option.promptKey;
      if (!promptKey) return;
      setSelectedBuildExample(option.key);
      applyBuildPrompt(kind, promptKey);
    },
    [applyBuildPrompt, homeBuildKind],
  );
  const showBuildDirectory = useCallback((animated: boolean) => {
    setBuildDirectoryMotion(animated ? 'expand' : 'idle');
    setBuildDrawerMotion(animated ? 'opening' : 'idle');
    setBuildTransitionDirection(null);
    setPendingBuildKind(null);
    setHomeView('develop-directory');
    setSelectedBuildExample(null);
    setPendingExample(null);
  }, []);
  const backFromBuildExamples = useCallback(() => {
    // Let X interrupt the opening animation instead of ignoring the click.
    if (buildDrawerMotion === 'opening') {
      setBuildTransitionDirection('back');
      setBuildDirectoryMotion('expand');
      setHomeView('develop-directory');
      setSelectedBuildExample(null);
      setPendingExample(null);
      setPendingBuildKind(null);
      return;
    }
    if (buildDrawerMotion !== 'idle') return;
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      showBuildDirectory(false);
      return;
    }
    setPendingBuildKind(null);
    setBuildTransitionDirection('back');
    setBuildDrawerMotion('closing');
  }, [buildDrawerMotion, showBuildDirectory]);
  const dismissPendingExample = useCallback(() => {
    setPendingExample(null);
    focusHomeComposer();
  }, [focusHomeComposer]);
  const handleSelectShowcaseApp = useCallback(
    () => requestHomeExample('app', t('guid.home.showcaseAppTitle')),
    [requestHomeExample, t],
  );
  const handleSelectShowcaseVideo = useCallback(
    () => requestHomeExample('video', t('guid.home.showcaseVideoTitle')),
    [requestHomeExample, t],
  );
  const handleSelectShowcaseImage = useCallback(
    () => requestHomeExample('image', t('guid.home.showcaseImageTitle')),
    [requestHomeExample, t],
  );
  const handleAddVideoRefs = useCallback((files: File[]) => {
    if (files.length === 0) return;
    setHomeVideoRefs((current) => {
      const room = Math.max(0, 8 - current.length);
      const added: CanvasReferenceDraft[] = files.slice(0, room).map((file, index) => ({
        localId: `home_ref_${Date.now().toString(36)}_${current.length + index}_${Math.random()
          .toString(36)
          .slice(2, 7)}`,
        file,
        previewUrl: URL.createObjectURL(file),
      }));
      return [...current, ...added];
    });
  }, []);
  const handleRemoveVideoRef = useCallback((localId: string) => {
    setHomeVideoRefs((current) => {
      const target = current.find((item) => item.localId === localId);
      if (target) URL.revokeObjectURL(target.previewUrl);
      return current.filter((item) => item.localId !== localId);
    });
  }, []);
  useEffect(() => {
    const report = () => { void reportVoiceCandidate(null, guidInput.dir?.trim() || null).catch(() => {}); };
    report();
    window.addEventListener('focus', report);
    return () => window.removeEventListener('focus', report);
  }, [guidInput.dir]);

  // --- Voice Call entry state & handler ---
  const { snapshot: voiceSnapshot } = useVoiceSnapshot();
  const asrAvailability = useClawAsrAvailable();
  const allowedDictationIntentId = (location.state as { dictationIntentId?: string } | null)?.dictationIntentId;
  const [dictationIntent, setDictationIntent] = useState<DictationIntent | null>(null);
  const dictationIntentRef = useRef<DictationIntent | null>(null);
  const claimDictationIntent = useCallback((id: string) => {
    const claimed = consumeDictationIntent('/', id);
    if (claimed && dictationIntentRef.current?.id === id) {
      dictationIntentRef.current = null;
      setDictationIntent(null);
    }
    return claimed;
  }, []);
  const finishDictationIntent = useCallback((id: string, result: DictationActivationResult) => {
    consumeDictationIntent('/', id);
    dictationIntentRef.current = null;
    setDictationIntent(null);
    if (result === 'unavailable') {
      Message.warning(t('voice.capabilities.textReplyUnavailable'));
    } else if (result === 'stale') {
      Message.warning(t('voice.capabilities.textReplyExpired'));
    } else if (result === 'already-recording') {
      Message.info(t('voice.capabilities.textReplyAlreadyRecording'));
    }
  }, [t]);
  useEffect(() => {
    const unsubscribe = subscribeDictationIntent((intent) => {
      const next = intent?.route === '/' && intent.id === allowedDictationIntentId ? intent : null;
      dictationIntentRef.current = next;
      setDictationIntent(next);
    });
    return () => {
      unsubscribe();
      const pending = dictationIntentRef.current;
      if (pending) revokeDictationIntent(pending.id);
      dictationIntentRef.current = null;
    };
  }, [allowedDictationIntentId]);
  useEffect(() => {
    const pending = dictationIntentRef.current;
    if (pending && asrAvailability.ready && !asrAvailability.available) {
      finishDictationIntent(pending.id, 'unavailable');
    }
  }, [asrAvailability.available, asrAvailability.ready, finishDictationIntent]);
  const dictationActivation = useMemo<DictationActivationRequest | undefined>(
    () => dictationIntent
      ? { id: dictationIntent.id, claim: () => claimDictationIntent(dictationIntent.id), onResult: (result) => finishDictationIntent(dictationIntent.id, result) }
      : undefined,
    [dictationIntent, claimDictationIntent, finishDictationIntent],
  );
  const isVoiceActive =
    Boolean(voiceSnapshot?.session_id) && !voiceSnapshot?.ended_reason && voiceSnapshot?.phase !== 'idle';
  const [isVoiceConnecting, setIsVoiceConnecting] = useState(false);
  const [voicePrerequisite, setVoicePrerequisite] = useState<'loading' | 'ready' | 'setup' | 'failed' | 'unavailable'>('loading');
  const [voiceFailureReason, setVoiceFailureReason] = useState<string | null>(null);
  const inFlightVoiceCallRef = useRef(false);
  const isMountedRef = useRef(true);
  useEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
      if (inFlightVoiceCallRef.current) void abandonPendingVoiceCallIntent();
    };
  }, []);
  const refreshVoicePrerequisite = useCallback(async () => {
    try {
      const [status, report] = await Promise.all([getVoiceCompanionStatus(), getVoiceCapabilities()]);
      setVoiceFailureReason(report?.reason_code ?? null);
      if (!status.models_ready || !status.autostart) {
        setVoicePrerequisite('unavailable');
      } else if (report?.can_start_full_call) {
        setVoicePrerequisite('ready');
      } else if (!report || report.reason_code === 'self_check_required') {
        setVoicePrerequisite('setup');
      } else {
        setVoicePrerequisite('failed');
      }
    } catch {
      setVoicePrerequisite('unavailable');
    }
  }, []);
  useEffect(() => {
    void refreshVoicePrerequisite();
    const onFocus = () => { void refreshVoicePrerequisite(); };
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [refreshVoicePrerequisite]);

  const handleStartVoiceCall = useCallback(async () => {
    if (inFlightVoiceCallRef.current) return;
    if (isVoiceConnecting) return;
    if (isVoiceActive && voiceSnapshot?.binding?.conversation_id) {
      await navigate(`/conversation/${encodeURIComponent(voiceSnapshot.binding.conversation_id)}`);
      return;
    }

    // Own the whole async preparation/start lifecycle, not only the final fetch.
    inFlightVoiceCallRef.current = true;
    setIsVoiceConnecting(true);
    const releaseGate = () => {
      inFlightVoiceCallRef.current = false;
      if (isMountedRef.current) setIsVoiceConnecting(false);
    };

    if (voicePrerequisite === 'loading') {
      Message.info(t('voice.entry.checking'));
      releaseGate();
      return;
    }
    if (voicePrerequisite === 'unavailable') {
      Message.warning(t('voice.entry.setupRequired'));
      releaseGate();
      await navigate('/settings/voice');
      return;
    }
    if (voicePrerequisite === 'failed') {
      const reason = voiceFailureReason ? t(`voice.capabilities.reason.${voiceFailureReason}`, { defaultValue: voiceFailureReason }) : '';
      Message.warning(`${t('voice.capabilities.notReady')}${reason ? `: ${reason}` : ''}`);
      releaseGate();
      await navigate('/settings/voice');
      return;
    }
    let prerequisiteReady = voicePrerequisite === 'ready';
    if (!prerequisiteReady) {
      try {
        const report = await prepareVoiceSession(true, localeKey, 'preparing');
        if (!isMountedRef.current) {
          releaseGate();
          return;
        }
        prerequisiteReady = report.can_start_full_call;
        setVoiceFailureReason(report.reason_code ?? null);
        setVoicePrerequisite(prerequisiteReady ? 'ready' : 'failed');
      } catch (error) {
        if (!isMountedRef.current) {
          releaseGate();
          return;
        }
        const failure = error as { code?: string };
        Message.warning(
          failure.code === 'models_not_installed'
            ? t('voice.errors.modelMissing')
            : t('voice.capabilities.notReady'),
        );
        releaseGate();
        await navigate('/settings/voice');
        return;
      }
      if (!prerequisiteReady) {
        releaseGate();
        await navigate('/settings/voice');
        return;
      }
    }

    const target: VoiceCallTarget = {
      kind: 'new_voice_conversation',
      workspace: guidInput.dir?.trim() || null,
      model: isNomiAgent ? mainModelRef : undefined,
      preset_id: isNomiAgent && agentSelection.is_presetAgent ? agentSelection.selectedAgentInfo?.preset_id : undefined,
    };
    const sessionMode = isNomiAgent
      ? (agentSelection.selectedMode as 'default' | 'auto_edit' | 'yolo')
      : undefined;
    try {
      const currentSkills = homeSkillChipsRef.current;
      useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, {
        text: guidInput.input,
        files: guidInput.files,
        dir: guidInput.dir,
        skillChips: currentSkills.map((s) => ({
          skillId: s.skillId,
          name: s.name,
          source: s.source,
          avatar: s.avatar,
        })),
      });

      const res = await startVoiceCallIntent(target, sessionMode, localeKey);
      if (!isMountedRef.current) return;
      await navigate(`/conversation/${encodeURIComponent(res.conversation_id)}`);
    } catch (err: unknown) {
      if (!isMountedRef.current) return;
      const errorObj = err as { status?: number; code?: string; active_conversation_id?: string; reason_code?: string; requires_check?: boolean; degraded_text_only?: boolean; call_request_id?: string } | undefined;
      if (errorObj?.code === 'voice_call_superseded') return;
      if (errorObj?.code === 'voice_call_cancel_unknown') {
        const recovered = await retryAbandonedVoiceCallIntent();
        if (!isMountedRef.current) return;
        Message.warning(t(recovered ? 'voice.entry.cancelRecovered' : 'voice.entry.cancelUnknown'));
        return;
      }
      const needsPrepare = voiceStartNeedsPrepare(errorObj);
      if (errorObj?.code === 'voice_call_starting') {
        Message.warning(t('voice.entry.callConflictStarting', { defaultValue: '上一通通话正在清理释放，请稍候重试' }));
        return;
      } else if (errorObj?.code === 'voice_call_conflict') {
        Message.info(t('voice.entry.callConflictActive', { defaultValue: '当前已有正在进行的语音通话，请先挂断' }));
        return;
      } else if (needsPrepare) {
        try {
          const report = await prepareVoiceSession(true, localeKey, 'preparing');
          if (!isMountedRef.current) return;
          if (!report.can_start_full_call) {
            discardPendingVoiceCallIntent(errorObj?.call_request_id);
            const reason = report.reason_code
              ? t(`voice.capabilities.reason.${report.reason_code}`, { defaultValue: errorObj?.reason_code || report.reason_code })
              : (errorObj?.reason_code ? t(`voice.capabilities.reason.${errorObj.reason_code}`, { defaultValue: errorObj.reason_code }) : '');
            Message.warning(
              `${t('voice.capabilities.notReady')}${reason ? `: ${reason}` : ''}${report.degraded_text_only ? ` ${t('voice.capabilities.textOnly')}` : ''}`
            );
            return;
          }
          const retry = await startVoiceCallIntent(target, sessionMode, localeKey);
          if (!isMountedRef.current) return;
          await navigate(`/conversation/${encodeURIComponent(retry.conversation_id)}`);
        } catch (prepareError) {
          if (!isMountedRef.current) return;
          const failure = prepareError as { code?: string; message?: string };
          if (failure.code === 'voice_call_superseded') return;
          discardPendingVoiceCallIntent(errorObj?.call_request_id);
          Message.error(
            failure.code === 'models_not_installed'
              ? t('voice.errors.modelMissing')
              : failure.message || t('voice.capabilities.checkFailed'),
          );
        }
      } else if (errorObj?.code === 'voice_suite_not_ready') {
        const reason = errorObj.reason_code ? t(`voice.capabilities.reason.${errorObj.reason_code}`, { defaultValue: errorObj.reason_code }) : '';
        Message.warning(`${t('voice.capabilities.notReady')}${reason ? `: ${reason}` : ''}`);
      } else {
        Message.error(t('voice.session.startFailed', { defaultValue: '启动语音通话失败' }));
      }
    } finally {
      inFlightVoiceCallRef.current = false;
      if (isMountedRef.current) {
        setIsVoiceConnecting(false);
      }
    }
  }, [isVoiceConnecting, isVoiceActive, voiceSnapshot, voicePrerequisite, voiceFailureReason, guidInput.input, guidInput.files, guidInput.dir, homeSkillChips, isNomiAgent, mainModelRef, agentSelection.is_presetAgent, agentSelection.selectedAgentInfo?.preset_id, agentSelection.selectedMode, navigate, t, localeKey]);
  const { openFileSelector: openHomeFileSelector } = useOpenFileSelector({
    onFilesSelected: guidInput.handleFilesUploaded,
  });
  const selectedChatModelOption = useMemo(
    () =>
      findChatModelOption(
        modelSelection.modelPicker,
        modelSelection.current_model?.id,
        modelSelection.current_model?.use_model
      ),
    [modelSelection.current_model?.id, modelSelection.current_model?.use_model, modelSelection.modelPicker]
  );
  const chatModelContextKey = `${modelSelection.current_model?.id ?? ''}\0${modelSelection.current_model?.use_model ?? ''}`;
  const previousChatModelContextKeyRef = useRef(chatModelContextKey);

  const reasoningEffortResolution = useMemo(() => {
    if (!isGeminiMode || !modelSelection.current_model?.use_model) {
      return { levels: [], effort: undefined };
    }
    const provider =
      modelSelection.modelList.find((entry) => entry.id === modelSelection.current_model?.id) ??
      modelSelection.current_model;
    return resolveReasoningEffortForLevels(
      catalogReasoningEffortForModel(provider, modelSelection.current_model.use_model),
      selectedReasoningEffort,
    );
  }, [
    isGeminiMode,
    modelSelection.current_model,
    modelSelection.modelList,
    selectedReasoningEffort,
  ]);
  const reasoningEffortLevels = reasoningEffortResolution.levels;
  const effectiveReasoningEffort = reasoningEffortResolution.effort;

  useEffect(() => {
    if (modelSelection.isModelCatalogLoading || !modelSelection.current_model?.use_model || !selectedChatModelOption) {
      return;
    }
    if (selectedReasoningEffort === effectiveReasoningEffort) return;
    setSelectedReasoningEffort(effectiveReasoningEffort);
    if (effectiveReasoningEffort) {
      void savePreferredReasoningEffort(effectiveReasoningEffort);
    }
  }, [
    effectiveReasoningEffort,
    modelSelection.current_model?.use_model,
    modelSelection.isModelCatalogLoading,
    selectedChatModelOption,
    selectedReasoningEffort,
  ]);

  const supportsHomeSkillLoading = ['nomi', 'acp'].includes(
    agentSelection.currentEffectiveAgentInfo.agent_type,
  );
  const supportsHomeGoalCommand = agentSelection.currentEffectiveAgentInfo.agent_type === 'nomi';

  // Compact stage check: content width <= 560 (i.e. stage area <= 608) or
  // available height <= 580. Measured on the real container so the 880x600
  // desktop window and the browser viewport share one rule.
  useEffect(() => {
    const el = guidContainerRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const apply = () => {
      const compact = el.clientWidth - 48 <= 560 || el.clientHeight <= 580;
      setHomeCompact(compact);
      if (compact) el.setAttribute('data-home-compact', 'true');
      else el.removeAttribute('data-home-compact');
    };
    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // ESC only changes stage display/focus; it never rewrites the draft.

  // Home directory entries and the per-capability input placeholder. Copy reuses
  // existing bilingual keys only (i18n files stay read-only in this task).
  const homeVideoDurationOptions = useMemo(
    () => clipDurationBoundsForModel(homeVideoModel).ticks,
    [homeVideoModel],
  );
  const homeVideoResolutionOptions = useMemo(
    () => videoModelCapabilities(homeVideoModel).resolutions,
    [homeVideoModel],
  );
  const homeDevelopOptions = useMemo<HomeStageOption[]>(
    () =>
      HOME_BUILD_KINDS.map((kind) => ({
        key: `develop-${kind}`,
        id: 'develop',
        title: t(HOME_BUILD_COPY[kind].title),
        description: t(HOME_BUILD_COPY[kind].description),
        cover: HOME_BUILD_EXAMPLES[kind][0].cover,
        coverAlt: t(HOME_BUILD_COPY[kind].title),
      })),
    [t],
  );
  const homeBuildExampleOptions = useMemo<HomeStageOption[]>(() => {
    const kind: HomeBuildKind = homeBuildKind ?? 'website';
    return HOME_BUILD_EXAMPLES[kind].map((example, index) => {
      const slot = HOME_BUILD_EXAMPLE_SLOTS[index];
      const key = `build-example-${kind}-${slot}`;
      return {
        key,
        id: 'develop',
        title: t(example.labelKey),
        cover: example.cover,
        coverAlt: t(example.labelKey),
        promptKey: example.promptKey,
        selected: selectedBuildExample === key,
      };
    });
  }, [homeBuildKind, selectedBuildExample, t]);
  const homeCreateOptions = useMemo<HomeStageOption[]>(
    () => [
      {
        key: 'create-slides',
        id: 'slides',
        title: t('presentation.home.label'),
        description: t('presentation.create.templateTitle'),
        preview: <SlideThemeThumbnail id='swiss' className={styles.dirThumbImage} />,
        disabled: !supportsHomeGoalCommand,
      },
      {
        key: 'create-video',
        id: 'video',
        title: t('videoGeneration.create.preferences.video'),
        description: t('guid.home.videoShortcutSubtitle'),
        cover: showcaseVideoCover,
        coverAlt: t('guid.home.showcaseVideoTitle'),
      },
    ],
    [supportsHomeGoalCommand, t],
  );
  const composerMode: 'office' | 'coding' | 'slides' =
    homeCapability === 'slides' ? 'slides' : homeCapability === 'develop' ? 'coding' : 'office';
  const composerProfile: 'office' | 'coding' = composerMode === 'coding' ? 'coding' : 'office';
  const handleHomeTaskProfileChange = useCallback(
    (profile: 'office' | 'coding') => {
      if (profile === 'coding') {
        if (homeCapability !== 'develop') {
          switchHomeCapability('develop');
          setHomeView('home');
        }
        return;
      }
      if (homeCapability !== 'office') {
        switchHomeCapability('office');
        setHomeView('home');
      }
    },
    [homeCapability, switchHomeCapability],
  );
  const homePlaceholder =
    homeCapability === 'slides'
      ? t('presentation.home.placeholder')
      : homeCapability === 'video'
        ? t('videoGeneration.create.composer.generatePlaceholder')
        : t('conversation.welcome.placeholder');

  const { skills: catalogSkills, loading: catalogSkillsLoading, error: catalogSkillsError } = useSkillCatalog(supportsHomeSkillLoading);
  const pickerSkills = useMemo(
    () => installedPickerSkills(catalogSkills),
    [catalogSkills],
  );
  const handleToggleHomeSkill = useCallback(
    (skillId: string) => {
      const skill = pickerSkills.find((candidate) => candidate.skillId === skillId);
      if (!skill) return;
      const chip = composerChipFromCatalog(
        skill,
        t(`conversation.skills.sources.${skill.source}`, { defaultValue: skill.source }),
        localeKey,
      );
      setHomeSkillChips((current) => {
        if (current.some((candidate) => candidate.skillId === skillId)) {
          return current.filter((candidate) => candidate.skillId !== skillId);
        }
        return [...current, chip];
      });
    },
    [pickerSkills, localeKey, t],
  );
  const enterSlideMode = useCallback(() => {
    setSlideOptions((current) => current ?? DEFAULT_SLIDE_OPTIONS);
    setSlidePanelOpen(true);
    if (homeCapability !== 'slides') switchHomeCapability('slides');
    setHomeView('capability');
    window.requestAnimationFrame(() => homeTokenInputRef.current?.focus());
  }, [homeCapability, switchHomeCapability]);
  const exitSlideMode = useCallback(() => {
    setSlideOptions(null);
    setSlidePanelOpen(false);
    if (homeCapability === 'slides') {
      switchHomeCapability('office');
      setHomeView('home');
    }
  }, [homeCapability, switchHomeCapability]);
  const handleSlidesChange = useCallback(
    (active: boolean) => (active ? enterSlideMode() : exitSlideMode()),
    [enterSlideMode, exitSlideMode],
  );
  useEffect(() => {
    if (!supportsHomeGoalCommand) {
      setGoalMode(false);
      setSlideOptions(null);
    }
  }, [supportsHomeGoalCommand]);
  useEffect(() => {
    if (slideOptions === null) setSlidePanelOpen(false);
  }, [slideOptions]);
  // Entering slides restores the slide-card wall independently of whether the
  // template panel is expanded; closing the panel keeps the wall and the mode.
  const slideStudioOpen = supportsHomeGoalCommand && homeCapability === 'slides';
  const buildAmbientVariant: BuildAmbientVariant | null =
    homeView === 'develop-directory'
      ? 'directory'
      : homeView === 'build-examples'
        ? (homeBuildKind ?? 'website')
        : null;
  // ESC: native selectors, dropdown menus and mention popovers keep priority;
  // then the template panel, then the capability layer, then the directory.
  useEffect(() => {
    const hasOpenOverlay = () => {
      const selectors = [
        '.arco-select-popup',
        '.arco-dropdown',
        '.arco-trigger-popup',
        '[class*="mentionMenu"]',
        '[class*="mention-menu"]',
        '[data-escape-owner]',
      ];
      return selectors.some((selector) =>
        Array.from(document.querySelectorAll<HTMLElement>(selector)).some((element) => {
          if (element.hidden) return false;
          const rect = element.getBoundingClientRect();
          if (rect.width === 0 || rect.height === 0) return false;
          const style = window.getComputedStyle(element);
          return style.visibility !== 'hidden' && style.display !== 'none';
        }),
      );
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      // A preceding handler (dropdown/popover) already consumed this Escape.
      if (event.defaultPrevented) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest?.('[role="listbox"],[role="menu"],[data-escape-owner]')) return;
      if (hasOpenOverlay()) return;
      if (pendingExample) {
        event.preventDefault();
        dismissPendingExample();
        return;
      }
      if (homeView === 'home') return;
      event.preventDefault();
      if (homeCapability === 'slides' && slidePanelOpen) {
        setSlidePanelOpen(false);
        return;
      }
      if (homeCapability === 'slides') {
        exitSlideMode();
        return;
      }
      if (homeCapability !== 'office') switchHomeCapability('office');
      setHomeView('home');
      focusHomeComposer();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [
    homeView,
    homeCapability,
    slidePanelOpen,
    pendingExample,
    dismissPendingExample,
    exitSlideMode,
    switchHomeCapability,
    focusHomeComposer,
  ]);
  // An imported template is the attached reference file: removing the chip (or
  // a draft reset) drops the template choice too.
  const importedTemplatePath = slideOptions?.template.kind === 'import' ? slideOptions.template.path : null;
  useEffect(() => {
    if (importedTemplatePath !== null && !guidInput.files.includes(importedTemplatePath)) {
      setSlideOptions((current) => (current ? { ...current, template: DEFAULT_SLIDE_OPTIONS.template } : current));
    }
  }, [guidInput.files, importedTemplatePath]);
  const handleSelectSlideTemplate = useCallback(
    (template: SlideTemplate) => {
      // Leaving an imported template detaches the reference file it added.
      if (importedTemplatePath !== null && (template.kind !== 'import' || template.path !== importedTemplatePath)) {
        guidInput.handleRemoveFile(importedTemplatePath);
      }
      setSlideOptions((current) => ({ ...(current ?? DEFAULT_SLIDE_OPTIONS), template }));
    },
    [guidInput.handleRemoveFile, importedTemplatePath],
  );
  const handleSelectSlideSuggestion = useCallback(
    (suggestion: SlideSuggestion) => {
      guidInput.setInput(t(`presentation.create.suggestions.${suggestion.id}.prompt`));
      // A topic brings its matching theme only while no template was chosen.
      if (slideOptions?.template.kind === 'free') {
        setSlideOptions({ ...slideOptions, template: { kind: 'theme', id: suggestion.theme } });
      }
      window.requestAnimationFrame(() => homeTokenInputRef.current?.focus());
    },
    [guidInput.setInput, slideOptions, t],
  );
  const handleImportSlideTemplate = useCallback(() => {
    void ipcBridge.dialog.showOpen
      .invoke({
        properties: ['openFile'],
        filters: [{ name: t('presentation.create.importFilter'), extensions: [...SLIDE_TEMPLATE_IMPORT_EXTENSIONS] }],
      })
      .then((paths) => {
        const path = paths?.[0];
        if (!path) return;
        // Same admission as any attachment (image limit); a rejected file
        // never shows up in `files`, so the effect above reverts the choice.
        guidInput.handleFilesUploaded([path]);
        handleSelectSlideTemplate({ kind: 'import', path });
      })
      .catch((error) => {
        console.warn('[GuidPage] Failed to open template file selector:', error);
      });
  }, [guidInput.handleFilesUploaded, handleSelectSlideTemplate, t]);
  // The managed ppt-master is in the catalog only while it is installed; before
  // that the option line tells Nomi to check and install it first.
  const managedSlidesSkillReady = pickerSkills.some((skill) => skill.skillId === PRESENTATION_SKILL_ID);
  const homeLauncherItems = useMemo<SlashLauncherItem[]>(
    () =>
      [
        {
          id: 'system:open',
          kind: 'system' as const,
          name: 'open',
          description: t('conversation.workspace.addFile', { defaultValue: 'Add File' }),
        },
        ...(supportsHomeGoalCommand
          ? [{
              id: 'system:goal',
              kind: 'system' as const,
              name: 'goal',
              description: t('guid.goalMode.tooltip'),
            }, {
              id: 'system:ppt',
              kind: 'system' as const,
              name: 'ppt',
              description: t('presentation.home.slashDescription'),
            }]
          : []),
        ...(supportsHomeSkillLoading
          ? pickerSkills.map((skill) => {
              const display = resolveSkillDisplay(
                {
                  name: skill.name,
                  description: skill.description,
                  name_i18n: skill.nameI18n,
                  description_i18n: skill.descriptionI18n,
                },
                localeKey,
              );
              return {
                id: skill.skillId,
                kind: 'skill' as const,
                name: display.name,
                description: display.description,
                source: t(`conversation.skills.sources.${skill.source}`, { defaultValue: skill.source }),
                avatar: skill.avatar ?? undefined,
                tags: [skill.name],
              };
            })
          : []),
      ],
    [pickerSkills, localeKey, supportsHomeGoalCommand, supportsHomeSkillLoading, t]
  );
  const homeSlashController = useSlashLauncherController({
    input: homeTokenInputState.projection,
    caretPosition: homeTokenInputState.selection.end,
    items: homeLauncherItems,
    onExecuteSystem: (item) => {
      if (item.name === 'open') {
        openHomeFileSelector();
        if (!homeTokenInputRef.current?.replaceActiveSlashToken()) {
          guidInput.setInput(
            replaceActiveSlashToken(
              guidInput.input,
              '',
              homeTokenInputState.textSelection.end,
            ),
          );
        }
        return;
      }
      if (item.name === 'ppt') {
        enterSlideMode();
        if (!homeTokenInputRef.current?.replaceActiveSlashToken()) {
          guidInput.setInput(
            replaceActiveSlashToken(guidInput.input, '', homeTokenInputState.textSelection.end),
          );
        }
        return;
      }
      if (item.name === 'goal') {
        setGoalMode(true);
        if (!homeTokenInputRef.current?.replaceActiveSlashToken()) {
          guidInput.setInput(
            replaceActiveSlashToken(
              guidInput.input,
              '',
              homeTokenInputState.textSelection.end,
            ),
          );
        }
      }
    },
    onSelectSkill: (item) => {
      const skill = pickerSkills.find((candidate) => candidate.skillId === item.id);
      if (!skill) {
        return;
      }
      homeTokenInputRef.current?.insertSkillAtActiveSlash(
        composerChipFromCatalog(
          skill,
          t(`conversation.skills.sources.${skill.source}`, { defaultValue: skill.source }),
          localeKey,
        ),
      );
    },
    onSelectAgent: (item) => {
      if (!homeTokenInputRef.current?.replaceActiveSlashToken(`/${item.name} `)) {
        guidInput.setInput(
          replaceActiveSlashToken(
            guidInput.input,
            `/${item.name} `,
            homeTokenInputState.textSelection.end,
          ),
        );
      }
    },
  });
  const homeSlashMenuItems = useMemo<SlashCommandMenuItem[]>(
    () =>
      homeSlashController.filteredItems.map((item) => ({
        key: item.id,
        label: item.kind === 'skill' ? item.name : `/${item.name}`,
        description: item.description,
        badge: item.source,
        icon:
          item.kind === 'skill' ? (
            <SkillAvatar
              skill={{ name: item.name, avatar: item.avatar }}
              size={18}
              radiusClassName='rounded-5px'
              showShadow={false}
            />
          ) : undefined,
        section:
          item.kind === 'system'
            ? t('conversation.slashLauncher.system')
            : item.kind === 'skill'
              ? t('conversation.slashLauncher.skills')
              : t('conversation.slashLauncher.agent'),
      })),
    [homeSlashController.filteredItems, t]
  );
  const homeAddMenuItems = useMemo<SlashCommandMenuItem[]>(
    () => [
      {
        key: 'files',
        label: t('common.fileAttach.filesAndFolders', { defaultValue: 'Files and folders' }),
        icon: <FlowyIcon name='paperclip' size={16} />,
      },
      {
        key: 'preset',
        label: t('guid.drawer.presetTab', { defaultValue: '专家' }),
        icon: <FlowyIcon name='graduationCap' size={16} />,
        trailing: <FlowyIcon name='right' size={14} />,
      },
      ...(supportsHomeSkillLoading
        ? [
            {
              key: 'skills',
              label: t('guid.entry.skills', { defaultValue: 'Use skills' }),
              icon: <FlowyIcon name='puzzle' size={16} />,
              trailing: <FlowyIcon name='right' size={14} />,
            },
          ]
        : []),
      {
        key: 'knowledge',
        label: t('knowledge.control.label', { defaultValue: '知识库' }),
        icon: <FlowyIcon name='book' size={16} />,
        trailing: <FlowyIcon name='right' size={14} />,
      },
      ...(supportsHomeGoalCommand
        ? [
            {
              key: 'goal',
              label: t('conversation.goal.chip.label', { defaultValue: 'Goal' }),
              icon: <FlowyIcon name='aiming' size={16} />,
            },
          ]
        : []),
    ],
    [supportsHomeGoalCommand, supportsHomeSkillLoading, t],
  );
  const handleOpenHomeAddMenu = useCallback(() => {
    setHomeAddMenuActiveIndex(0);
    setIsHomeAddMenuOpen(true);
    window.requestAnimationFrame(() => homeTokenInputRef.current?.focus());
  }, []);
  const closeEntryFlyout = useCallback((suppressKey?: string | null) => {
    entryFlyoutSuppressedKeyRef.current = suppressKey ?? entryFlyout;
    setEntryFlyout(null);
  }, [entryFlyout]);
  const handleHomeAddMenuSelect = useCallback(
    (item: SlashCommandMenuItem) => {
      const flyoutKey = asHomeFlyout(item.key);
      if (flyoutKey) {
        const nextIndex = homeAddMenuItems.findIndex((candidate) => candidate.key === flyoutKey);
        if (nextIndex >= 0) setHomeAddMenuActiveIndex(nextIndex);
        entryFlyoutPausedKeyRef.current = null;
        setEntryFlyout((current) => {
          if (current === flyoutKey) {
            entryFlyoutSuppressedKeyRef.current = flyoutKey;
            return null;
          }
          entryFlyoutSuppressedKeyRef.current = null;
          return flyoutKey;
        });
        return;
      }
      setEntryFlyout(null);
      entryFlyoutSuppressedKeyRef.current = null;
      setIsHomeAddMenuOpen(false);
      if (item.key === 'files') {
        openHomeFileSelector();
      } else if (item.key === 'goal') {
        setGoalMode(true);
      }
    },
    [homeAddMenuItems, openHomeFileSelector],
  );
  const handleHomeAddMenuKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (!isHomeAddMenuOpen || homeAddMenuItems.length === 0) {
        return false;
      }
      if (event.key === 'Escape' || event.key === 'ArrowLeft') {
        if (entryFlyout) {
          event.preventDefault();
          closeEntryFlyout(entryFlyout);
          return true;
        }
        if (event.key === 'Escape') {
          event.preventDefault();
          setIsHomeAddMenuOpen(false);
          return true;
        }
        return false;
      }
      if (event.key === 'ArrowRight') {
        const activeItem = homeAddMenuItems[homeAddMenuActiveIndex];
        const flyoutKey = asHomeFlyout(activeItem?.key);
        if (flyoutKey) {
          event.preventDefault();
          entryFlyoutSuppressedKeyRef.current = null;
          entryFlyoutPausedKeyRef.current = null;
          setEntryFlyout(flyoutKey);
          return true;
        }
        return false;
      }
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        setHomeAddMenuActiveIndex((current) => (current + 1) % homeAddMenuItems.length);
        return true;
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault();
        setHomeAddMenuActiveIndex((current) => (current - 1 + homeAddMenuItems.length) % homeAddMenuItems.length);
        return true;
      }
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        const activeItem = homeAddMenuItems[homeAddMenuActiveIndex];
        if (!activeItem) return true;
        const flyoutKey = asHomeFlyout(activeItem.key);
        if (flyoutKey) {
          entryFlyoutSuppressedKeyRef.current = null;
          entryFlyoutPausedKeyRef.current = null;
          setEntryFlyout(flyoutKey);
          return true;
        }
        handleHomeAddMenuSelect(activeItem);
        return true;
      }
      return false;
    },
    [closeEntryFlyout, entryFlyout, handleHomeAddMenuSelect, homeAddMenuActiveIndex, homeAddMenuItems, isHomeAddMenuOpen],
  );
  const handleAddMenuEscape = useCallback(
    (event: KeyboardEvent) => {
      if (!entryFlyout) return false;
      event.preventDefault();
      closeEntryFlyout(entryFlyout);
      return true;
    },
    [closeEntryFlyout, entryFlyout],
  );
  useEffect(() => {
    if (!isHomeAddMenuOpen) {
      entryFlyoutSuppressedKeyRef.current = null;
      entryFlyoutPausedKeyRef.current = null;
      setEntryFlyout(null);
      return;
    }
    const item = homeAddMenuItems[homeAddMenuActiveIndex];
    if (entryFlyoutPausedKeyRef.current && entryFlyoutPausedKeyRef.current !== item?.key) {
      entryFlyoutPausedKeyRef.current = null;
    }
    const flyoutKey = asHomeFlyout(item?.key);
    if (!flyoutKey) {
      entryFlyoutSuppressedKeyRef.current = null;
      setEntryFlyout(null);
      return;
    }
    if (entryFlyoutSuppressedKeyRef.current === flyoutKey || entryFlyoutPausedKeyRef.current === flyoutKey) return;
    const key = flyoutKey;
    // Wait briefly so the click that enters the row opens the flyout instead
    // of toggling a panel the pointer movement just revealed.
    const timer = window.setTimeout(() => {
      if (entryFlyoutPausedKeyRef.current === key || entryFlyoutSuppressedKeyRef.current === key) return;
      entryFlyoutSuppressedKeyRef.current = null;
      setEntryFlyout(key);
    }, 160);
    return () => window.clearTimeout(timer);
  }, [homeAddMenuActiveIndex, homeAddMenuItems, isHomeAddMenuOpen]);
  useEffect(() => {
    if (homeSlashController.isOpen) {
      setIsHomeAddMenuOpen(false);
    }
  }, [homeSlashController.isOpen]);
  const homeInitialSkillIds = useMemo(
    () => (supportsHomeSkillLoading ? homeSkillChips.map((skill) => skill.skillId) : []),
    [homeSkillChips, supportsHomeSkillLoading],
  );
  // Slides mode applies only to a real brief: an empty composer stays empty
  // (the send button stays disabled), and nothing is added to it visibly.
  const slidesBrief =
    homeCapability === 'slides' &&
    supportsHomeGoalCommand &&
    slideOptions !== null &&
    guidInput.input.trim().length > 0;
  const sendInput = slidesBrief ? applySlideHeader(guidInput.input, slideOptions) : guidInput.input;
  const sendSkillIds = useMemo(
    () =>
      slidesBrief && managedSlidesSkillReady && !homeInitialSkillIds.includes(PRESENTATION_SKILL_ID)
        ? [...homeInitialSkillIds, PRESENTATION_SKILL_ID]
        : homeInitialSkillIds,
    [homeInitialSkillIds, managedSlidesSkillReady, slidesBrief],
  );

  // Advanced per-conversation drafts (knowledge mounts / AutoWork / IDMM) —
  // collected up front and applied right after the conversation is created.
  const advancedConfig = useGuidAdvancedConfig();

  // Knowledge activation hand-off from QuickCapture / empty-state sample seed.
  // When auto_send is set, arm the same pendingAutoSend path used by model/workspace gates
  // so create → applyBinding → initial message runs without a second click.
  useEffect(() => {
    const activation = consumeKnowledgeActivation();
    if (!activation) return;
    advancedConfig.setKnowledge(activation.binding);
    guidInput.setInput(activation.suggest_prompt);
    if (activation.auto_send) {
      pendingAutoSendRef.current = true;
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps -- one-shot on mount

  // When AutoWork is armed (switch on + requirement tag) the primary button
  // becomes a "Start AutoWork" action: clickable without typed input, and it
  // creates the session + starts AutoWork without sending a first message (see
  // planGuidEntry).
  const isAutoWorkMode = isAutoWorkEntry(advancedConfig.autoWork);

  const mention = useGuidMention({
    availableAgents: agentSelection.availableAgents,
    customAgentAvatarMap: agentSelection.customAgentAvatarMap,
    selectedAgentKey: agentSelection.selectedAgentKey,
    setSelectedAgentKey: agentSelection.setSelectedAgentKey,
    setInput: guidInput.setInput,
    selectedAgentInfo: agentSelection.selectedAgentInfo,
  });

  const needsModelForAgent = useMemo(() => {
    const effectiveType = agentSelection.is_presetAgent
      ? agentSelection.currentEffectiveAgentInfo.agent_type
      : agentSelection.selectedAgent;
    return effectiveType === 'nomi';
  }, [
    agentSelection.currentEffectiveAgentInfo.agent_type,
    agentSelection.is_presetAgent,
    agentSelection.selectedAgent,
  ]);

  const readiness = useMemo(
    () =>
      resolveGuidReadiness({
        intentId: 'freeform',
        hasModel: Boolean(modelSelection.current_model),
        workspaceDir: guidInput.dir,
        needsModelForAgent,
      }),
    [guidInput.dir, modelSelection.current_model, needsModelForAgent]
  );

  const handleLinkWorkspace = useCallback(() => {
    return workspaceDialogGateRef.current(async () => {
      try {
        const dirs = await ipcBridge.dialog.showOpen.invoke({ properties: ['openDirectory', 'createDirectory'] });
        const next = dirs?.[0];
        if (!next) return;
        addRecentWorkspace(next);
        guidInput.setDir(next);
        trackFunnelEvent('prerequisite_resolved', { kind: 'workspace' });
        if (pendingAutoSendRef.current && inputSnapshotRef.current.trim()) {
          pendingAutoSendRef.current = false;
          window.setTimeout(() => {
            sendRef.current?.();
          }, 0);
        }
      } catch (error) {
        console.error('[GuidPage] Failed to open workspace dialog:', error);
      }
    });
  }, [guidInput.setDir]);

  const homeVideoSendRef = useRef<(() => void) | null>(null);
  const backFromHomePanel = useCallback(() => {
    if (homeCapability !== 'office') switchHomeCapability('office');
    setHomeView('home');
    focusHomeComposer();
  }, [focusHomeComposer, homeCapability, switchHomeCapability]);
  const showBuildExamples = useCallback(
    (kind: HomeBuildKind, animated: boolean) => {
      setHomeBuildKind(kind);
      setSelectedBuildExample(null);
      setPendingExample(null);
      switchHomeCapability('develop');
      setHomeView('build-examples');
      setBuildTransitionDirection(null);
      setBuildDrawerMotion(animated ? 'opening' : 'idle');
    },
    [switchHomeCapability],
  );
  const selectHomeOption = useCallback(
    (option: HomeStageOption) => {
      if (option.id === 'slides' && !supportsHomeGoalCommand) return;
      setPendingExample(null);
      if (option.id === 'slides') {
        setSlideOptions((current) => current ?? DEFAULT_SLIDE_OPTIONS);
        setSlidePanelOpen(true);
      }
      const buildKind = homeBuildKindOfOption(option.key);
      if (buildKind) {
        if (buildDrawerMotion !== 'idle') return;
        if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
          showBuildExamples(buildKind, false);
        } else {
          setBuildDirectoryMotion('exit');
          setBuildTransitionDirection('forward');
          setPendingBuildKind(buildKind);
          setBuildDrawerMotion('closing');
        }
        return;
      }
      switchHomeCapability(option.id);
      setHomeView('capability');
    },
    [buildDrawerMotion, showBuildExamples, supportsHomeGoalCommand, switchHomeCapability],
  );
  const handleDirectoryMotionEnd = useCallback(() => {
    if (buildDirectoryMotion === 'exit') {
      if (buildTransitionDirection === 'forward' && pendingBuildKind) {
        showBuildExamples(pendingBuildKind, true);
        setPendingBuildKind(null);
        return;
      }
      if (buildTransitionDirection === 'home') {
        setBuildDrawerMotion('idle');
        setBuildTransitionDirection(null);
        setBuildDirectoryMotion('idle');
        setPendingBuildKind(null);
        setHomeView('home');
        if (homeCapability !== 'office') switchHomeCapability('office');
        focusHomeComposer();
      }
      return;
    }
    if (buildDirectoryMotion === 'expand') {
      setBuildDrawerMotion('idle');
      setBuildDirectoryMotion('idle');
    }
  }, [
    buildDirectoryMotion,
    buildTransitionDirection,
    focusHomeComposer,
    homeCapability,
    pendingBuildKind,
    showBuildExamples,
    switchHomeCapability,
  ]);
  const handleExamplesMotionEnd = useCallback(() => {
    if (buildTransitionDirection === 'back' && buildDrawerMotion === 'closing') {
      showBuildDirectory(true);
      return;
    }
    if (buildDrawerMotion === 'opening') {
      setBuildDrawerMotion('idle');
      setBuildDirectoryMotion('idle');
    }
  }, [buildDrawerMotion, buildTransitionDirection, showBuildDirectory]);

  const send = useGuidSend({
    // Input state (slides mode prepends the ppt-master option line here)
    input: sendInput,
    setInput: guidInput.setInput,
    files: guidInput.files,
    setFiles: guidInput.setFiles,
    dir: guidInput.dir,
    setDir: guidInput.setDir,
    setLoading: guidInput.setLoading,
    loading: guidInput.loading,

    // Agent state
    selectedAgent: agentSelection.selectedAgent,
    selectedAgentKey: agentSelection.selectedAgentKey,
    selectedAgentInfo: agentSelection.selectedAgentInfo,
    is_presetAgent: agentSelection.is_presetAgent,
    is_presetAgentPending: agentSelection.is_presetAgentPending,
    selectedMode: agentSelection.selectedMode,
    selectedAcpModel: agentSelection.selectedAcpModel,
    current_model: modelSelection.current_model,
    reasoningEffort: effectiveReasoningEffort,
    taskProfile: composerProfile,

    // Agent helpers
    findAgentByKey: agentSelection.findAgentByKey,
    getEffectiveAgentType: agentSelection.getEffectiveAgentType,
    onStalePreset: handleStalePreset,
    initialSkillIds: sendSkillIds,
    onInitialSkillsSent: () => setHomeSkillChips([]),
    availableMcpServers,
    selectedMcpServerIds: guidSelectedMcpServerIds,
    currentEffectiveAgentInfo: agentSelection.currentEffectiveAgentInfo,
    applyAdvancedConfig: advancedConfig.applyToConversation,
    autoWork: advancedConfig.autoWork,
    goalMode,
    delegationPolicy,
    executionModelPool,
    decisionPolicy,

    // Mention state reset
    setMentionOpen: mention.setMentionOpen,
    setMentionQuery: mention.setMentionQuery,
    setMentionSelectorOpen: mention.setMentionSelectorOpen,
    setMentionActiveIndex: mention.setMentionActiveIndex,

    // Navigation
    navigate,
    t,
    onNeedModel: () => {
      pendingAutoSendRef.current = true;
      addProviderRef.current?.open();
    },
    onNeedWorkspace: handleLinkWorkspace,
    readinessReady: readiness.ready,
    readinessBlocker: readiness.blocker,

    // Instant "creating conversation" loading overlay (ConversationShell-level)
    beginPending: pendingConversation.begin,
    advancePending: pendingConversation.advance,
    endPending: pendingConversation.end,
    abortPending: pendingConversation.abort,
    attachPending: pendingConversation.attach,
  });

  sendRef.current = send.sendMessageHandler;
  inputSnapshotRef.current = guidInput.input;

  // Fire knowledge-activation auto-send once the suggest prompt is in the composer.
  // If model/workspace is missing, sendMessageHandler re-arms pendingAutoSendRef via
  // onNeedModel / onNeedWorkspace and the existing recovery paths finish the send.
  useEffect(() => {
    if (!pendingAutoSendRef.current) return;
    if (!guidInput.input.trim()) return;
    const timer = window.setTimeout(() => {
      if (!pendingAutoSendRef.current) return;
      pendingAutoSendRef.current = false;
      sendRef.current?.();
    }, 80);
    return () => window.clearTimeout(timer);
  }, [guidInput.input]);

  // --- Coordinated handlers (depend on multiple hooks) ---
  const handleComposerSend = send.sendMessageHandler;
  // Homepage send: the video capability owns its own submit path (clip task +
  // existing video result route); every other capability uses the existing
  // session send path, so office/develop/slides keep their current behavior.
  const homeSend = useCallback(() => {
    if (homeCapability === 'video') {
      homeVideoSendRef.current?.();
      return;
    }
    handleComposerSend();
  }, [handleComposerSend, homeCapability]);

  // Video capability submit: the same clip-task contract the video page uses —
  // upload only explicit material, create the task, register history, then open
  // the existing clip result route. Failures before the create POST keep the
  // homepage draft; a confirmed task is never discarded.
  const clearHomeVideoDraft = useCallback(() => {
    setHomeVideoText('');
    setHomeVideoRefs((current) => {
      current.forEach((reference) => URL.revokeObjectURL(reference.previewUrl));
      return [];
    });
  }, []);
  const cleanupHomeVideoAttemptMedia = useCallback(async (attempt: { uploadedMediaIds: string[] }) => {
    if (attempt.uploadedMediaIds.length === 0) return;
    const results = await Promise.allSettled(
      attempt.uploadedMediaIds.map((mediaId) => deleteCanvasMedia(mediaId)),
    );
    results.forEach((result, index) => {
      if (result.status === 'rejected') {
        console.warn(
          'Failed to delete an unconfirmed home-video upload:',
          attempt.uploadedMediaIds[index],
          result.reason,
        );
      }
    });
  }, []);
  const openHomeVideoTask = useCallback(
    async (taskId: string, title: string, prompt: string) => {
      if (homeVideoOpenGuardRef.current) return false;
      homeVideoOpenGuardRef.current = true;
      try {
        // Registering the task in history and opening its route are one guarded
        // "open the confirmed task" step: either failure keeps the task + media
        // and surfaces the reopen path instead of discarding the result.
        rememberVideoGenerationTask(taskId, title);
        await navigate(`/video-generation/clip/${encodeURIComponent(taskId)}`, {
          state: { title, prompt, taskId },
        });
        setHomeVideoNavFailure(null);
        return true;
      } catch (navigationError) {
        // Confirmed task: keep it and its media, and surface a reopen path.
        console.error('Failed to open the video task route:', navigationError);
        setHomeVideoNavFailure({ taskId, title });
        return false;
      } finally {
        homeVideoOpenGuardRef.current = false;
      }
    },
    [navigate],
  );
  const resolveHomeVideoUnknownAttempt = useCallback(async () => {
    const pending = homeVideoRecovery;
    if (!pending || homeVideoRecoveryCheckingRef.current) return;
    homeVideoRecoveryCheckingRef.current = true;
    setHomeVideoRecoveryChecking(true);
    try {
      if (pending.uploadedMediaIds.length === 0) {
        // No media set to attribute a task uniquely: never claim by text/time.
        Message.warning(t('common.unknownError'));
        return;
      }
      const tasks = await listHomeVideoClaimCandidates();
      const wanted = [...pending.uploadedMediaIds].sort().join(',');
      const matches = tasks.filter(
        (task) => [...(task.reference_media_ids ?? [])].sort().join(',') === wanted,
      );
      if (matches.length !== 1) {
        Message.warning(t('common.unknownError'));
        return;
      }
      const task = matches[0];
      setHomeVideoRecovery(null);
      clearHomeVideoDraft();
      await openHomeVideoTask(task.task_id, pending.title, pending.prompt);
    } catch (cause) {
      Message.error(
        formatClipOperationError(cause, t, {
          key: 'videoGeneration.list.loadError',
          defaultValue: '加载失败',
        }).message,
      );
    } finally {
      homeVideoRecoveryCheckingRef.current = false;
      setHomeVideoRecoveryChecking(false);
    }
  }, [
    homeVideoRecovery,
    clearHomeVideoDraft,
    openHomeVideoTask,
    t,
  ]);
  const submitHomeVideo = useCallback(async () => {
    if (homeVideoSendGuardRef.current) return;
    // R3: the same send action can never issue a second POST for an attempt whose
    // outcome is unproven — it only checks/claims the existing backend task.
    if (homeVideoRecovery) {
      void resolveHomeVideoUnknownAttempt();
      return;
    }
    const text = homeVideoText.trim();
    if (!text) return;
    if (homeVideoCatalogLoading) {
      Message.warning(t('videoGeneration.create.preferences.loading'));
      return;
    }
    if (homeVideoCatalogFailed) {
      Message.error(t('videoGeneration.list.loadError'));
      return;
    }
    // R2: model eligibility is checked before any upload starts.
    if (!homeVideoModel || !homeVideoOpenModels.some((model) => model.id === homeVideoModel)) {
      Message.warning(
        t('videoGeneration.create.modelRequiredAction', {
          defaultValue: 'A video model is required before generating.',
        }),
      );
      return;
    }
    homeVideoSendGuardRef.current = true;
    setHomeVideoSubmitting(true);
    const attempt = {
      uploadedMediaIds: [] as string[],
      prompt: '',
      title: '',
      model: homeVideoModel,
      resolution: normalizeVideoResolution(homeVideoModel, homeVideoResolution),
      durationSecs: clampClipDurationForModel(homeVideoModel, homeVideoDuration),
      postIssued: false,
    };
    homeVideoAttemptRef.current = attempt;
    try {
      const pendingReferences = homeVideoRefs.filter((reference) => reference.file);
      if (homeVideoRefs.length > 0 && pendingReferences.length === 0) {
        throw new Error(
          t('videoGeneration.create.upload.referenceFileMissing', {
            defaultValue: 'Reference image files expired; add them again before sending.',
          }),
        );
      }
      for (const reference of pendingReferences) {
        const media = await uploadCanvasMedia(
          reference.file,
          reference.file.name.replace(/\.[^.]+$/, '').trim() || `reference-${attempt.uploadedMediaIds.length + 1}`,
        );
        attempt.uploadedMediaIds.push(media.media_id);
      }
      // R2: the catalog can refresh while uploads are in flight, so re-check the
      // selected model before the create POST. Uploads are this attempt's own.
      // Read the eligibility REF (not this closure): a running submit keeps its
      // original render's state values for its whole lifetime.
      const liveEligibility = homeVideoEligibilityRef.current;
      if (
        !attempt.model ||
        !liveEligibility.openModels.some((model) => model.id === attempt.model)
      ) {
        await cleanupHomeVideoAttemptMedia(attempt);
        homeVideoAttemptRef.current = null;
        Message.warning(
          t('videoGeneration.create.modelRequiredAction', {
            defaultValue: 'A video model is required before generating.',
          }),
        );
        return;
      }
      attempt.prompt = composeClipPrompt(materializeHomeImageMentions(text), '');
      attempt.title =
        text.split(/\r?\n/, 1)[0]?.trim().slice(0, 36) ||
        t('videoGeneration.create.generateTitle', { defaultValue: '视频生成' });
      // Everything before this line is proving "no task exists"; after it the
      // backend may have created a task even if this page sees a failure.
      attempt.postIssued = true;
      const task = await withHomeVideoTimeout(
        createGenerationTask({
          mode: 'video',
          prompt: attempt.prompt,
          model: attempt.model,
          resolution: attempt.resolution,
          duration_secs: attempt.durationSecs,
          reference_media_ids: attempt.uploadedMediaIds,
        }),
        HOME_VIDEO_CREATE_TIMEOUT_MS,
      );
      homeVideoAttemptRef.current = null;
      setHomeVideoNavFailure(null);
      clearHomeVideoDraft();
      await openHomeVideoTask(task.task_id, attempt.title, attempt.prompt);
    } catch (cause) {
      const pending = homeVideoAttemptRef.current;
      homeVideoAttemptRef.current = null;
      if (isInvalidCloudSessionError(cause)) {
        if (pending) await cleanupHomeVideoAttemptMedia(pending);
        await navigate('/cloud-login');
        return;
      }
      if (!pending) return;
      if (!pending.postIssued || isDefinitelyNotCreatedCreateError(cause)) {
        // Upload-stage failure, or a deterministic 4xx rejection: no task exists,
        // so only this attempt's uploads are cleaned. Draft stays retryable.
        await cleanupHomeVideoAttemptMedia(pending);
        const failure = formatClipOperationError(cause, t, {
          key: 'videoGeneration.create.generateFailed',
          defaultValue: '视频生成创建失败',
        });
        Message.error(failure.message);
        return;
      }
      // 5xx / timeout / disconnect / unparsable response: the task may exist.
      // Keep the draft, the uploaded media ids and block this send path until a
      // unique media set confirms the backend task.
      setHomeVideoRecovery({ ...pending, postIssued: true });
      Message.warning(t('common.unknownError'));
    } finally {
      homeVideoSendGuardRef.current = false;
      setHomeVideoSubmitting(false);
    }
  }, [
    homeVideoRecovery,
    resolveHomeVideoUnknownAttempt,
    homeVideoText,
    homeVideoCatalogLoading,
    homeVideoCatalogFailed,
    homeVideoModel,
    homeVideoOpenModels,
    homeVideoResolution,
    homeVideoDuration,
    homeVideoRefs,
    cleanupHomeVideoAttemptMedia,
    clearHomeVideoDraft,
    openHomeVideoTask,
    navigate,
    t,
  ]);
  homeVideoSendRef.current = () => {
    void submitHomeVideo();
  };

  const handleInputChange = useCallback(
    (value: string) => {
      setIsHomeAddMenuOpen(false);
      guidInput.setInput(value);
      const match = value.match(mention.mentionMatchRegex);
      // 首页不根据输入 @ 呼起 mention 列表，占位符里的 @agent 仅为提示，选 agent 用顶部栏或下拉手动选
      if (match) {
        mention.setMentionQuery(match[1]);
        mention.setMentionOpen(false);
      } else {
        mention.setMentionQuery(null);
        mention.setMentionOpen(false);
      }
    },
    [mention.mentionMatchRegex, guidInput.setInput, mention.setMentionQuery, mention.setMentionOpen],
  );

  const [sendKeyPref] = useConfig('chat.sendKey');
  const sendKey = sendKeyPref ?? 'enter';

  const handleInputKeyDown = useCallback(
    (event: React.KeyboardEvent) => {
      if (handleHomeAddMenuKeyDown(event)) {
        return;
      }
      if (homeSlashController.onKeyDown(event)) {
        return;
      }
      if (
        (mention.mentionOpen || mention.mentionSelectorOpen) &&
        (event.key === 'ArrowDown' || event.key === 'ArrowUp')
      ) {
        event.preventDefault();
        if (mention.filteredMentionOptions.length === 0) return;
        mention.setMentionActiveIndex((prev) => {
          if (event.key === 'ArrowDown') {
            return (prev + 1) % mention.filteredMentionOptions.length;
          }
          return (prev - 1 + mention.filteredMentionOptions.length) % mention.filteredMentionOptions.length;
        });
        return;
      }
      if ((mention.mentionOpen || mention.mentionSelectorOpen) && event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        if (mention.filteredMentionOptions.length > 0) {
          const query = mention.mentionQuery?.toLowerCase();
          const exactMatch = query
            ? mention.filteredMentionOptions.find(
                (option) => option.label.toLowerCase() === query || option.tokens.has(query),
              )
            : undefined;
          const selected =
            exactMatch ||
            mention.filteredMentionOptions[mention.mentionActiveIndex] ||
            mention.filteredMentionOptions[0];
          if (selected) {
            mention.selectMentionAgent(selected.key);
            return;
          }
        }
        mention.setMentionOpen(false);
        mention.setMentionQuery(null);
        mention.setMentionSelectorOpen(false);
        mention.setMentionActiveIndex(0);
        return;
      }
      if (mention.mentionOpen && (event.key === 'Backspace' || event.key === 'Delete') && !mention.mentionQuery) {
        mention.setMentionOpen(false);
        mention.setMentionQuery(null);
        mention.setMentionActiveIndex(0);
        return;
      }
      if (
        !mention.mentionOpen &&
        mention.mentionSelectorVisible &&
        !guidInput.input.trim() &&
        (event.key === 'Backspace' || event.key === 'Delete')
      ) {
        event.preventDefault();
        mention.setMentionSelectorVisible(false);
        mention.setMentionSelectorOpen(false);
        mention.setMentionActiveIndex(0);
        return;
      }
      if ((mention.mentionOpen || mention.mentionSelectorOpen) && event.key === 'Escape') {
        event.preventDefault();
        mention.setMentionOpen(false);
        mention.setMentionQuery(null);
        mention.setMentionSelectorOpen(false);
        mention.setMentionActiveIndex(0);
        return;
      }
      if (isSubmitGesture(event, sendKey)) {
        event.preventDefault();
        if (!hasGuidInitialPayload(guidInput.input, homeInitialSkillIds)) return;
        homeSend();
      }
    },
    [
      homeInitialSkillIds,
      handleHomeAddMenuKeyDown,
      homeSlashController,
      mention,
      guidInput.input,
      homeSend,
      sendKey,
    ],
  );

  const handleSelectPresetKey = useCallback(
    (selectedAgentKey: string) => {
      agentSelection.setSelectedAgentKey(selectedAgentKey);
      mention.setMentionOpen(false);
      mention.setMentionQuery(null);
      mention.setMentionSelectorOpen(false);
      mention.setMentionActiveIndex(0);
    },
    [
      agentSelection.setSelectedAgentKey,
      mention.setMentionOpen,
      mention.setMentionQuery,
      mention.setMentionSelectorOpen,
      mention.setMentionActiveIndex,
    ],
  );

  const selectedPresetRecord = useMemo(() => {
    if (!agentSelection.is_presetAgent || !agentSelection.selectedAgentInfo?.preset_id) return undefined;
    return agentSelection.presets.find(
      (item) => item.preset_id === agentSelection.selectedAgentInfo?.preset_id,
    );
  }, [agentSelection.presets, agentSelection.is_presetAgent, agentSelection.selectedAgentInfo?.preset_id]);

  // Reset guid-local UI state before paint so same-route navigations do not
  // briefly show the previous draft or preset preset layout.
  useLayoutEffect(() => {
    guidInput.setInput('');
    guidInput.setFiles([]);
    guidInput.setLoading(false);
    setHomeSkillChips([]);
    if (!(location.state as { workspace?: string } | null)?.workspace) {
      guidInput.setDir('');
    }
    advancedConfig.reset();
  }, [
    guidInput.setDir,
    guidInput.setFiles,
    guidInput.setInput,
    guidInput.setLoading,
    setHomeSkillChips,
    advancedConfig.reset,
    location.key,
    location.state,
  ]);

  // Clear resetPreset from location.state after the hook has consumed it,
  // so that re-renders don't re-trigger the reset logic.
  //
  // Must go through React Router's navigate — raw window.history.replaceState
  // with `location.pathname` would write the HashRouter virtual path (e.g.
  // '/guid') into the browser's real URL and strip the leading '#'. On the
  // next hard reload, the browser would then request '/guid' directly from
  // the dev server (which has no SPA fallback) and 404.
  useEffect(() => {
    if (!resetPresetRequested && !preselectAgentKey) return;
    navigate(`${location.pathname}${location.search}${location.hash}`, {
      replace: true,
      state: null,
    });
  }, [resetPresetRequested, preselectAgentKey, location.pathname, location.search, location.hash, navigate]);

  const currentPresetAgentId =
    selectedPresetRecord?.preferred_agent_id || selectedPresetRecord?.agent_preferences[0]?.agent_id;
  // Mirrors PresetEditDrawer's Main Agent options so avatars resolve the same way.
  const agentSwitcherItems = useMemo(() => {
    if (!agentSelection.availableAgents || !selectedPresetRecord) return [];
    return agentSelection.availableAgents
      .filter((a) => !a.is_preset && a.agent_type !== 'remote')
      .map((a) => {
        const key = a.id || a.backend || a.agent_type;
        const extensionAvatar = a.isExtension ? resolveExtensionAssetUrl(a.avatar) : undefined;
        const logo =
          extensionAvatar ||
          resolveAgentLogo({
            icon: a.icon,
            backend: a.backend || a.agent_type,
            agentId: a.id,
            isExtension: a.isExtension,
          });
        return {
          key,
          label: a.name,
          logo,
          isCurrent: key === currentPresetAgentId,
          isExtension: a.isExtension,
        };
      });
  }, [agentSelection.availableAgents, currentPresetAgentId, selectedPresetRecord]);

  const effectiveAgentRecord = useMemo(() => {
    return agentSelection.availableAgents?.find(
      (agent) =>
        !agent.is_preset && (agent.backend || agent.agent_type) === agentSelection.currentEffectiveAgentInfo.agent_type,
    );
  }, [agentSelection.availableAgents, agentSelection.currentEffectiveAgentInfo.agent_type]);

  const effectiveAgentLogo = useMemo(
    () =>
      resolveAgentLogo({
        icon: effectiveAgentRecord?.icon,
        backend: effectiveAgentRecord?.backend || agentSelection.currentEffectiveAgentInfo.agent_type,
        agentId: effectiveAgentRecord?.id,
        isExtension: effectiveAgentRecord?.isExtension,
      }),
    [effectiveAgentRecord, agentSelection.currentEffectiveAgentInfo.agent_type],
  );
  const handlePresetAgentSwitch = useCallback(
    async (nextAgentId: AgentId) => {
      const presetId = selectedPresetRecord?.preset_id;
      if (!presetId || nextAgentId === currentPresetAgentId) return;
      try {
        await swrMutate(
          PRESET_CATALOG_SWR_KEY,
          (prev: Preset[] | undefined) =>
            prev?.map((item) =>
              item.preset_id === presetId ? { ...item, preferred_agent_id: nextAgentId } : item,
            ),
          { revalidate: false },
        );
        await ipcBridge.presets.setState.invoke({
          preset_id: presetId,
          preferred_agent_id: nextAgentId,
        });
        await Promise.all([
          swrMutate(PRESET_CATALOG_SWR_KEY),
          agentSelection.refreshCustomAgents(),
        ]);
        const agent_name = agentSelection.availableAgents?.find((a) => a.id === nextAgentId)?.name || nextAgentId;
        Message.success(t('guid.switchedToAgent', { agent: agent_name }));
      } catch (error) {
        console.error('[GuidPage] Failed to switch preset agent preference:', error);
        Message.error(t('common.failed', { defaultValue: 'Failed' }));
      }
    },
    [agentSelection, currentPresetAgentId, selectedPresetRecord, t],
  );

  // Build the mention dropdown node
  const mentionDropdownNode = (
    <MentionDropdown
      menuRef={mention.mentionMenuRef}
      options={mention.filteredMentionOptions}
      selectedKey={mention.mentionMenuSelectedKey}
      onSelect={mention.selectMentionAgent}
    />
  );

  // Build the model selector node — a plain single-select model picker.
  const modelSelectorNode = (
    <GuidModelSelector
      isGeminiMode={isGeminiMode}
      modelList={modelSelection.modelList}
      current_model={modelSelection.current_model}
      defaultModelUnavailable={modelSelection.defaultModelUnavailable}
      setCurrentModel={modelSelection.setCurrentModel}
      modelPicker={modelSelection.modelPicker}
      isModelCatalogLoading={modelSelection.isModelCatalogLoading}
      modelCatalogError={modelSelection.modelCatalogError}
      refreshModelCatalog={modelSelection.refreshModelCatalog}
      popupVisible={activeChatPopup === 'model'}
      onPopupVisibleChange={handleModelPopupVisibleChange}
      currentAcpCachedModelInfo={agentSelection.currentAcpCachedModelInfo}
      selectedAcpModel={agentSelection.selectedAcpModel}
      setSelectedAcpModel={agentSelection.setSelectedAcpModel}
    />
  );

  const handleReasoningEffortChanged = useCallback((effort: string | undefined) => {
    setSelectedReasoningEffort(effort);
    if (effort) {
      void savePreferredReasoningEffort(effort);
    }
  }, []);

  const autoTierSelectorNode =
    isGeminiMode && selectedChatModelOption?.family === 'auto' ? (
      <AutoTierSelector
        options={modelSelection.modelPicker.autoModels}
        selected={selectedChatModelOption}
        popupVisible={activeChatPopup === 'strategy'}
        onPopupVisibleChange={handleStrategyPopupVisibleChange}
        onSelect={(option) =>
          modelSelection.setCurrentModel({
            ...option.provider,
            use_model: option.model,
          } as TProviderWithModel)
        }
      />
    ) : null;

  const reasoningEffortSelectorNode =
    selectedChatModelOption?.family !== 'auto' && reasoningEffortLevels.length > 0 ? (
      <ReasoningEffortSelector
        levels={reasoningEffortLevels}
        modelKey={`${modelSelection.current_model?.id ?? ''}:${modelSelection.current_model?.use_model ?? ''}`}
        initialEffort={selectedReasoningEffort}
        popupVisible={activeChatPopup === 'strategy'}
        onPopupVisibleChange={handleStrategyPopupVisibleChange}
        onEffortChanged={handleReasoningEffortChanged}
      />
    ) : null;

  const strategySelectorNode =
    isGeminiMode &&
    (selectedChatModelOption?.family === 'auto' ||
      reasoningEffortLevels.length > 0 ||
      modelSelection.isModelCatalogLoading ||
      Boolean(modelSelection.modelCatalogError))
      ? autoTierSelectorNode ??
        reasoningEffortSelectorNode ??
        <span className='sendbox-strategy-slot-placeholder' aria-hidden='true' />
      : null;

  useEffect(() => {
    if (previousChatModelContextKeyRef.current !== chatModelContextKey) {
      previousChatModelContextKeyRef.current = chatModelContextKey;
      setActiveChatPopup(null);
      return;
    }
    setActiveChatPopup((current) => {
      if (!isGeminiMode) return null;
      if (current === 'strategy' && !strategySelectorNode) return null;
      return current;
    });
  }, [chatModelContextKey, isGeminiMode, strategySelectorNode]);

  // Build the action row
  // When AutoWork is enabled (with a tag) the primary button becomes a
  // "Start AutoWork" action: clickable without typed input, and it creates the
  // session + starts AutoWork without sending a first message (see planGuidEntry).
  const hasDraft = hasGuidInitialPayload(guidInput.input, homeInitialSkillIds) || guidInput.files.length > 0;
  const actionRowNode = (
    <GuidActionRow
      onStartVoiceCall={isNomiAgent ? handleStartVoiceCall : undefined}
      isVoiceConnecting={isVoiceConnecting || (!isVoiceActive && voicePrerequisite === 'loading')}
      isVoiceActive={isVoiceActive}
      voiceCallLabel={t(
        isVoiceActive
          ? 'voice.entry.openActive'
          : voicePrerequisite === 'loading'
            ? 'voice.entry.checking'
            : (voicePrerequisite === 'ready' || voicePrerequisite === 'setup')
              ? 'voice.entry.start'
              : 'voice.entry.setup'
      )}
      onOpenAddMenu={handleOpenHomeAddMenu}
      addMenuOpen={isHomeAddMenuOpen}
      onAddMenuOpenChange={setIsHomeAddMenuOpen}
      addMenuActiveIndex={homeAddMenuActiveIndex}
      addMenuPopoverRef={homeAddMenuPopoverRef}
      entryFlyoutRef={homeEntryFlyoutRef}
      onAddMenuEscape={handleAddMenuEscape}
      onDismissEntryFlyout={() => {
        entryFlyoutPausedKeyRef.current = entryFlyout ?? homeAddMenuItems[homeAddMenuActiveIndex]?.key ?? '*';
        setEntryFlyout(null);
        setHomeAddMenuActiveIndex(-1);
      }}
      entryFlyout={
        entryFlyout === 'knowledge' ? (
          <GuidAnchoredFlyout
            ref={homeEntryFlyoutRef}
            anchorRef={homeAddMenuPopoverRef}
            label={t('knowledge.control.label', { defaultValue: '知识库' })}
            testId='guid-knowledge-control'
          >
            <KnowledgeControl
              presentation='panel'
              draft={{ value: advancedConfig.knowledge, onChange: advancedConfig.setKnowledge }}
              applyNote={t('guid.advanced.applyNote')}
            />
          </GuidAnchoredFlyout>
        ) : entryFlyout ? (
          <GuidEntryFlyout
            ref={homeEntryFlyoutRef}
            mode={entryFlyout}
            anchorRef={homeAddMenuPopoverRef}
            presets={agentSelection.presets}
            localeKey={localeKey}
            selectedPresetId={selectedPresetRecord?.preset_id}
            skills={supportsHomeSkillLoading ? pickerSkills : []}
            selectedSkillIds={homeSkillChips.map((skill) => skill.skillId)}
            onToggleSkill={supportsHomeSkillLoading ? handleToggleHomeSkill : undefined}
            skillsLoading={catalogSkillsLoading}
            skillsError={catalogSkillsError}
            onSelectPreset={(id) => {
              handleSelectPresetKey(`preset:${id}`);
              setEntryFlyout(null);
              entryFlyoutSuppressedKeyRef.current = null;
              setIsHomeAddMenuOpen(false);
            }}
            onFree={() => {
              agentSelection.setSelectedAgentKey(agentSelection.defaultAgentKey);
              setEntryFlyout(null);
              entryFlyoutSuppressedKeyRef.current = null;
              setIsHomeAddMenuOpen(false);
            }}
            onManage={() => {
              const target = entryFlyout === 'skills' ? '/skills' : '/settings/presets';
              setEntryFlyout(null);
              entryFlyoutSuppressedKeyRef.current = null;
              setIsHomeAddMenuOpen(false);
              void navigate(target);
            }}
          />
        ) : null
      }
      addMenu={
        <SlashCommandMenu
          title={t('common.add')}
          compact
          items={homeAddMenuItems}
          activeIndex={homeAddMenuActiveIndex}
          onHoverItem={setHomeAddMenuActiveIndex}
          onSelectItem={handleHomeAddMenuSelect}
          emptyText={t('messages.slash.empty', { defaultValue: 'No commands found' })}
        />
      }
      modelSelectorNode={modelSelectorNode}
      modelSelectorIsChat={isGeminiMode}
      activeChatPopup={activeChatPopup}
      reasoningEffortSelectorNode={strategySelectorNode}
      selectedAgent={agentSelection.selectedAgent}
      effectiveModeAgent={agentSelection.currentEffectiveAgentInfo.agent_type}
      selectedMode={agentSelection.selectedMode}
      onModeSelect={agentSelection.setSelectedMode}
      is_presetAgent={agentSelection.is_presetAgent}
      selectedAgentInfo={agentSelection.selectedAgentInfo}
      presets={agentSelection.presets}
      localeKey={localeKey}
      onClosePresetTag={() => agentSelection.setSelectedAgentKey(agentSelection.defaultAgentKey)}
      agentLogo={effectiveAgentLogo}
      agentSwitcherItems={agentSwitcherItems}
      onAgentSwitch={(key) => {
        handlePresetAgentSwitch(parseAgentId(key)).catch((err) =>
          console.error('Failed to switch preset agent:', err)
        );
      }}
      mcpServers={availableMcpServers}
      selectedMcpServerIds={guidSelectedMcpServerIds ?? []}
      onToggleMcpServer={handleToggleMcpServer}
      hidePresetTag
      loading={guidInput.loading}
      autoWorkMode={isAutoWorkMode}
      autoWorkDraft={advancedConfig.autoWork}
      goalMode={supportsHomeGoalCommand && goalMode}
      onGoalModeChange={supportsHomeGoalCommand ? setGoalMode : undefined}
      slidesActive={supportsHomeGoalCommand && homeCapability === 'slides'}
      onSlidesChange={supportsHomeGoalCommand ? handleSlidesChange : undefined}
      taskProfile={composerProfile}
      onTaskProfileChange={handleHomeTaskProfileChange}
      slideControls={
        homeCapability === 'slides' && slideOptions !== null ? (
          <SlideOptionsControls
            options={slideOptions}
            onChange={setSlideOptions}
            disabled={guidInput.loading}
            compact={homeCompact}
            templatesOpen={slidePanelOpen}
            onToggleTemplates={() => setSlidePanelOpen((open) => !open)}
          />
        ) : undefined
      }
      hasDraft={hasDraft}
      speechLocale={i18n.language}
      onSpeechTranscript={(transcript) => {
        guidInput.setInput((current) => appendSpeechTranscript(current, transcript));
      }}
      dictationActivation={dictationActivation}
      onSend={homeSend}
    />
  );

  return (
    <ConfigProvider getPopupContainer={() => guidContainerRef.current || document.body}>
      <div
        ref={guidContainerRef}
        className={styles.guidContainer}
        data-slide-studio={slideStudioOpen || undefined}
      >
        {/* Slides owns the slide-card wall. Build has its own lightweight layer. */}
        {slideStudioOpen ? (
          <SlideCardWall />
        ) : buildAmbientVariant ? (
          <div
            className={styles.ambientWall}
            data-testid='guid-build-ambient'
            data-ambient-variant={buildAmbientVariant}
            aria-hidden='true'
          >
            <BuildAmbientWall variant={buildAmbientVariant} />
          </div>
        ) : null}
        <div className={styles.guidScrollArea} data-home-layer={homeView}>
          <div className={styles.guidPrimaryStage}>
            <div className={styles.guidLayout}>
              {homeView === 'home' ? (
                <div className={styles.homeStage}>
                  <GuidHomeBrand />
                  <div className={styles.homeEntries}>
                    <FeatureCards
                      onOpenDevelop={() => openHomeDirectory('develop')}
                      onOpenCreate={() => openHomeDirectory('create')}
                    />
                  </div>
                  <div className={styles.homeGallery}>
                    <ShowcaseCards
                      onSelectVideo={handleSelectShowcaseVideo}
                      onSelectApp={handleSelectShowcaseApp}
                      onSelectImage={handleSelectShowcaseImage}
                    />
                  </div>
                </div>
              ) : null}
              {homeView === 'capability' && homeCapability === 'video' ? (
                <div className={styles.homePrepStage}>
                  <button
                    type='button'
                    className={styles.homePrepHero}
                    data-testid='guid-home-video-hero'
                    onClick={() => requestHomeExample('video', t('guid.home.showcaseVideoTitle'))}
                  >
                    <img className={styles.homePrepCover} src={showcaseVideoCover} alt='' />
                    <div className={styles.homePrepCopy}>
                      <span className={styles.homePrepTitle}>{t('guid.home.showcaseVideoTitle')}</span>
                      <span className={styles.homePrepText}>{t('guid.home.videoShortcutSubtitle')}</span>
                    </div>
                  </button>
                </div>
              ) : null}
            </div>
          </div>
        </div>

        <div className={styles.guidFixedInputArea}>
          <div className={styles.guidLayout}>
            {pendingExample ? (
              <div
                className={styles.exampleConfirm}
                data-testid='guid-home-example-confirm'
                role='alertdialog'
                aria-label={pendingExample.title}
              >
                <span className={styles.exampleConfirmText}>{pendingExample.title}</span>
                <div className={styles.exampleConfirmActions}>
                  <button
                    ref={exampleConfirmCancelRef}
                    type='button'
                    className={styles.exampleConfirmCancel}
                    onClick={dismissPendingExample}
                  >
                    {t('common.cancel', { defaultValue: 'Cancel' })}
                  </button>
                  <button
                    type='button'
                    className={styles.exampleConfirmApply}
                    onClick={() => {
                      const run = pendingExample.run;
                      setPendingExample(null);
                      run();
                    }}
                  >
                    {t('common.confirm', { defaultValue: 'Confirm' })}
                  </button>
                </div>
              </div>
            ) : null}
            {homeView === 'develop-directory' ? (
              <HomeInputDrawer
                title={t('guid.home.buildTitle')}
                options={homeDevelopOptions}
                onSelect={selectHomeOption}
                onClose={closeHomePanel}
                closeLabel={t('common.close', { defaultValue: 'Close' })}
                testId='guid-home-develop-directory'
                onMotionEnd={handleDirectoryMotionEnd}
                motion={buildDirectoryMotion}
                motionDuration={buildDrawerMotion === 'closing' ? BUILD_DRAWER_COLLAPSE_MS : BUILD_DRAWER_EXPAND_MS}
              />
            ) : null}
            {homeView === 'build-examples' ? (
              <HomeInputDrawer
                title={t(HOME_BUILD_COPY[homeBuildKind ?? 'website'].title)}
                options={homeBuildExampleOptions}
                onSelect={selectBuildExample}
                onClose={backFromBuildExamples}
                closeLabel={t('common.close', { defaultValue: 'Close' })}
                testId='guid-home-build-examples'
                onMotionEnd={handleExamplesMotionEnd}
                motion={
                  buildTransitionDirection === 'back' && buildDrawerMotion === 'closing'
                    ? 'exit'
                    : buildDrawerMotion === 'opening'
                      ? 'expand'
                      : 'idle'
                }
                motionDuration={buildDrawerMotion === 'closing' ? BUILD_DRAWER_COLLAPSE_MS : BUILD_DRAWER_EXPAND_MS}
              />
            ) : null}
            {homeView === 'create-directory' ? (
              <HomeInputDrawer
                title={t('guid.home.createTitle')}
                options={homeCreateOptions}
                onSelect={selectHomeOption}
                onClose={closeHomePanel}
                closeLabel={t('common.close', { defaultValue: 'Close' })}
                testId='guid-home-create-directory'
              />
            ) : null}
            {homeView === 'capability' && homeCapability === 'slides' && slidePanelOpen && slideOptions !== null ? (
              <div className={styles.slidePanelHost} data-testid='guid-home-slides-panel'>
                <SlideCreatePanel
                  template={slideOptions.template}
                  onSelectTemplate={handleSelectSlideTemplate}
                  onSelectSuggestion={handleSelectSlideSuggestion}
                  onImportTemplate={handleImportSlideTemplate}
                  onClose={() => setSlidePanelOpen(false)}
                  disabled={guidInput.loading}
                />
              </div>
            ) : null}
            {homeView === 'capability' && homeCapability === 'video' ? (
              <HomeVideoComposer
                text={homeVideoText}
                onTextChange={setHomeVideoText}
                references={homeVideoRefs}
                onAddReferences={handleAddVideoRefs}
                onRemoveReference={handleRemoveVideoRef}
                model={homeVideoModel}
                modelOptions={homeVideoModelOptions}
                onModelChange={setHomeVideoModel}
                modelsLoading={homeVideoCatalogState === 'loading'}
                catalogState={homeVideoCatalogState}
                lockedModelCount={homeVideoLockedModelCount}
                onRetryModels={retryHomeVideoCatalog}
                recovery={
                  homeVideoRecovery
                    ? {
                        kind: 'unknown',
                        checking: homeVideoRecoveryChecking,
                        onCheck: () => {
                          void resolveHomeVideoUnknownAttempt();
                        },
                        onOpen: () => {
                          void navigate('/video-generation');
                        },
                      }
                    : homeVideoNavFailure
                      ? {
                          kind: 'navigation-failed',
                          checking: false,
                          onCheck: () => {
                            void openHomeVideoTask(
                              homeVideoNavFailure.taskId,
                              homeVideoNavFailure.title,
                              '',
                            );
                          },
                          onOpen: () => {
                            void openHomeVideoTask(
                              homeVideoNavFailure.taskId,
                              homeVideoNavFailure.title,
                              '',
                            );
                          },
                        }
                      : undefined
                }
                durationSecs={homeVideoDuration}
                durationOptions={homeVideoDurationOptions}
                onDurationChange={setHomeVideoDuration}
                resolution={homeVideoResolution}
                resolutionOptions={homeVideoResolutionOptions}
                onResolutionChange={setHomeVideoResolution}
                submitting={homeVideoSubmitting}
                onSend={() => {
                  void submitHomeVideo();
                }}
                onExit={backFromHomePanel}
              />
            ) : (
            <GuidInputCard
              containerRef={guidInputCardRef}
              input={guidInput.input}
              onInputChange={handleInputChange}
              onKeyDown={handleInputKeyDown}
              onPaste={guidInput.onPaste}
              onFocus={guidInput.handleTextareaFocus}
              onBlur={guidInput.handleTextareaBlur}
              placeholder={homePlaceholder}
              isInputActive={guidInput.isInputFocused}
              isFileDragging={guidInput.isFileDragging}
              activeBorderColor={activeBorderColor}
              inactiveBorderColor={inactiveBorderColor}
              dragHandlers={guidInput.dragHandlers}
              mentionOpen={mention.mentionOpen}
              mentionSelectorBadge={
                <MentionSelectorBadge
                  visible={mention.mentionSelectorVisible}
                  open={mention.mentionSelectorOpen}
                  onOpenChange={mention.setMentionSelectorOpen}
                  agentLabel={mention.selectedAgentLabel}
                  mentionMenu={mentionDropdownNode}
                  onResetQuery={() => mention.setMentionQuery(null)}
                />
              }
              mentionDropdown={mentionDropdownNode}
              slashMenuOpen={homeSlashController.isOpen}
              slashMenu={
                <SlashCommandMenu
                  title={t('messages.slash.title', { defaultValue: 'Commands' })}
                  hint={t('messages.slash.hint', { defaultValue: 'Type / to open command menu' })}
                  compact
                  items={homeSlashMenuItems}
                  activeIndex={homeSlashController.activeIndex}
                  onHoverItem={homeSlashController.setActiveIndex}
                  onSelectItem={(item) => {
                    const targetIndex = homeSlashController.filteredItems.findIndex(
                      (launcherItem) => launcherItem.id === item.key
                    );
                    if (targetIndex >= 0) {
                      homeSlashController.onSelectByIndex(targetIndex);
                    }
                  }}
                  emptyText={t('messages.slash.empty', { defaultValue: 'No commands found' })}
                />
              }
              skillChips={supportsHomeSkillLoading ? homeSkillChips : []}
              onSkillChipsChange={supportsHomeSkillLoading ? setHomeSkillChips : undefined}
              onTokenInputStateChange={setHomeTokenInputState}
              tokenInputRef={homeTokenInputRef}
              files={guidInput.files}
              onRemoveFile={guidInput.handleRemoveFile}
              actionRow={actionRowNode}
              workspaceDir={guidInput.dir}
              onSelectWorkspace={(dir) => guidInput.setDir(dir)}
              onClearWorkspace={() => guidInput.setDir('')}
              onOpenLocalDialog={() => void handleLinkWorkspace()}
            />
            )}

            {/* Editor host (modals + fallback notice) */}
            <GuidPresetEditorHost
              localeKey={localeKey}
              currentEffectiveAgentInfo={agentSelection.currentEffectiveAgentInfo}
            />
          </div>
        </div>

        <GuidAddProviderModal
          ref={addProviderRef}
          onConfigured={(model) => {
            void modelSelection.setCurrentModel(model).then(() => {
              trackFunnelEvent('prerequisite_resolved', { kind: 'model' });
              if (pendingAutoSendRef.current) {
                pendingAutoSendRef.current = false;
                window.setTimeout(() => {
                  send.sendMessageHandler();
                }, 0);
              }
            }).catch((error) => {
              console.error('[GuidPage] Failed to persist configured model:', error);
            });
          }}
        />
      </div>
    </ConfigProvider>
  );
};

export default GuidPage;
