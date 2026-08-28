import { Button } from '@arco-design/web-react';
import { ArrowUp, Brain, Down, Lightning, LinkOne, More, Search } from '@icon-park/react';
import React, { useEffect, useMemo, useState } from 'react';
import type { IProvider, TProviderWithModel } from '@/common/config/storage';
import { FLOWY_BUILTIN_PROVIDER_ID } from '@/common/types/ids';
import { LayoutContext } from '@renderer/hooks/context/LayoutContext';
import { PreviewProvider } from '@renderer/pages/conversation/Preview';
import { CloudAuthContext, type CloudAuthContextValue } from '@renderer/hooks/context/CloudAuthContext';
import NomiModelSelector from '@renderer/pages/conversation/platforms/nomi/NomiModelSelector';
import type { NomiModelSelection } from '@renderer/pages/conversation/platforms/nomi/useNomiModelSelection';
import { ContextUsageRing } from '@renderer/pages/conversation/platforms/nomi/ContextUsageRing';
import AgentModeSelector from '@renderer/components/agent/AgentModeSelector';
import AutoTierSelector from '@renderer/components/agent/AutoTierSelector';
import ReasoningEffortSelector from '@renderer/components/agent/ReasoningEffortSelector';
import ComposerSubmitCluster from '@renderer/components/chat/ComposerSubmitCluster';
import { GuidModelSelectorButton } from '@renderer/pages/guid/components/GuidModelSelector';
import guidStyles from '@renderer/pages/guid/index.module.css';
import KnowledgeDetailActionBar from '@renderer/pages/knowledge/KnowledgeDetailPage/KnowledgeDetailActionBar';
import type {
  ChatModelOption,
  ChatModelPickerViewModel,
} from '@renderer/utils/model/chatModelPicker';
import { ensureThemeControlContract } from '@renderer/utils/theme/themeControlContract';
import { processCustomCss } from '@renderer/utils/theme/customCssProcessor';
import { PRESET_THEMES } from '@renderer/pages/settings/DisplaySettings/presets';
import './buttonLayoutProbe.css';

type ProbeButtonReport = {
  id: string;
  label: string;
  style: {
    display: string;
    flexDirection: string;
    alignItems: string;
    justifyContent: string;
    whiteSpace: string;
    writingMode: string;
  };
  rect: { x: number; y: number; width: number; height: number };
  iconRect: { x: number; y: number; width: number; height: number } | null;
  labelRect: { x: number; y: number; width: number; height: number } | null;
  centerDeltaY: number | null;
  visible: boolean;
  clipped: boolean;
  outOfViewport: boolean;
  iconOnly: boolean;
  circle: boolean;
  displaySources: Array<{ selector: string; value: string; important: string }>;
  pass: boolean;
  failures: string[];
};

type ProbeReport = {
  ok: boolean;
  missingFixtures: string[];
  viewport: { width: number; height: number; devicePixelRatio: number };
  theme: string;
  locale: string;
  userAgent: string;
  styleOrder: string[];
  buttons: ProbeButtonReport[];
  missingChatFixtures: string[];
  chatScenarios: ProbeChatScenarioReport[];
  chatCoordinateStability: ProbeChatCoordinateStability[];
};

type ProbeLayoutElement = {
  rect: NonNullable<ReturnType<typeof rectOf>>;
  centerY: number;
  style: {
    display: string;
    overflow: string;
    position: string;
    visibility: string;
    pointerEvents: string;
    textOverflow: string;
    whiteSpace: string;
  };
  scrollWidth: number | null;
  clientWidth: number | null;
};

type ProbeChatScenarioReport = {
  id: string;
  pair: string;
  state: string;
  collapsed: boolean;
  rect: NonNullable<ReturnType<typeof rectOf>>;
  slots: Record<string, ProbeLayoutElement | null>;
  controls: Record<string, ProbeLayoutElement | null>;
  parts: Record<string, ProbeLayoutElement | null>;
  labels: Record<string, ProbeLayoutElement | null>;
  centerSpreadY: number | null;
  microphoneSendCenterDeltaY: number | null;
  iconChevronGaps: Record<string, number | null>;
  clipping: {
    strategyIcon: boolean;
    strategyChevron: boolean;
    modelIcon: boolean;
    modelChevron: boolean;
  };
  modelOutsideVisibleBoundary: boolean;
  strategyOutsideVisibleBoundary: boolean;
  accessibleNames: Record<string, { title: string | null; ariaLabel: string | null }>;
  pass: boolean;
  failures: string[];
};

type ProbeChatCoordinateStability = {
  pair: string;
  anchors: Record<string, number>;
  deltas: Record<string, number>;
  pass: boolean;
};

const CHAT_PROBE_SCENARIOS = [
  'auto-collapsed',
  'auto-sending',
  'auto-expanded',
  'cloud-collapsed',
  'cloud-expanded',
  'cloud-short',
  'cloud-single',
  'auto-image-disabled',
  'model-popup',
  'strategy-popup',
  'context-popup',
] as const;

const CHAT_PROBE_PROVIDER: IProvider = {
  id: FLOWY_BUILTIN_PROVIDER_ID,
  platform: 'flowy',
  name: 'Flowy',
  base_url: '',
  api_key: '',
  models: [
    'AIPC-auto-intelligence',
    'AIPC-auto-balance',
    'AIPC-auto-cost',
    'Deepseek-v4-flash',
    'Deepseek-v4-flash-vision-exp',
    'GLM-5',
  ],
};

const chatProbeOption = (
  model: string,
  family: ChatModelOption['family'],
  config: Partial<Pick<ChatModelOption, 'autoTier' | 'reasoningLevels' | 'supportsVision' | 'supportsTools'>> = {},
): ChatModelOption => ({
  key: `button-layout-probe:${model}`,
  provider: CHAT_PROBE_PROVIDER,
  model,
  label: model,
  family,
  reasoningLevels: [],
  supportsVision: false,
  supportsTools: true,
  ...config,
});

