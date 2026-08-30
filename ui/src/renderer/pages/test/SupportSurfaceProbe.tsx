import { useEffect, useMemo, useState } from 'react';
import type { ICloudImConversation, ICloudImMessage } from '@/common/adapter/ipcBridge';
import type { AgentStreamErrorInfo } from '@/common/chat/chatLib';
import { CloudAuthContext, type CloudAuthContextValue } from '@renderer/hooks/context/CloudAuthContext';
import { ThemeContext, type ThemeContextValue } from '@renderer/hooks/context/ThemeContext';
import { PRESET_THEMES } from '@renderer/pages/settings/DisplaySettings/presets';
import { processCustomCss } from '@renderer/utils/theme/customCssProcessor';
import { ensureThemeControlContract } from '@renderer/utils/theme/themeControlContract';
import { useTranslation } from 'react-i18next';
import ConversationErrorReportModal from '@renderer/features/supportChat/components/ConversationErrorReportModal';
import {
  SupportChatModalView,
  type SupportChatModalViewProps,
} from '@renderer/features/supportChat/components/SupportChatModal';
import type { SupportChatState, SupportMessage } from '@renderer/features/supportChat/api/supportChatTypes';
import './supportSurfaceProbe.css';

type ProbeReport = {
  ok: boolean;
  surface: 'feedback' | 'support';
  scenario: string;
  viewport: { width: number; height: number; devicePixelRatio: number };
  theme: string;
  scheme: string;
  locale: string;
  modal: {
    width: number;
    height: number;
    top: number;
    bottom: number;
    horizontalOverflow: boolean;
    headerVisible: boolean;
    footerVisible: boolean;
    scrollOwnerCount: number;
  };
  controls: Array<{
    label: string;
    width: number;
    height: number;
    disabled: boolean;
    iconCenterDeltaY: number | null;
    transform: string;
    focusVisible: boolean;
    pass: boolean;
  }>;
  focusableControlCount: number;
  focusVisibleControlCount: number;
  contrastChecks: Array<{ label: string; ratio: number | null; pass: boolean }>;
  screenshotPreviewCount: number;
  screenshotPreviewSize: { width: number; height: number } | null;
  expectedStateVisible: boolean;
  failures: string[];
};

const PROBE_ACCOUNT = 'support-surface-probe';
const PROBE_CONVERSATION_ID = 9001;
const PROBE_NOW = '2026-08-28T10:00:00.000Z';

const PROBE_AUTH: CloudAuthContextValue = {
  ready: true,
  authState: { phase: 'authenticated', accountId: PROBE_ACCOUNT },
  status: 'authenticated',
  whoami: null,
  modelEnvironment: { phase: 'ready', usableModelCount: 0, canRetry: false },
  modelStatus: 'ready',
  modelError: null,
  refresh: async () => 'authenticated',
  retryModelEnvironment: async () => undefined,
  logout: async () => undefined,
};

const makeMessage = (
  seq: number,
  content: string,
  senderType: 'sys_user' | 'user' = 'sys_user',
  msgType = 'text'
): ICloudImMessage => ({
  id: 10000 + seq,
  conversationId: PROBE_CONVERSATION_ID,
  seq,
  clientMsgId: `support-surface-probe-${seq}`,
  senderType,
  senderId: senderType === 'sys_user' ? 7 : 1,
  msgType,
  content,
  status: 'sent',
  createdAt: new Date(Date.parse(PROBE_NOW) + seq * 60_000).toISOString(),
});

const makeConversation = (): ICloudImConversation => ({
  id: PROBE_CONVERSATION_ID,
  userId: 1,
  externalChannelCode: 'probe',
  app: 'flowymes',
  status: 'open',
  lastSeq: 3,
  lastMessageId: 10003,
  lastMessageAt: PROBE_NOW,
  lastMessagePreview: 'probe',
  lastSenderType: 'sys_user',
  userUnreadCount: 0,
  opsUnreadCount: 0,
  hasUnread: false,
  createdAt: PROBE_NOW,
  updatedAt: PROBE_NOW,
});

