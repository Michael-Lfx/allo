import { Modal } from '@arco-design/web-react';
import type {
  AgentErrorOwnership,
  AgentErrorResolution,
  IMessageTips,
  IMessageText,
  TMessage,
} from '@/common/chat/chatLib';
import { parseConversationId, parseMessageId } from '@/common/types/ids';
import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { MemoryRouter } from 'react-router-dom';

import ErrorDiagnosticContent from '@renderer/components/base/ErrorDiagnosticContent';
import MessageTips from '@renderer/pages/conversation/Messages/components/MessageTips';
import { resolveMessageErrorRecoveryAction } from '@renderer/pages/conversation/Messages/components/messageErrorRecovery';
import { MessageListProvider } from '@renderer/pages/conversation/Messages/hooks';
import { PRESET_THEMES } from '@renderer/pages/settings/DisplaySettings/presets';
import { ConversationProvider } from '@renderer/hooks/context/ConversationContext';
import { buildErrorDiagnostic } from '@renderer/utils/ui/errorDiagnostics';
import { processCustomCss } from '@renderer/utils/theme/customCssProcessor';
import { ensureThemeControlContract } from '@renderer/utils/theme/themeControlContract';
import './errorSurfaceProbe.css';
import '../conversation/Messages/messages.css';

type ErrorSurfaceFixture = {
  id: string;
  title: string;
  body: string;
  modelId?: string;
  code?: string;
  incidentId?: string;
  ownership?: AgentErrorOwnership;
  retryable?: boolean;
  resolution?: AgentErrorResolution;
  detail?: string;
};

type ErrorSurfaceProbeReport = {
  ok: boolean;
  viewport: { width: number; height: number; devicePixelRatio: number };
  theme: string;
  locale: string;
  expanded: boolean;
  fixture: string;
  summaryVisible: boolean;
  detailsDefaultCollapsed: boolean;
  detailsVisible: boolean;
  horizontalOverflow: boolean;
  cardHeight: number;
  modalVisible: boolean;
  modalDetailsDefaultCollapsed: boolean;
  modalDetailsVisible: boolean;
  modalHorizontalOverflow: boolean;
  modalHeight: number;
  hasLegacyRail: boolean;
  incidentIdVisible: boolean;
  actionLabels: string[];
  expectedActionLabels: string[];
  disabledActionCount: number;
  failures: string[];
};

const readProbeParams = (): URLSearchParams => {
  const hashQuery = window.location.hash.split('?')[1] ?? '';
  const params = new URLSearchParams(window.location.search);
  new URLSearchParams(hashQuery).forEach((value, key) => params.set(key, value));
  return params;
};

const fixtures: ErrorSurfaceFixture[] = [
  {
    id: 'provider-schema',
    title: '模型服务商拒绝了工具定义',
    body: '服务商拒绝了本次请求中的工具定义。请更新 Agent，或禁用不兼容的工具后重试。',
    modelId: 'claude-sonnet-4-20250514',
    code: 'USER_LLM_PROVIDER_INVALID_TOOL_SCHEMA',
    incidentId: 'err_probe_01',
    ownership: 'user_llm_provider',
    retryable: true,
    detail:
      "Nomi agent error: Provider error: API error 500:\n{\"code\":500,\"msg\":\"Invalid schema for function 'Read'; schema must have type 'object'\"}",
  },
  {
    id: 'legacy-message',
    title: '请求未完成',
    body: '请求没有完成，请稍后重试。',
    retryable: false,
    detail: 'Legacy upstream message without a machine-readable error code.',
  },
  {
    id: 'long-detail',
    title: '上游返回了无法解析的响应',
    body: '服务返回了异常响应。可以复制诊断信息交给开发排查。',
    code: 'UPSTREAM_RESPONSE_WITH_A_VERY_LONG_CODE_THAT_MUST_WRAP_WITHOUT_CLIPPING',
    incidentId: 'incident-with-a-long-identifier-that-must-wrap-at-narrow-width',
    ownership: 'unknown_upstream',
    retryable: true,
    detail: `${'Detailed provider payload line with safe diagnostic content. '.repeat(12)}\nworkspace_path=C:\\Users\\demo\\workspace\\private-project`,
  },
  {
    id: 'no-detail',
    title: '应用处理失败',
    body: '本次请求在应用处理过程中失败。',
    code: 'NOMIFUN_INTERNAL_ERROR',
    retryable: false,
  },
  {
    id: 'config-recovery',
    title: '模型配置不可用',
    body: '当前模型配置不可用，请更换模型后重试。',
    code: 'PROVIDER_UNAVAILABLE',
    ownership: 'user_llm_provider',
    retryable: false,
    resolution: { kind: 'change_model', target: 'provider_settings' },
    detail: 'The selected provider is unavailable for this request.',
  },
  {
    id: 'provider-billing',
    title: '模型服务商需要开通计费',
    body: '所选服务商需要有效的计费、余额或额度后才能完成请求。请处理后重试。',
    code: 'USER_LLM_PROVIDER_BILLING_REQUIRED',
    ownership: 'user_llm_provider',
    retryable: false,
    resolution: { kind: 'check_provider_billing', target: 'provider_settings' },
    detail: 'Nomi agent error: Provider error: API error 402: Insufficient credits.',
  },
];