const CHAT_PROBE_PICKER: ChatModelPickerViewModel = {
  autoModels: [
    chatProbeOption('AIPC-auto-intelligence', 'auto', { autoTier: 'intelligence' }),
    chatProbeOption('AIPC-auto-balance', 'auto', { autoTier: 'balance' }),
    chatProbeOption('AIPC-auto-cost', 'auto', { autoTier: 'cost' }),
  ],
  cloudModels: [
    chatProbeOption('Deepseek-v4-flash', 'cloud', {
      reasoningLevels: ['low', 'medium', 'xhigh'],
    }),
    chatProbeOption('Deepseek-v4-flash-vision-exp', 'cloud', {
      reasoningLevels: ['low', 'medium', 'xhigh'],
      supportsVision: true,
    }),
    chatProbeOption('GLM-5', 'cloud', { reasoningLevels: ['medium'] }),
  ],
  otherProviderGroups: [],
};

const CHAT_PROBE_CLOUD_AUTH: CloudAuthContextValue = {
  ready: true,
  authState: { phase: 'authenticated', accountId: 'button-layout-probe' },
  status: 'authenticated',
  whoami: null,
  modelEnvironment: { phase: 'ready', usableModelCount: 0, canRetry: false },
  modelStatus: 'ready',
  modelError: null,
  refresh: async () => 'authenticated',
  retryModelEnvironment: async () => undefined,
  logout: async () => undefined,
};

const ProbeCloudAuthProvider: React.FC<React.PropsWithChildren> = ({ children }) => (
  <CloudAuthContext.Provider value={CHAT_PROBE_CLOUD_AUTH}>{children}</CloudAuthContext.Provider>
);

const ProbeGuidSubmitSurface: React.FC<{ sending?: boolean }> = ({ sending = false }) => (
  <ProbeCloudAuthProvider>
    <div
      className={`${guidStyles.actionSubmit} ${guidStyles.actionSubmitResponsive} button-layout-probe__guid-submit-surface`}
      data-guid-submit-probe-state={sending ? 'sending' : 'idle'}
    >
      <ComposerSubmitCluster
        hasDraft
        loading={sending}
        onSend={() => undefined}
        onSpeechTranscript={() => undefined}
        showStop={sending}
        onStop={() => undefined}
        sendTestId={`probe-guid-send-${sending ? 'sending' : 'idle'}`}
        stopTestId={`probe-guid-stop-${sending ? 'sending' : 'idle'}`}
      />
    </div>
  </ProbeCloudAuthProvider>
);

const chatProbeSelection = (model: string): NomiModelSelection => {
  const currentModel = { ...CHAT_PROBE_PROVIDER, use_model: model } as TProviderWithModel;
  return {
    current_model: currentModel,
    isCurrentModelAvailable: true,
    isModelCatalogLoading: false,
    refreshModelCatalog: () => undefined,
    providers: [CHAT_PROBE_PROVIDER],
    getAvailableModels: () => CHAT_PROBE_PROVIDER.models,
    handleSelectModel: async () => true,
    formatModelLabel: (_provider, modelName) => modelName ?? '',
    getDisplayModelName: (modelName) => modelName ?? '',
    modelPicker: CHAT_PROBE_PICKER,
  };
};

const readProbeParams = (): URLSearchParams => {
  const hashQuery = window.location.hash.split('?')[1] ?? '';
  const params = new URLSearchParams(window.location.search);
  new URLSearchParams(hashQuery).forEach((value, key) => params.set(key, value));
  return params;
};

const rectOf = (element: Element | null) => {
  if (!(element instanceof HTMLElement) && !(element instanceof SVGElement)) return null;
  const rect = element.getBoundingClientRect();
  return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
};

const visibleRect = (rect: ReturnType<typeof rectOf>): rect is NonNullable<ReturnType<typeof rectOf>> =>
  Boolean(rect && rect.width > 0 && rect.height > 0);

const rectOfText = (node: Text) => {
  const range = document.createRange();
  range.selectNodeContents(node);
  const rect = range.getBoundingClientRect();
  return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
};

const findLabelRect = (button: HTMLButtonElement): ReturnType<typeof rectOf> => {
  const marked = button.querySelector<HTMLElement>('[data-button-probe-label]');
  if (marked) return rectOf(marked);

  const walker = document.createTreeWalker(button, NodeFilter.SHOW_TEXT);
  let node = walker.nextNode();
  let firstTextRect: ReturnType<typeof rectOf> = null;
  while (node) {
    const text = node.textContent?.trim();
    const parent = node.parentElement;
    if (text && parent && !parent.closest('.i-icon, .arco-icon, svg, [data-button-probe-icon]')) {
      const textRect = rectOfText(node as Text);
      firstTextRect ??= textRect;
      if (visibleRect(textRect)) return textRect;
    }
    node = walker.nextNode();
  }
  return firstTextRect;
};