const longDetail = Array.from(
  { length: 24 },
  (_, index) => `detail line ${index + 1}: provider response remained unavailable during the probe.`
).join('\n');

const readProbeParams = (): URLSearchParams => {
  const params = new URLSearchParams(window.location.search);
  const hashQuery = window.location.hash.split('?')[1] ?? '';
  new URLSearchParams(hashQuery).forEach((value, key) => params.set(key, value));
  return params;
};

const rectOf = (element: Element | null): DOMRect | null =>
  element instanceof HTMLElement || element instanceof SVGElement ? element.getBoundingClientRect() : null;

const visibleRect = (rect: DOMRect | null): boolean => Boolean(rect && rect.width > 0 && rect.height > 0);

type ProbeColor = { r: number; g: number; b: number; a: number };

const clampColorChannel = (value: number): number => Math.min(255, Math.max(0, value));

const parseCssColor = (value: string): ProbeColor | null => {
  const normalized = value.trim().toLowerCase();
  if (normalized === 'transparent') return { r: 0, g: 0, b: 0, a: 0 };

  if (normalized.startsWith('#')) {
    const hex = normalized.slice(1);
    if (hex.length === 3 || hex.length === 4) {
      const channels = Array.from(hex, (part) => Number.parseInt(`${part}${part}`, 16));
      return {
        r: channels[0],
        g: channels[1],
        b: channels[2],
        a: channels[3] === undefined ? 1 : channels[3] / 255,
      };
    }
    if (hex.length === 6 || hex.length === 8) {
      const channels = hex.match(/.{2}/g)?.map((part) => Number.parseInt(part, 16));
      if (channels?.length === 3 || channels?.length === 4) {
        return {
          r: channels[0],
          g: channels[1],
          b: channels[2],
          a: channels[3] === undefined ? 1 : channels[3] / 255,
        };
      }
    }
    return null;
  }

  const numeric = normalized.match(/[-+]?(?:\d*\.\d+|\d+\.?\d*)%?/g);
  if (normalized.startsWith('color(srgb') && numeric && numeric.length >= 3) {
    return {
      r: clampColorChannel(Number(numeric[0]) * 255),
      g: clampColorChannel(Number(numeric[1]) * 255),
      b: clampColorChannel(Number(numeric[2]) * 255),
      a: numeric[3] ? Number(numeric[3]) : 1,
    };
  }
  if ((normalized.startsWith('rgb(') || normalized.startsWith('rgba(')) && numeric && numeric.length >= 3) {
    const channel = (part: string) =>
      part.endsWith('%') ? (Number(part.slice(0, -1)) / 100) * 255 : Number(part);
    const alpha = numeric[3]
      ? numeric[3].endsWith('%')
        ? Number(numeric[3].slice(0, -1)) / 100
        : Number(numeric[3])
      : 1;
    return {
      r: clampColorChannel(channel(numeric[0])),
      g: clampColorChannel(channel(numeric[1])),
      b: clampColorChannel(channel(numeric[2])),
      a: Math.min(1, Math.max(0, alpha)),
    };
  }
  return null;
};

const compositeColor = (foreground: ProbeColor, background: ProbeColor): ProbeColor => {
  const alpha = Math.min(1, Math.max(0, foreground.a));
  return {
    r: foreground.r * alpha + background.r * (1 - alpha),
    g: foreground.g * alpha + background.g * (1 - alpha),
    b: foreground.b * alpha + background.b * (1 - alpha),
    a: 1,
  };
};

const relativeLuminance = (color: ProbeColor): number => {
  const channel = (value: number) => {
    const normalized = value / 255;
    return normalized <= 0.03928
      ? normalized / 12.92
      : ((normalized + 0.055) / 1.055) ** 2.4;
  };
  return channel(color.r) * 0.2126 + channel(color.g) * 0.7152 + channel(color.b) * 0.0722;
};