const PROBE_CONVERSATION_ID = parseConversationId('019b0000-0000-7000-8000-000000000901');
const PROBE_USER_MESSAGE_ID = parseMessageId('019b0000-0000-7000-8000-000000000902');
const PROBE_ERROR_MESSAGE_ID = parseMessageId('019b0000-0000-7000-8000-000000000903');

const buildProbeMessages = (fixture: ErrorSurfaceFixture): { messages: TMessage[]; error: IMessageTips } => {
  const structuredError =
    fixture.id === 'legacy-message'
      ? undefined
      : {
          message: fixture.body,
          ...(fixture.modelId ? { model_id: fixture.modelId } : {}),
          ...(fixture.code ? { code: fixture.code } : {}),
          ...(fixture.incidentId ? { incident_id: fixture.incidentId } : {}),
          ...(fixture.ownership ? { ownership: fixture.ownership } : {}),
          ...(fixture.retryable !== undefined ? { retryable: fixture.retryable } : {}),
          ...(fixture.resolution ? { resolution: fixture.resolution } : {}),
          ...(fixture.detail ? { detail: fixture.detail } : {}),
        };
  const userMessage: IMessageText = {
    id: 'error-surface-user',
    msg_id: PROBE_USER_MESSAGE_ID,
    conversation_id: PROBE_CONVERSATION_ID,
    type: 'text',
    content: { content: '你好' },
    position: 'right',
    created_at: 100,
  };
  const errorMessage: IMessageTips = {
    id: 'error-surface-error',
    msg_id: PROBE_ERROR_MESSAGE_ID,
    conversation_id: PROBE_CONVERSATION_ID,
    type: 'tips',
    content: {
      content: fixture.body,
      type: 'error',
      ...(structuredError ? { error: structuredError } : {}),
    },
    position: 'left',
    created_at: 200,
  };
  return { messages: [userMessage, errorMessage], error: errorMessage };
};