const markProductionButtons = () => {
  const targets: Array<{
    selector: string;
    id: string;
    labelRequired: boolean;
    iconOnly?: boolean;
    allowTruncate?: boolean;
  }> = [
    {
      selector: '[data-testid="nomi-model-selector"]',
      id: 'production-nomi-model-selector',
      labelRequired: false,
      allowTruncate: true,
    },
    { selector: '[data-testid^="agent-mode-selector"]', id: 'production-agent-mode-selector', labelRequired: false },
    {
      selector: '[data-testid="guid-model-selector"]',
      id: 'production-guid-model-selector',
      labelRequired: false,
      allowTruncate: true,
    },
    {
      selector: '[data-testid="reasoning-effort-compact-trigger"]',
      id: 'production-reasoning-effort',
      labelRequired: false,
    },
    { selector: '[data-testid="knowledge-detail-action-search"]', id: 'production-knowledge-search', labelRequired: true },
    { selector: '[data-testid="knowledge-detail-action-mount"]', id: 'production-knowledge-mount', labelRequired: true },
    {
      selector: '[data-testid="knowledge-detail-action-more"]',
      id: 'production-knowledge-more',
      iconOnly: true,
      labelRequired: false,
    },
  ];

  for (const target of targets) {
    document.querySelectorAll<HTMLElement>(target.selector).forEach((element) => {
      const button = element.matches('button') ? element : element.querySelector<HTMLButtonElement>('button');
      if (!button) return;
      button.dataset.buttonProbe = target.id;
      button.dataset.buttonProbeLabelRequired = String(target.labelRequired);
      if (target.iconOnly) button.dataset.buttonIconOnly = 'true';
      if (target.allowTruncate) button.dataset.buttonAllowTruncate = 'true';
    });
  }
};

const requiredProductionProbeIds = [
  'production-nomi-model-selector',
  'production-agent-mode-selector',
  'production-guid-model-selector',
  'production-reasoning-effort',
  'production-knowledge-search',
  'production-knowledge-mount',
  'production-knowledge-more',
] as const;

const layoutElementOf = (element: Element | null): ProbeLayoutElement | null => {
  const rect = rectOf(element);
  if (!element || !rect) return null;
  const style = getComputedStyle(element);
  const htmlElement = element instanceof HTMLElement ? element : null;
  return {
    rect,
    centerY: rect.y + rect.height / 2,
    style: {
      display: style.display,
      overflow: style.overflow,
      position: style.position,
      visibility: style.visibility,
      pointerEvents: style.pointerEvents,
      textOverflow: style.textOverflow,
      whiteSpace: style.whiteSpace,
    },
    scrollWidth: htmlElement?.scrollWidth ?? null,
    clientWidth: htmlElement?.clientWidth ?? null,
  };
};

const firstLayoutElement = (root: Element | null, selector: string): ProbeLayoutElement | null =>
  layoutElementOf(root?.querySelector(selector) ?? null);

const getControlElement = (slot: Element | null): Element | null => {
  if (!slot) return null;
  return (
    slot.querySelector('button') ??
    slot.querySelector("[data-static='true'] .sendbox-responsive-static-value") ??
    slot.querySelector("[data-static='true']")
  );
};

const partIsOutsideControl = (part: ProbeLayoutElement | null, control: ProbeLayoutElement | null): boolean => {
  if (!part || !control) return false;
  const tolerance = 0.5;
  return (
    part.rect.x < control.rect.x - tolerance ||
    part.rect.x + part.rect.width > control.rect.x + control.rect.width + tolerance ||
    part.rect.y < control.rect.y - tolerance ||
    part.rect.y + part.rect.height > control.rect.y + control.rect.height + tolerance
  );
};

const isHorizontalClip = (value: string): boolean =>
  value === 'hidden' || value === 'clip' || value === 'auto' || value === 'scroll' || value === 'overlay';

const visibleHorizontalBoundaryOf = (element: Element): { left: number; right: number } => {
  const visualViewport = window.visualViewport;
  let left = visualViewport?.offsetLeft ?? 0;
  let right = left + (visualViewport?.width ?? window.innerWidth);

  for (let ancestor = element.parentElement; ancestor && ancestor !== document.body; ancestor = ancestor.parentElement) {
    const style = getComputedStyle(ancestor);
    if (!isHorizontalClip(style.overflowX) && !isHorizontalClip(style.overflowY)) continue;
    const rect = ancestor.getBoundingClientRect();
    left = Math.max(left, rect.left);
    right = Math.min(right, rect.right);
  }

  return { left: left + 12, right: right - 12 };
};

const isOutsideVisibleHorizontalBoundary = (element: Element | null): boolean => {
  if (!element) return false;
  const rect = element.getBoundingClientRect();
  const boundary = visibleHorizontalBoundaryOf(element);
  return rect.left < boundary.left - 0.5 || rect.right > boundary.right + 0.5;
};