const contrastRatio = (foreground: ProbeColor, background: ProbeColor): number => {
  const foregroundOnBackground = foreground.a < 1 ? compositeColor(foreground, background) : foreground;
  const light = Math.max(relativeLuminance(foregroundOnBackground), relativeLuminance(background));
  const dark = Math.min(relativeLuminance(foregroundOnBackground), relativeLuminance(background));
  return (light + 0.05) / (dark + 0.05);
};

const findOpaqueBackground = (element: HTMLElement): ProbeColor | null => {
  let current: HTMLElement | null = element;
  while (current) {
    const color = parseCssColor(getComputedStyle(current).backgroundColor);
    if (color && color.a >= 0.98) return color;
    current = current.parentElement;
  }
  return null;
};

const hasVisibleFocusStyle = (element: HTMLElement): boolean => {
  if (element.hasAttribute('disabled')) return true;
  element.focus({ preventScroll: true });
  if (document.activeElement !== element) return false;
  const style = getComputedStyle(element);
  const outlineVisible = style.outlineStyle !== 'none' && Number.parseFloat(style.outlineWidth) > 0;
  const shadowVisible = style.boxShadow !== 'none';
  return element.matches(':focus-visible') && (outlineVisible || shadowVisible);
};

const makeSupportMessages = (scenario: string): SupportChatState => {
  const messages: SupportMessage[] = [];
  if (scenario === 'empty') {
    return { status: 'ready', unreadCount: 0, conversation: makeConversation(), messages, syncWarning: false };
  }

  const longText = Array.from({ length: 16 }, (_, index) => `客服历史消息 ${index + 1}：${longDetail}`).join('\n');
  messages.push({ kind: 'server', message: makeMessage(1, scenario === 'long' ? longText : '你好，这里是客服回归探针。') });
  messages.push({ kind: 'server', message: makeMessage(2, '请补充复现步骤和当前页面状态。') });
  messages.push({ kind: 'server', message: makeMessage(3, '这是一条用于验证边界的客服回复。') });
  if (scenario === 'sending' || scenario === 'failed') {
    messages.push({
      kind: 'pending',
      clientMsgId: `support-surface-probe-${scenario}`,
      content: scenario === 'failed' ? '待重试的用户消息' : '正在发送的用户消息',
      createdAt: new Date(Date.parse(PROBE_NOW) + 4 * 60_000).toISOString(),
      delivery: scenario === 'failed' ? 'failed' : 'sending',
    });
  }
  return { status: 'ready', unreadCount: 0, conversation: makeConversation(), messages, syncWarning: false };
};

const makeErrorContext = (scenario: string): Parameters<typeof ConversationErrorReportModal>[0]['context'] => {
  const error: AgentStreamErrorInfo = {
    message: 'Provider returned an unavailable response',
    code: 'PROVIDER_UNAVAILABLE',
    ownership: 'unknown_upstream',
    retryable: true,
    feedback_recommended: true,
    detail: scenario === 'long' || scenario === 'expanded' ? longDetail : 'request_id=probe-1',
  };
  return {
    error,
    conversationId: 'probe-conversation',
    messageId: 'probe-message',
    turnId: 'probe-turn',
    occurredAt: PROBE_NOW,
  };
};

const ProbeTheme: React.FC<React.PropsWithChildren<{ scheme: 'light' | 'dark' }>> = ({ scheme, children }) => {
  const value: ThemeContextValue = useMemo(
    () => ({
      theme: scheme,
      themePreference: scheme,
      setThemePreference: async () => undefined,
      colorScheme: 'default',
      setColorScheme: async () => undefined,
      fontScale: 1,
      setFontScale: async () => undefined,
    }),
    [scheme]
  );
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
};