const ErrorSurfaceProbe: React.FC = () => {
  const params = useMemo(readProbeParams, []);
  const themeId = params.get('theme') ?? 'codex-neutral';
  const locale = params.get('locale') === 'en-US' ? 'en-US' : 'zh-CN';
  const fixtureId = params.get('fixture') ?? 'provider-schema';
  const expanded = params.get('expanded') === '1';
  const selectedFixture = fixtures.find((item) => item.id === fixtureId) ?? fixtures[0];
  const probeMessages = useMemo(() => buildProbeMessages(selectedFixture), [selectedFixture]);
  const [report, setReport] = useState<ErrorSurfaceProbeReport | null>(null);
  const [isExpanded, setIsExpanded] = useState(expanded);
  const { i18n, t } = useTranslation();

  useEffect(() => {
    void i18n.changeLanguage(locale);
    const scheme = params.get('scheme') === 'dark' ? 'dark' : 'light';
    const preset = PRESET_THEMES.find((item) => item.id === themeId);
    document.documentElement.setAttribute('data-theme', scheme);
    document.body.setAttribute('arco-theme', scheme);
    const customStyleId = 'error-surface-probe-user-css';
    document.getElementById(customStyleId)?.remove();
    const customStyle = document.createElement('style');
    customStyle.id = customStyleId;
    customStyle.textContent = processCustomCss(preset?.css ?? '');
    if (params.get('scenario') !== 'off') document.head.appendChild(customStyle);
    ensureThemeControlContract();

    const measure = () => {
      const card = document.querySelector<HTMLElement>('[data-testid="error-surface-card"]');
      const modal = document.querySelector<HTMLElement>('.error-surface-probe__modal');
      const summary = card?.querySelector<HTMLElement>('.message-error-note__diagnostic-summary');
      const detail = card?.querySelector<HTMLElement>('.message-error-note__detail-body');
      const collapseItem = card?.querySelector<HTMLElement>('.message-error-note__details .arco-collapse-item');
      const modalDiagnostic = modal?.querySelector<HTMLElement>('.conversation-error-diagnostic');
      const modalDetail = modal?.querySelector<HTMLElement>('.conversation-error-diagnostic__detail');
      const modalCollapseItem = modal?.querySelector<HTMLElement>('.conversation-error-diagnostic__details .arco-collapse-item');
      const cardRect = card?.getBoundingClientRect();
      const modalRect = modal?.getBoundingClientRect();
      const failures: string[] = [];
      const summaryVisible = Boolean(summary && summary.getBoundingClientRect().height > 0);
      const detailsVisible = Boolean(detail && detail.getBoundingClientRect().height > 0);
      const detailsOpen = Boolean(collapseItem?.classList.contains('arco-collapse-item-active'));
      const modalVisible = Boolean(modal && modalRect && modalRect.width > 0 && modalRect.height > 0);
      const modalDetailsVisible = Boolean(modalDetail && modalDetail.getBoundingClientRect().height > 0);
      const modalDetailsOpen = Boolean(modalCollapseItem?.classList.contains('arco-collapse-item-active'));
      const horizontalOverflow = Boolean(
        card && (card.scrollWidth > card.clientWidth + 1 || (detail && detail.scrollWidth > detail.clientWidth + 1))
      );
      const modalHorizontalOverflow = Boolean(
        modalDiagnostic &&
          (modalDiagnostic.scrollWidth > modalDiagnostic.clientWidth + 1 ||
            (modalDetail && modalDetail.scrollWidth > modalDetail.clientWidth + 1))
      );
      const hasLegacyRail = Boolean(card?.querySelector('.message-error-note__rail'));
      const incidentIdVisible = Boolean(card?.querySelector('.message-error-note__incident'));
      const actionElements = Array.from(
        card?.querySelectorAll<HTMLElement>('.message-error-note__actions [role="button"], .message-error-note__actions button') ?? []
      );
      const actionLabels = actionElements
        .map((element) => element.textContent?.trim() || element.getAttribute('aria-label') || '')
        .filter(Boolean);
      const recoveryAction = resolveMessageErrorRecoveryAction(probeMessages.error.content.error);
      const recoveryActionLabel = recoveryAction
        ? t(recoveryAction.labelKey, {
            defaultValue:
              recoveryAction.source === 'open_billing'
                ? 'Buy credits'
                : recoveryAction.source === 'change_model'
                  ? 'Change model'
                  : 'Fix setup',
          })
        : null;
      const expectedActionLabels = [
        ...(selectedFixture.id === 'legacy-message' || selectedFixture.retryable !== false
          ? [t('conversation.agentError.retryAction', { defaultValue: 'Retry' })]
          : []),
        ...(recoveryActionLabel ? [recoveryActionLabel] : []),
        ...(selectedFixture.id === 'legacy-message' || selectedFixture.retryable !== false
          ? [t('common.edit', { defaultValue: 'Edit' })]
          : []),
        t('conversation.agentError.copyDiagnostic'),
        t('settings.oneClickFeedback'),
      ];
      const disabledActionCount = actionElements.filter(
        (element) => element.hasAttribute('disabled') || element.getAttribute('aria-disabled') === 'true'
      ).length;
      if (!summaryVisible) failures.push('diagnostic-summary-not-visible');
      if (!isExpanded && detailsOpen) failures.push('details-not-collapsed');
      if (isExpanded && (!detailsOpen || !detailsVisible)) failures.push('details-not-visible');
      if (!modalVisible) failures.push('modal-not-visible');
      if (!isExpanded && modalDetailsOpen) failures.push('modal-details-not-collapsed');
      if (isExpanded && (!modalDetailsOpen || !modalDetailsVisible)) failures.push('modal-details-not-visible');
      if (horizontalOverflow) failures.push('horizontal-overflow');
      if (modalHorizontalOverflow) failures.push('modal-horizontal-overflow');
      if (hasLegacyRail) failures.push('legacy-side-rail-present');
      if (incidentIdVisible) failures.push('incident-id-visible-in-card-header');
      if (!cardRect || cardRect.height > 720) failures.push(`card-height=${cardRect?.height ?? 0}`);
      if (!modalRect || modalRect.top < -0.5 || modalRect.bottom > window.innerHeight + 0.5) {
        failures.push(
          `modal-viewport=${modalRect ? `${modalRect.top.toFixed(2)}:${modalRect.bottom.toFixed(2)}` : 'missing'}`
        );
      }
      if (JSON.stringify(actionLabels) !== JSON.stringify(expectedActionLabels)) {
        failures.push(`action-order=${JSON.stringify(actionLabels)}`);
      }
      if (disabledActionCount > 0) failures.push(`disabled-actions=${disabledActionCount}`);

      setReport({
        ok: failures.length === 0,
        viewport: { width: window.innerWidth, height: window.innerHeight, devicePixelRatio: window.devicePixelRatio },
        theme: themeId,
        locale,
        expanded: isExpanded,
        fixture: selectedFixture.id,
        summaryVisible,
        detailsDefaultCollapsed: !detailsOpen,
        detailsVisible,
        horizontalOverflow,
        cardHeight: cardRect?.height ?? 0,
        modalVisible,
        modalDetailsDefaultCollapsed: !modalDetailsOpen,
        modalDetailsVisible,
        modalHorizontalOverflow,
        modalHeight: modalRect?.height ?? 0,
        hasLegacyRail,
        incidentIdVisible,
        actionLabels,
        expectedActionLabels,
        disabledActionCount,
        failures,
      });
    };
    let measureTimer: number | null = null;
    const disclosureTimer = window.setTimeout(() => {
      const card = document.querySelector<HTMLElement>('[data-testid="error-surface-card"]');
      const collapseItem = card?.querySelector<HTMLElement>('.message-error-note__details .arco-collapse-item');
      const detailsOpen = Boolean(collapseItem?.classList.contains('arco-collapse-item-active'));
      if (detailsOpen !== isExpanded) {
        card?.querySelector<HTMLElement>('.message-error-note__details .arco-collapse-item-header')?.click();
      }
      const modal = document.querySelector<HTMLElement>('.error-surface-probe__modal');
      const modalCollapseItem = modal?.querySelector<HTMLElement>('.conversation-error-diagnostic__details .arco-collapse-item');
      const modalDetailsOpen = Boolean(modalCollapseItem?.classList.contains('arco-collapse-item-active'));
      if (modalDetailsOpen !== isExpanded) {
        modal?.querySelector<HTMLElement>('.conversation-error-diagnostic__details .arco-collapse-item-header')?.click();
      }
      measureTimer = window.setTimeout(measure, 120);
    }, 0);

    return () => {
      window.clearTimeout(disclosureTimer);
      if (measureTimer !== null) window.clearTimeout(measureTimer);
      document.getElementById(customStyleId)?.remove();
    };
  }, [i18n, isExpanded, locale, params, selectedFixture, t, themeId]);

  useEffect(() => {
    setIsExpanded(expanded);
  }, [expanded]);

  return (
    <main className='error-surface-probe' data-testid='error-surface-probe'>
      <header className='error-surface-probe__header'>
        <div>
          <p className='error-surface-probe__eyebrow'>DEV ONLY · ERROR SURFACE</p>
          <h1>错误诊断体验探针</h1>
          <p>摘要常显，技术详情默认折叠；用于 Edge 窄屏、主题和超长诊断回归。</p>
        </div>
        <div className='error-surface-probe__controls'>
          <label>
            Fixture
            <select
              value={selectedFixture.id}
              onChange={(event) => {
                const next = new URL(window.location.href);
                next.searchParams.set('fixture', event.target.value);
                window.location.assign(next.toString());
              }}
            >
              {fixtures.map((fixture) => (
                <option key={fixture.id} value={fixture.id}>
                  {fixture.id}
                </option>
              ))}
            </select>
          </label>
          <label className='error-surface-probe__checkbox'>
            <input type='checkbox' checked={isExpanded} onChange={(event) => setIsExpanded(event.target.checked)} />
            展开详情
          </label>
        </div>
      </header>

      <section className='error-surface-probe__grid'>
        <div>
          <h2>对话错误卡片</h2>
          <ConversationProvider
            value={{
              conversation_id: PROBE_CONVERSATION_ID,
              type: 'nomi',
              readOnly: false,
              isProcessing: false,
            }}
          >
            <MessageListProvider value={probeMessages.messages}>
              <MemoryRouter>
                <div data-testid='error-surface-card'>
                  <MessageTips message={probeMessages.error} />
                </div>
              </MemoryRouter>
            </MessageListProvider>
          </ConversationProvider>
        </div>
        <div>
          <Modal
            visible
            simple
            mask={false}
            closable={false}
            footer={null}
            title='Modal 诊断内容'
            className='error-surface-probe__modal'
          >
            <ErrorDiagnosticContent
              diagnostic={buildErrorDiagnostic({
                message: selectedFixture.body,
                modelId: selectedFixture.modelId,
                code: selectedFixture.code,
                incidentId: selectedFixture.incidentId,
                ownership: selectedFixture.ownership,
                retryable: selectedFixture.retryable,
                resolutionKind: selectedFixture.resolution?.kind,
                resolutionTarget: selectedFixture.resolution?.target,
                detail: selectedFixture.detail,
              })}
            />
          </Modal>
        </div>
      </section>

      <pre id='error-surface-probe-result' data-ready={report ? 'true' : 'false'}>
        {report ? JSON.stringify(report) : JSON.stringify({ ready: false })}
      </pre>
    </main>
  );
};

export default ErrorSurfaceProbe;