const measureChatScenario = (row: HTMLElement): ProbeChatScenarioReport => {
  const pair = row.dataset.chatProbePair ?? row.dataset.chatProbeId ?? 'unknown';
  const state = row.dataset.chatProbeState ?? 'unknown';
  const collapsed = row.dataset.chatProbeCollapsed === 'true';
  const rowRect = rectOf(row) ?? { x: 0, y: 0, width: 0, height: 0 };
  const slots = Object.fromEntries(
    ['strategy', 'model', 'context', 'submit'].map((name) => [
      name,
      layoutElementOf(row.querySelector(`[data-layout-slot='${name}']`)),
    ]),
  );
  const strategySlot = row.querySelector("[data-layout-slot='strategy']");
  const modelSlot = row.querySelector("[data-layout-slot='model']");
  const contextSlot = row.querySelector("[data-layout-slot='context']");
  const strategyControlElement = getControlElement(strategySlot);
  const modelControlElement = getControlElement(modelSlot);
  const strategySlotStyle = strategySlot ? getComputedStyle(strategySlot) : null;
  const controls = {
    strategy: layoutElementOf(strategyControlElement),
    model: layoutElementOf(modelControlElement),
    context: firstLayoutElement(contextSlot, "[data-layout-part='context-ring']"),
    microphone: layoutElementOf(row.querySelector("[data-layout-part='microphone']")),
    send: layoutElementOf(row.querySelector("[data-layout-part='send']")),
  };
  const parts = {
    strategyLeading: firstLayoutElement(strategySlot, "[data-layout-part='leading-icon']"),
    strategyChevron: firstLayoutElement(strategySlot, "[data-layout-part='chevron']"),
    modelLeading: firstLayoutElement(modelSlot, "[data-layout-part='leading-icon']"),
    modelChevron: firstLayoutElement(modelSlot, "[data-layout-part='chevron']"),
    contextRing: controls.context,
    microphone: controls.microphone,
    send: controls.send,
  };
  const labels = {
    strategy: layoutElementOf(
      strategySlot?.querySelector('.sendbox-responsive-label, .sendbox-responsive-static-value > span:last-child') ?? null,
    ),
    model: firstLayoutElement(modelSlot, '.sendbox-responsive-label'),
  };
  const centerCandidates = [
    slots.strategy,
    slots.model,
    slots.context,
    controls.microphone,
    controls.send,
  ].filter((value): value is ProbeLayoutElement => Boolean(value));
  const centerValues = centerCandidates.map((value) => value.centerY);
  const centerSpreadY = centerValues.length > 1 ? Math.max(...centerValues) - Math.min(...centerValues) : null;
  const microphoneSendCenterDeltaY =
    controls.microphone && controls.send ? controls.microphone.centerY - controls.send.centerY : null;
  const iconChevronGaps = {
    strategy:
      parts.strategyLeading && parts.strategyChevron && !visibleRect(labels.strategy?.rect ?? null)
        ? parts.strategyChevron.rect.x - (parts.strategyLeading.rect.x + parts.strategyLeading.rect.width)
        : null,
    model:
      parts.modelLeading && parts.modelChevron && !visibleRect(labels.model?.rect ?? null)
        ? parts.modelChevron.rect.x - (parts.modelLeading.rect.x + parts.modelLeading.rect.width)
        : null,
  };
  const clipping = {
    strategyIcon: partIsOutsideControl(parts.strategyLeading, controls.strategy),
    strategyChevron: partIsOutsideControl(parts.strategyChevron, controls.strategy),
    modelIcon: partIsOutsideControl(parts.modelLeading, controls.model),
    modelChevron: partIsOutsideControl(parts.modelChevron, controls.model),
  };
  const accessibleNames = Object.fromEntries(
    ['strategy', 'model'].map((name) => {
      const element = name === 'strategy' ? strategyControlElement : modelControlElement;
      return [
        name,
        {
          title: element?.getAttribute('title') ?? null,
          ariaLabel: element?.getAttribute('aria-label') ?? null,
        },
      ];
    }),
  );
  const failures: string[] = [];
  const expectChevron = row.dataset.chatProbeStrategy !== 'single';
  const modelOverlayOpen = state === 'expanded' || state === 'model-popup';
  const strategyOverlayOpen = state === 'expanded' || state === 'strategy-popup';
  const modelOutsideVisibleBoundary = modelOverlayOpen && isOutsideVisibleHorizontalBoundary(modelControlElement);
  const strategyOutsideVisibleBoundary =
    strategyOverlayOpen && row.dataset.chatProbeStrategy !== 'single' && isOutsideVisibleHorizontalBoundary(strategyControlElement);
  const within = (value: number | undefined, expected: number, tolerance = 0.5) =>
    value !== undefined && Math.abs(value - expected) <= tolerance;

  for (const [name, expectedHeight] of [
    ['strategy', 28],
    ['model', 28],
    ['context', 28],
  ] as const) {
    const measured = slots[name]?.rect.height;
    if (measured === undefined || !within(measured, expectedHeight)) {
      failures.push(`${name}-slot-height=${measured?.toFixed(2) ?? 'missing'}`);
    }
  }

  if (controls.microphone && !within(controls.microphone.rect.width, 26)) {
    failures.push(`microphone-width=${controls.microphone.rect.width.toFixed(2)}`);
  }
  if (controls.send && !within(controls.send.rect.width, 26)) {
    failures.push(`send-width=${controls.send.rect.width.toFixed(2)}`);
  }
  const expectedStrategyWidth =
    row.dataset.chatProbeStrategy === 'single' || modelOverlayOpen ? 28 : strategyOverlayOpen ? 108 : 28;
  const expectedModelWidth = modelOverlayOpen ? 176 : 28;
  if (!modelOverlayOpen && controls.strategy && !within(controls.strategy.rect.width, expectedStrategyWidth)) {
    failures.push(`strategy-width=${controls.strategy.rect.width.toFixed(2)} expected=${expectedStrategyWidth}`);
  }
  if (controls.model && !within(controls.model.rect.width, expectedModelWidth)) {
    failures.push(`model-width=${controls.model.rect.width.toFixed(2)} expected=${expectedModelWidth}`);
  }
  if (centerSpreadY !== null && centerSpreadY > 0.5) {
    failures.push(`center-spread-y=${centerSpreadY.toFixed(2)}`);
  }
  if (microphoneSendCenterDeltaY !== null && Math.abs(microphoneSendCenterDeltaY) > 0.25) {
    failures.push(`microphone-send-center-delta-y=${microphoneSendCenterDeltaY.toFixed(2)}`);
  }
  if (modelOverlayOpen) {
    if (strategySlotStyle?.visibility !== 'hidden') failures.push('strategy-not-hidden-behind-model');
    if (strategySlotStyle?.pointerEvents !== 'none') failures.push('strategy-hit-target-not-disabled');
    if (modelOutsideVisibleBoundary) failures.push('model-out-of-visible-boundary');
  }
  if (strategyOutsideVisibleBoundary) failures.push('strategy-out-of-visible-boundary');
  if (!modelOverlayOpen && strategySlotStyle?.visibility === 'hidden') {
    failures.push('strategy-hidden-without-model-overlay');
  }
  if (expectChevron && !modelOverlayOpen && !visibleRect(parts.strategyChevron?.rect ?? null)) {
    failures.push('strategy-chevron-not-visible');
  }
  if (!modelOverlayOpen && !visibleRect(parts.modelChevron?.rect ?? null)) {
    failures.push('model-chevron-not-visible');
  }
  if (!expectChevron && parts.strategyChevron) failures.push('single-strategy-has-chevron');
  for (const [name, gap] of Object.entries(iconChevronGaps)) {
    if (gap !== null && Math.abs(gap - 2) > 1) failures.push(`${name}-icon-chevron-gap=${gap.toFixed(2)}`);
  }
  if (clipping.strategyIcon) failures.push('strategy-icon-clipped');
  if (clipping.strategyChevron) failures.push('strategy-chevron-clipped');
  if (clipping.modelIcon) failures.push('model-icon-clipped');
  if (clipping.modelChevron) failures.push('model-chevron-clipped');
  if (collapsed && controls.model?.style.overflow === 'hidden') failures.push('collapsed-model-overflow-hidden');
  if (collapsed && parts.modelLeading && !controls.model) failures.push('missing-model-control');
  for (const name of ['strategy', 'model']) {
    const names = accessibleNames[name];
    if (!names.title || !names.ariaLabel) failures.push(`${name}-missing-accessible-name`);
  }

  return {
    id: row.dataset.chatProbeId ?? 'unknown',
    pair,
    state,
    collapsed,
    rect: rowRect,
    slots,
    controls,
    parts,
    labels,
    centerSpreadY,
    microphoneSendCenterDeltaY,
    iconChevronGaps,
    clipping,
    modelOutsideVisibleBoundary,
    strategyOutsideVisibleBoundary,
    accessibleNames,
    pass: failures.length === 0,
    failures,
  };
};