const SupportSurfaceProbe: React.FC = () => {
  const params = useMemo(readProbeParams, []);
  const surface = params.get('surface') === 'support' ? 'support' : 'feedback';
  const scenario = params.get('scenario') ?? (surface === 'support' ? 'normal' : 'long');
  const locale = params.get('locale') === 'en-US' ? 'en-US' : 'zh-CN';
  const scheme = params.get('scheme') === 'dark' ? 'dark' : 'light';
  const themeId = params.get('theme') ?? 'codex-neutral';
  const { i18n, t } = useTranslation();
  const [report, setReport] = useState<ProbeReport | null>(null);

  const reportContext = useMemo(() => makeErrorContext(scenario), [scenario]);
  const supportState = useMemo(() => makeSupportMessages(scenario), [scenario]);
  useEffect(() => {
    void i18n.changeLanguage(locale);
  }, [i18n, locale]);

  useEffect(() => {
    const preset = PRESET_THEMES.find((item) => item.id === themeId);
    document.documentElement.setAttribute('data-theme', scheme);
    document.body.setAttribute('arco-theme', scheme);
    document.documentElement.setAttribute('data-color-scheme', 'default');
    const styleId = 'support-surface-probe-custom-css';
    const style = document.getElementById(styleId) ?? document.createElement('style');
    style.id = styleId;
    style.textContent = processCustomCss(preset?.css ?? '');
    if (!style.parentElement) document.head.appendChild(style);
    ensureThemeControlContract();
    return () => document.getElementById(styleId)?.remove();
  }, [scheme, themeId]);

  useEffect(() => {
    const rootSelector = surface === 'support' ? '.support-chat-modal' : '.conversation-error-report-modal';
    const interactionTimer = window.setTimeout(() => {
      const root = document.querySelector<HTMLElement>(rootSelector);
      if (!root) return;

      if (scenario === 'expanded') {
        root.querySelector<HTMLElement>('.conversation-error-report__diagnostic .arco-collapse-item-header')?.click();
      }

      if (scenario === 'screenshots') {
        const input = root.querySelector<HTMLInputElement>('input[type=file]');
        if (input && typeof DataTransfer !== 'undefined') {
          const transfer = new DataTransfer();
          transfer.items.add(new File(['probe'], 'probe.png', { type: 'image/png' }));
          Object.defineProperty(input, 'files', { configurable: true, value: transfer.files });
          input.dispatchEvent(new Event('change', { bubbles: true }));
        }
      }

      if (surface === 'support' && scenario === 'log-confirm') {
        root.querySelector<HTMLButtonElement>('[data-button-shape="circle"]')?.click();
        window.setTimeout(() => {
          const label = locale === 'en-US' ? 'Upload logs' : '上传日志';
          Array.from(document.querySelectorAll<HTMLButtonElement>('button'))
            .find((button) => button.textContent?.includes(label))
            ?.click();
        }, 80);
      }
    }, 160);
    return () => window.clearTimeout(interactionTimer);
  }, [locale, scenario, surface]);

  useEffect(() => {
    const measureTimer = window.setTimeout(() => {
      const root = document.querySelector<HTMLElement>(
        surface === 'support' ? '.support-chat-modal' : '.conversation-error-report-modal'
      );
      const modalRect = rectOf(root);
      const header = root?.querySelector<HTMLElement>('.nomifun-modal-structured-header');
      const footer =
        root?.querySelector<HTMLElement>('.nomifun-modal-structured-footer') ??
        root?.querySelector<HTMLElement>('.support-chat-composer');
      const scrollSelector = surface === 'support' ? '.support-message-list' : '.conversation-error-report__scroll';
      const scrollOwners = root ? Array.from(root.querySelectorAll<HTMLElement>(scrollSelector)) : [];
      const controls = Array.from(
        root?.querySelectorAll<HTMLButtonElement>(
          surface === 'support'
            ? '.support-chat-composer button[data-button-shape="circle"]'
            : '.nomifun-modal-structured-header button, .conversation-error-report__footer-actions button'
        ) ?? []
      ).map((button, index) => {
        const buttonRect = button.getBoundingClientRect();
        const icon = button.querySelector<SVGElement>('svg');
        const iconRect = rectOf(icon);
        const iconCenterDeltaY = iconRect
          ? iconRect.top + iconRect.height / 2 - (buttonRect.top + buttonRect.height / 2)
          : null;
        const transform = getComputedStyle(button).transform;
        const focusVisible = hasVisibleFocusStyle(button);
        return {
          label: button.getAttribute('aria-label') ?? button.textContent?.trim() ?? `control-${index}`,
          width: buttonRect.width,
          height: buttonRect.height,
          disabled: button.disabled,
          iconCenterDeltaY,
          transform,
          focusVisible,
          pass:
            buttonRect.width > 0 &&
            buttonRect.height > 0 &&
            (iconCenterDeltaY === null || Math.abs(iconCenterDeltaY) <= 1) &&
            transform === 'none' &&
            focusVisible,
        };
      });
      const focusableControls = Array.from(
        root?.querySelectorAll<HTMLElement>('button, textarea, [role="button"], [tabindex]:not([tabindex="-1"])') ?? []
      ).filter((element) => visibleRect(rectOf(element)) && !element.hasAttribute('disabled'));
      const focusVisibleControlCount = focusableControls.reduce(
        (count, element) => count + (hasVisibleFocusStyle(element) ? 1 : 0),
        0
      );
      const contrastTargets = [
        {
          label: 'surface-text',
          element: root?.querySelector<HTMLElement>(
            surface === 'support' ? '.support-message-list' : '.conversation-error-report__intro'
          ),
        },
        {
          label: 'input-text',
          element: root?.querySelector<HTMLElement>('textarea'),
        },
        {
          label: 'primary-action',
          element: root?.querySelector<HTMLElement>(
            surface === 'support'
              ? '.support-chat-composer__send'
              : '.conversation-error-report__button--primary'
          ),
        },
      ];
      const contrastChecks = contrastTargets.map(({ label, element }) => {
        if (!element) return { label, ratio: null, pass: false };
        const foreground = parseCssColor(getComputedStyle(element).color);
        const background = findOpaqueBackground(element);
        if (!foreground || !background) return { label, ratio: null, pass: false };
        const ratio = contrastRatio(foreground, background);
        return { label, ratio, pass: ratio >= 3 };
      });
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      const thumbnails = Array.from(
        root?.querySelectorAll<HTMLElement>('.support-image-preview-item') ?? []
      );
      const thumbnailRect = rectOf(thumbnails[0] ?? null);
      const failures: string[] = [];
      const visibleRoot = visibleRect(modalRect);
      const horizontalOverflow = Boolean(root && root.scrollWidth > root.clientWidth + 1);
      const headerVisible = visibleRect(rectOf(header ?? null));
      const footerVisible = visibleRect(rectOf(footer ?? null));
      const withinViewport = Boolean(
        modalRect && modalRect.top >= -1 && modalRect.bottom <= window.innerHeight + 1
      );
      let expectedStateVisible = false;
      if (surface === 'support') {
        if (scenario === 'log-confirm') {
          expectedStateVisible = Boolean(root?.querySelector('.support-chat-log-confirm'));
        } else if (scenario === 'sending') {
          expectedStateVisible = Boolean(root?.querySelector('[data-delivery="sending"]'));
        } else if (scenario === 'failed') {
          expectedStateVisible = Boolean(root?.querySelector('[data-delivery="failed"]'));
        } else if (scenario === 'uploading') {
          expectedStateVisible = Boolean(root?.querySelector('textarea:disabled'));
        } else {
          expectedStateVisible =
            (root?.querySelectorAll('.support-message-list > *').length ?? 0) !== 0 ||
            scenario === 'empty';
        }
      } else {
        expectedStateVisible = scenario !== 'screenshots' || thumbnails.length > 0;
      }

      if (!visibleRoot) failures.push('modal-not-visible');
      if (!withinViewport) failures.push('modal-outside-viewport');
      if (horizontalOverflow) failures.push('horizontal-overflow');
      if (!headerVisible) failures.push('header-not-visible');
      if (!footerVisible) failures.push('footer-not-visible');
      if (scrollOwners.length !== 1) failures.push(`scroll-owner-count=${scrollOwners.length}`);
      if (controls.some((control) => !control.pass)) failures.push('icon-control-geometry');
      if (focusVisibleControlCount !== focusableControls.length) failures.push('focus-visible-control');
      if (contrastChecks.some((check) => !check.pass)) failures.push('contrast-check');
      if (surface === 'support' && scenario === 'log-confirm' && !expectedStateVisible) {
        failures.push('log-confirm-not-visible');
      }
      if (
        surface === 'support' &&
        (scenario === 'sending' || scenario === 'failed' || scenario === 'uploading') &&
        !expectedStateVisible
      ) {
        failures.push('message-state-not-visible');
      }

      setReport({
        ok: failures.length === 0,
        surface,
        scenario,
        viewport: { width: window.innerWidth, height: window.innerHeight, devicePixelRatio: window.devicePixelRatio },
        theme: themeId,
        scheme,
        locale,
        modal: {
          width: modalRect?.width ?? 0,
          height: modalRect?.height ?? 0,
          top: modalRect?.top ?? 0,
          bottom: modalRect?.bottom ?? 0,
          horizontalOverflow,
          headerVisible,
          footerVisible,
          scrollOwnerCount: scrollOwners.length,
        },
        controls,
        focusableControlCount: focusableControls.length,
        focusVisibleControlCount,
        contrastChecks,
        screenshotPreviewCount: thumbnails.length,
        screenshotPreviewSize: thumbnailRect
          ? { width: thumbnailRect.width, height: thumbnailRect.height }
          : null,
        expectedStateVisible,
        failures,
      });
    }, scenario === 'screenshots' || scenario === 'log-confirm' || scenario === 'expanded' ? 520 : 280);
    return () => window.clearTimeout(measureTimer);
  }, [locale, scenario, scheme, surface, themeId]);

  const noOpViewProps: Omit<SupportChatModalViewProps, 'state'> = {
    closeSupportChat: () => undefined,
    openSupportChat: () => undefined,
    sendMessage: async () => undefined,
    sendImages: () => undefined,
    retryMessage: async () => undefined,
    loadOlder: async () => false,
    composerDisabled: scenario === 'uploading',
  };

  return (
    <ProbeTheme scheme={scheme}>
      <CloudAuthContext.Provider value={PROBE_AUTH}>
        <main className='support-surface-probe' data-testid='support-surface-probe'>
          <header className='support-surface-probe__header'>
            <p className='support-surface-probe__eyebrow'>DEV ONLY · SUPPORT SURFACE</p>
            <h1>{surface === 'support' ? '联系客服窗口布局探针' : '反馈问题窗口布局探针'}</h1>
            <p>验证窄屏、长内容、单滚动容器、截图缩略图和按钮几何稳定性。</p>
          </header>
          {surface === 'support' ? (
            <SupportChatModalView state={supportState} {...noOpViewProps} />
          ) : (
            <ConversationErrorReportModal
              context={reportContext}
              onCancel={() => undefined}
              onSubmit={async () => ({ status: 'success' })}
              onOpenSupportChat={() => undefined}
            />
          )}
          <pre id='support-surface-probe-result' data-ready={report ? 'true' : 'false'}>
            {report ? JSON.stringify(report) : JSON.stringify({ ready: false })}
          </pre>
        </main>
      </CloudAuthContext.Provider>
    </ProbeTheme>
  );
};

export default SupportSurfaceProbe;
