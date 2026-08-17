import { Button } from '@arco-design/web-react';
import { ArrowUp, Brain, Down, Lightning, LinkOne, More, Search } from '@icon-park/react';
import React, { useEffect, useMemo, useState } from 'react';
import { LayoutContext } from '@renderer/hooks/context/LayoutContext';
import { PreviewProvider } from '@renderer/pages/conversation/Preview';
import NomiModelSelector from '@renderer/pages/conversation/platforms/nomi/NomiModelSelector';
import AgentModeSelector from '@renderer/components/agent/AgentModeSelector';
import ReasoningEffortSelector from '@renderer/components/agent/ReasoningEffortSelector';
import { GuidModelSelectorButton } from '@renderer/pages/guid/components/GuidModelSelector';
import guidStyles from '@renderer/pages/guid/index.module.css';
import KnowledgeDetailActionBar from '@renderer/pages/knowledge/KnowledgeDetailPage/KnowledgeDetailActionBar';
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
  const targets = [
    { selector: '[data-testid="nomi-model-selector"]', id: 'production-nomi-model-selector', labelRequired: false },
    { selector: '[data-testid^="agent-mode-selector"]', id: 'production-agent-mode-selector', labelRequired: false },
    { selector: '[data-testid="guid-model-selector"]', id: 'production-guid-model-selector', labelRequired: false },
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
    const clipped =
      buttonRect.left < -0.5 ||
      buttonRect.top < -0.5 ||
      buttonRect.right > window.innerWidth + 0.5 ||
      buttonRect.bottom > window.innerHeight + 0.5 ||
      (!button.dataset.buttonAllowTruncate && button.scrollWidth > button.clientWidth + 1);
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
      iconOnly,
      circle,
      displaySources,
      pass: failures.length === 0,
      failures,
    } satisfies ProbeButtonReport;
  });

  const presentIds = new Set(buttons.map((button) => button.id));
  const missingFixtures = requiredProductionProbeIds.filter((id) => !presentIds.has(id));

  return {
    ok: buttons.length > 0 && missingFixtures.length === 0 && buttons.every((button) => button.pass),
    missingFixtures,
    viewport: { width: window.innerWidth, height: window.innerHeight, devicePixelRatio: window.devicePixelRatio },
    theme,
    locale,
    userAgent: navigator.userAgent,
    styleOrder: Array.from(document.head.children)
      .filter((element) => element.tagName === 'STYLE' || element.tagName === 'LINK')
      .map((element) => element.id || element.getAttribute('href') || element.tagName.toLowerCase()),
    buttons,
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