const measureChatScenarios = () =>
  Array.from(document.querySelectorAll<HTMLElement>('[data-chat-probe-id]')).map(measureChatScenario);

const measureChatCoordinateStability = (
  scenarios: ProbeChatScenarioReport[],
): ProbeChatCoordinateStability[] => {
  const anchors = ['context', 'microphone', 'send'] as const;
  const grouped = new Map<string, ProbeChatScenarioReport[]>();
  scenarios.forEach((scenario) => {
    const group = grouped.get(scenario.pair) ?? [];
    group.push(scenario);
    grouped.set(scenario.pair, group);
  });

  return [...grouped.entries()]
    .filter(([, group]) => group.length > 1)
    .map(([pair, group]) => {
      const values = Object.fromEntries(
        anchors.map((anchor) => [
          anchor,
          group
            .map((scenario) => {
              const element = scenario.controls[anchor];
              return element ? element.rect.x - scenario.rect.x : undefined;
            })
            .filter((value): value is number => typeof value === 'number'),
        ]),
      ) as Record<(typeof anchors)[number], number[]>;
      const deltas = Object.fromEntries(
        anchors.map((anchor) => [
          anchor,
          values[anchor].length > 1 ? Math.max(...values[anchor]) - Math.min(...values[anchor]) : 0,
        ]),
      ) as Record<(typeof anchors)[number], number>;
      return {
        pair,
        anchors: Object.fromEntries(anchors.map((anchor) => [anchor, values[anchor][0] ?? 0])),
        deltas,
        pass: anchors.every((anchor) => deltas[anchor] <= 0.5),
      };
    });
};

const collectProbeReport = (theme: string, locale: string): ProbeReport => {
  const buttons = Array.from(document.querySelectorAll<HTMLButtonElement>('button[data-button-probe]')).map((button) => {
    const id = button.dataset.buttonProbe ?? 'unknown';
    const style = getComputedStyle(button);
    const icon = button.querySelector<HTMLElement>('[data-button-probe-icon], .i-icon, .arco-icon, svg');
    const labelRect = findLabelRect(button);
    const buttonRect = button.getBoundingClientRect();
    const iconRect = rectOf(icon);
    const iconVisible = visibleRect(iconRect);
    const labelVisible = visibleRect(labelRect);
    const centerDeltaY = iconVisible && labelVisible
      ? Math.abs(iconRect.y + iconRect.height / 2 - (labelRect.y + labelRect.height / 2))
      : null;
    const iconOnly = button.dataset.buttonIconOnly === 'true';
    const labelRequired = button.dataset.buttonProbeLabelRequired !== 'false';
    const circle = style.borderRadius === '50%' || button.classList.contains('arco-btn-shape-circle');
    const outOfViewport =
      buttonRect.left < -0.5 ||
      buttonRect.top < -0.5 ||
      buttonRect.right > window.innerWidth + 0.5 ||
      buttonRect.bottom > window.innerHeight + 0.5;
    const clipped =
      !button.dataset.buttonAllowTruncate &&
      (button.scrollWidth > button.clientWidth + 1 || button.scrollHeight > button.clientHeight + 1);
    const displaySources: Array<{ selector: string; value: string; important: string }> = [];
    const inspectRules = (rules: CSSRuleList) => {
      Array.from(rules).forEach((rule) => {
        if ('cssRules' in rule) {
          const nestedRules = (rule as CSSGroupingRule).cssRules;
          if (nestedRules) inspectRules(nestedRules);
        }
        const styleRule = rule as CSSStyleRule;
        if (styleRule.type === CSSRule.STYLE_RULE && styleRule.selectorText && styleRule.style.display) {
          try {
            if (button.matches(styleRule.selectorText)) {
              displaySources.push({
                selector: styleRule.selectorText,
                value: styleRule.style.display,
                important: styleRule.style.getPropertyPriority('display'),
              });
            }
          } catch {
            // Ignore selectors unsupported by the probe browser.
          }
        }
      });
    };
    Array.from(document.styleSheets).forEach((sheet) => {
      try {
        if (sheet.cssRules) inspectRules(sheet.cssRules);
      } catch {
        // Cross-origin sheets are not inspectable; the report still includes
        // computed styles and head order for those environments.
      }
    });
    const failures: string[] = [];

    const winningDisplaySource = displaySources.at(-1);
    // Edge 139 reports an inline-level flex box as `flex` in this headless
    // configuration even though the winning stylesheet declaration is
    // `inline-flex !important`. Treat that serialization as equivalent only
    // when our contract is demonstrably the winning source.
    const displayPass =
      style.display === 'inline-flex' ||
      (style.display === 'flex' && winningDisplaySource?.value === 'inline-flex');
    if (!iconOnly && !displayPass) failures.push(`display=${style.display}`);
    if (!iconOnly && style.flexDirection !== 'row') failures.push(`flex-direction=${style.flexDirection}`);
    if (!iconOnly && style.alignItems !== 'center') failures.push(`align-items=${style.alignItems}`);
    if (!iconOnly && style.whiteSpace !== 'nowrap') failures.push(`white-space=${style.whiteSpace}`);
    if (!iconOnly && style.writingMode !== 'horizontal-tb') failures.push(`writing-mode=${style.writingMode}`);
    if (!iconOnly && (buttonRect.width <= 0 || buttonRect.height <= 0)) failures.push('button-not-visible');
    if (!iconOnly && buttonRect.height > 80) failures.push(`button-height=${buttonRect.height.toFixed(2)}`);
    if (!iconOnly && labelRequired && !labelVisible) failures.push('label-not-visible');
    if (!iconOnly && labelRequired && !iconVisible) failures.push('icon-not-visible');
    if (!iconOnly && labelRequired && centerDeltaY === null) failures.push('missing-center-measurement');
    if (!iconOnly && centerDeltaY !== null && centerDeltaY > 2.5) {
      failures.push(`center-delta-y=${centerDeltaY.toFixed(2)}`);
    }
    if (clipped) failures.push('clipped-or-overflowed');
    if (iconOnly && circle && Math.abs(buttonRect.width - buttonRect.height) > 1) {
      failures.push(`circle-size=${buttonRect.width.toFixed(2)}x${buttonRect.height.toFixed(2)}`);
    }

    return {
      id,
      label: button.textContent?.trim() ?? '',
      style: {
        display: style.display,
        flexDirection: style.flexDirection,
        alignItems: style.alignItems,
        justifyContent: style.justifyContent,
        whiteSpace: style.whiteSpace,
        writingMode: style.writingMode,
      },
      rect: { x: buttonRect.x, y: buttonRect.y, width: buttonRect.width, height: buttonRect.height },
      iconRect,
      labelRect,
      centerDeltaY,
      visible: buttonRect.width > 0 && buttonRect.height > 0,
      clipped,
      outOfViewport,
      iconOnly,
      circle,
      displaySources,
      pass: failures.length === 0,
      failures,
    } satisfies ProbeButtonReport;
  });

  const presentIds = new Set(buttons.map((button) => button.id));
  const missingFixtures = requiredProductionProbeIds.filter((id) => !presentIds.has(id));
  const chatScenarios = measureChatScenarios();
  const chatCoordinateStability = measureChatCoordinateStability(chatScenarios);
  const missingChatFixtures = CHAT_PROBE_SCENARIOS.filter(
    (id) => !chatScenarios.some((scenario) => scenario.id === id),
  );

  return {
    ok:
      buttons.length > 0 &&
      missingFixtures.length === 0 &&
      buttons.every((button) => button.pass) &&
      missingChatFixtures.length === 0 &&
      chatScenarios.every((scenario) => scenario.pass) &&
      chatCoordinateStability.every((scenario) => scenario.pass),
    missingFixtures,
    missingChatFixtures,
    viewport: { width: window.innerWidth, height: window.innerHeight, devicePixelRatio: window.devicePixelRatio },
    theme,
    locale,
    userAgent: navigator.userAgent,
    styleOrder: Array.from(document.head.children)
      .filter((element) => element.tagName === 'STYLE' || element.tagName === 'LINK')
      .map((element) => element.id || element.getAttribute('href') || element.tagName.toLowerCase()),
    buttons,
    chatScenarios,
    chatCoordinateStability,
  };
};

const copy = {
  zh: {
    title: '按钮布局回归探针',
    direct: '检索',
    primary: '挂载到会话',
    long: '打开并挂载到当前会话',
    model: 'Qwen3.7-plus',
    mode: '普通模式',
    reasoning: '中等',
    disabled: '禁用操作',
    loading: '加载中',
    send: '发送',
    more: '更多',
  },
  en: {
    title: 'Button layout probe',
    direct: 'Search',
    primary: 'Mount to session',
    long: 'Open and mount to current session',
    model: 'Qwen3.7-plus',
    mode: 'Standard mode',
    reasoning: 'Medium',
    disabled: 'Disabled action',
    loading: 'Loading',
    send: 'Send',
    more: 'More',
  },
} as const;

type ProbeChatControlRowProps = {
  id: (typeof CHAT_PROBE_SCENARIOS)[number];
  pair: string;
  state: 'collapsed' | 'expanded' | 'model-popup' | 'strategy-popup' | 'context-popup';
  strategy: 'auto' | 'cloud' | 'single';
  model: string;
  hasImageAttachments?: boolean;
  sending?: boolean;
};

const ProbeChatControlRow: React.FC<ProbeChatControlRowProps> = ({
  id,
  pair,
  state,
  strategy,
  model,
  hasImageAttachments = false,
  sending = false,
}) => {
  const modelOverlayOpen = state === 'expanded' || state === 'model-popup';
  const strategyOverlayOpen = state === 'expanded' || state === 'strategy-popup';
  const expanded = modelOverlayOpen || strategyOverlayOpen;
  const strategyNode =
    strategy === 'auto' ? (
      <AutoTierSelector
        options={CHAT_PROBE_PICKER.autoModels}
        selected={CHAT_PROBE_PICKER.autoModels[1]}
        hasImageAttachments={hasImageAttachments}
        popupVisible={state === 'strategy-popup'}
        onPopupVisibleChange={() => undefined}
        className={strategyOverlayOpen ? 'sendbox-responsive-control-open' : undefined}
        onSelect={() => undefined}
      />
    ) : (
      <ReasoningEffortSelector
        levels={strategy === 'single' ? ['medium'] : ['low', 'medium', 'xhigh']}
        modelKey={`button-layout-probe:${model}`}
        initialEffort='medium'
        popupVisible={strategy !== 'single' && state === 'strategy-popup'}
        onPopupVisibleChange={() => undefined}
        className={strategyOverlayOpen && strategy !== 'single' ? 'sendbox-responsive-control-open' : undefined}
        onEffortChanged={() => undefined}
      />
    );

  return (
    <ProbeCloudAuthProvider>
      <div
        className={`button-layout-probe__chat-row sendbox-actions sendbox-actions--nomi ${expanded ? 'button-layout-probe__chat-row--expanded' : ''}`}
        data-chat-probe-id={id}
        data-chat-probe-pair={pair}
        data-chat-probe-state={sending ? 'sending' : state}
        data-chat-probe-collapsed={state === 'collapsed' ? 'true' : 'false'}
        data-chat-probe-strategy={strategy}
      >
        <div className='sendbox-responsive-config-group chat-model-picker-config-group'>
          <div className='sendbox-strategy-slot' data-layout-slot='strategy'>
            {strategyNode}
          </div>
          <div className='chat-model-picker-slot' data-layout-slot='model'>
            <NomiModelSelector
              selection={chatProbeSelection(model)}
              hasImageAttachments={hasImageAttachments}
              popupVisible={state === 'model-popup'}
              onPopupVisibleChange={() => undefined}
              className={modelOverlayOpen ? 'sendbox-responsive-control-open' : undefined}
            />
          </div>
          <div className='nomi-context-usage-slot' data-layout-slot='context'>
            <ContextUsageRing
              used={4200}
              max={16000}
              popupVisible={state === 'context-popup'}
              onPopupVisibleChange={() => undefined}
            />
          </div>
        </div>
        <ComposerSubmitCluster
          hasDraft
          loading={sending}
          onSend={() => undefined}
          onSpeechTranscript={() => undefined}
          showStop={sending}
          onStop={() => undefined}
          sendTestId={`probe-send-${id}`}
          stopTestId={`probe-stop-${id}`}
        />
      </div>
    </ProbeCloudAuthProvider>
  );
};

const ButtonLayoutProbe: React.FC = () => {
  const params = useMemo(readProbeParams, []);
  const themeId = params.get('theme') ?? 'codex-neutral';
  const locale = params.get('locale') === 'en-US' ? 'en' : 'zh';
  const scenario = params.get('scenario') ?? 'normal';
  const text = copy[locale];
  const [report, setReport] = useState<ProbeReport | null>(null);

  useEffect(() => {
    const scheme = params.get('scheme') === 'dark' ? 'dark' : 'light';
    const preset = PRESET_THEMES.find((item) => item.id === themeId);
    document.documentElement.setAttribute('data-theme', scheme);
    document.body.setAttribute('arco-theme', scheme);
    document.body.dataset.buttonProbe = 'true';

    const customStyleId = 'button-layout-probe-user-css';
    document.getElementById(customStyleId)?.remove();
    const customStyle = document.createElement('style');
    customStyle.id = customStyleId;
    const adversarialCss =
      scenario === 'adversarial'
        ? '\nbutton.flowy-icon-text-btn { display: flex; flex-direction: column; }\n'
        : '';
    if (scenario !== 'off') {
      customStyle.textContent = processCustomCss(`${preset?.css ?? ''}${adversarialCss}`);
      document.head.appendChild(customStyle);
    }

    // The real shell performs this after user CSS. The probe deliberately uses
    // the same ordering so a hostile theme rule is a red-capable regression.
    ensureThemeControlContract();

    const timer = window.setTimeout(() => {
      markProductionButtons();
      setReport(collectProbeReport(themeId, locale === 'zh' ? 'zh-CN' : 'en-US'));
    }, 80);

    return () => {
      window.clearTimeout(timer);
      document.getElementById(customStyleId)?.remove();
      document.body.removeAttribute('data-button-probe');
    };
  }, [locale, params, scenario, themeId]);

  return (
    <main className='button-layout-probe' data-testid='button-layout-probe'>
      <header className='button-layout-probe__header'>
        <h1>{text.title}</h1>
        <p>
          {themeId} · {locale === 'zh' ? '中文' : 'English'} · {scenario}
        </p>
      </header>

      <section className='button-layout-probe__grid' aria-label='button fixtures'>
        <article className='button-layout-probe__card'>
          <h2>Arco icon prop</h2>
          <div className='button-layout-probe__actions'>
            <Button
              data-button-probe='arco-icon-prop-search'
              className='flowy-icon-text-btn'
              icon={<Search theme='outline' size='14' />}
            >
              {text.direct}
            </Button>
            <Button
              data-button-probe='arco-icon-prop-mount'
              className='flowy-icon-text-btn'
              type='primary'
              icon={<LinkOne theme='outline' size='14' />}
            >
              {text.primary}
            </Button>
            <Button
              data-button-probe='arco-icon-prop-long'
              className='flowy-icon-text-btn'
              type='primary'
              icon={<LinkOne theme='outline' size='14' />}
            >
              {text.long}
            </Button>
          </div>
        </article>

        <article className='button-layout-probe__card'>
          <h2>Production selector surfaces</h2>
          <PreviewProvider persistNamespace='button-layout-probe' subscribeGlobalOpen={false}>
            <LayoutContext.Provider value={{ isMobile: false, siderCollapsed: false, setSiderCollapsed: () => undefined }}>
              <div className='button-layout-probe__production-stack'>
                <section className='button-layout-probe__chat-scenarios' aria-label='chat control geometry'>
                  <h3>Chat control geometry</h3>
                  <ProbeChatControlRow
                    id='auto-collapsed'
                    pair='auto'
                    state='collapsed'
                    strategy='auto'
                    model='AIPC-auto-balance'
                  />
                  <ProbeChatControlRow
                    id='auto-sending'
                    pair='auto'
                    state='collapsed'
                    strategy='auto'
                    model='AIPC-auto-balance'
                    sending
                  />
                  <ProbeChatControlRow
                    id='auto-expanded'
                    pair='auto'
                    state='expanded'
                    strategy='auto'
                    model='AIPC-auto-balance'
                  />
                  <ProbeChatControlRow
                    id='cloud-collapsed'
                    pair='cloud'
                    state='collapsed'
                    strategy='cloud'
                    model='Deepseek-v4-flash-vision-exp'
                  />
                  <ProbeChatControlRow
                    id='cloud-expanded'
                    pair='cloud'
                    state='expanded'
                    strategy='cloud'
                    model='Deepseek-v4-flash-vision-exp'
                  />
                  <ProbeChatControlRow
                    id='cloud-short'
                    pair='cloud-short'
                    state='collapsed'
                    strategy='cloud'
                    model='Deepseek-v4-flash'
                  />
                  <ProbeChatControlRow
                    id='cloud-single'
                    pair='cloud-single'
                    state='collapsed'
                    strategy='single'
                    model='GLM-5'
                  />
                  <ProbeChatControlRow
                    id='auto-image-disabled'
                    pair='auto-image-disabled'
                    state='collapsed'
                    strategy='auto'
                    model='AIPC-auto-balance'
                    hasImageAttachments
                  />
                  <ProbeChatControlRow
                    id='model-popup'
                    pair='model-popup'
                    state='model-popup'
                    strategy='cloud'
                    model='Deepseek-v4-flash-vision-exp'
                  />
                  <ProbeChatControlRow
                    id='strategy-popup'
                    pair='strategy-popup'
                    state='strategy-popup'
                    strategy='auto'
                    model='AIPC-auto-balance'
                  />
                  <ProbeChatControlRow
                    id='context-popup'
                    pair='context-popup'
                    state='context-popup'
                    strategy='cloud'
                    model='Deepseek-v4-flash'
                  />
                </section>
                <section className='button-layout-probe__chat-scenarios' aria-label='guid submit geometry'>
                  <h3>Guid submit geometry</h3>
                  <ProbeGuidSubmitSurface />
                  <ProbeGuidSubmitSurface sending />
                </section>
                <div className='button-layout-probe__production-sendbox sendbox-actions'>
                  <div className='sendbox-responsive-config-group'>
                    <NomiModelSelector compact selection={undefined} />
                    <AgentModeSelector
                      backend='codex'
                      compact
                      compactLabelOverride={text.mode}
                      compactLeadingIcon={<Brain theme='outline' size='14' />}
                      dynamicModes={[{ value: 'default', label: text.mode }]}
                      onModeSelect={() => undefined}
                    />
                    <ReasoningEffortSelector levels={['low', 'medium', 'high']} initialEffort='medium' />
                  </div>
                </div>
                <div
                  className={`${guidStyles.actionConfigGroup} ${guidStyles.actionConfigGroupResponsive} button-layout-probe__production-guid`}
                >
                  <GuidModelSelectorButton label={text.model} />
                </div>
                <KnowledgeDetailActionBar
                  labels={{
                    search: text.direct,
                    mountToSession: text.primary,
                    export: text.more,
                    openFolder: text.more,
                    delete: text.more,
                    more: text.more,
                  }}
                  onSearch={() => undefined}
                  onMountToSession={() => undefined}
                  onExport={() => undefined}
                  onOpenFolder={() => undefined}
                  onDelete={() => undefined}
                />
              </div>
            </LayoutContext.Provider>
          </PreviewProvider>
        </article>

        <article className='button-layout-probe__card'>
          <h2>State and icon-only controls</h2>
          <div className='button-layout-probe__actions'>
            <Button
              data-button-probe='disabled-icon-text'
              className='flowy-icon-text-btn'
              disabled
              icon={<Search theme='outline' size='14' />}
            >
              {text.disabled}
            </Button>
            <Button
              data-button-probe='loading-icon-text'
              className='flowy-icon-text-btn'
              loading
              icon={<Search theme='outline' size='14' />}
            >
              {text.loading}
            </Button>
            <Button
              data-button-probe='send-circle'
              data-button-icon-only='true'
              shape='circle'
              className='send-button-custom'
              aria-label={text.send}
              icon={<ArrowUp theme='outline' size='16' />}
            />
            <Button
              data-button-probe='more-circle'
              data-button-icon-only='true'
              shape='circle'
              aria-label={text.more}
              icon={<More theme='outline' size='16' />}
            />
          </div>
        </article>
      </section>

      <pre id='button-layout-probe-result' data-ready={report ? 'true' : 'false'}>
        {report ? JSON.stringify(report) : JSON.stringify({ ready: false })}
      </pre>
    </main>
  );
};

export default ButtonLayoutProbe;
