/** Isolated acceptance fixture. Never imported by production routes. */
import React, { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { flushSync } from 'react-dom';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import '@arco-design/web-react/es/_util/react-19-adapter';
import { LayoutContext } from '@/renderer/hooks/context/LayoutContext';
import zh from '@/renderer/services/i18n/locales/zh-CN';
import en from '@/renderer/services/i18n/locales/en-US';
import ConversationRow from './ConversationRow';
import type { ConversationRowProps } from './types';

interface Options {
  locale?: 'zh-CN' | 'en-US';
  theme?: 'light' | 'dark';
  width?: number;
  active?: boolean;
  mobile?: boolean;
  collapsed?: boolean;
  batch?: boolean;
  ordinary?: boolean;
  pinned?: boolean;
}

const host = document.createElement('div');
host.style.cssText = 'padding:20px;max-width:100%;box-sizing:border-box';
document.body.append(host);
const root = createRoot(host);
const events = { navigations: 0, menus: 0, deletions: 0 };
const title = '很长的重命名语音会话 A long renamed conversation for layout verification';

function Scene({ options }: { options: Options }) {
  const [menuVisible, setMenuVisible] = useState(false);
  const props = {
    conversation: {
      id: '0190f5fe-7c00-7a00-8000-000000000909', type: 'nomi', name: title,
      extra: options.ordinary ? {} : { voice: true }, pinned: Boolean(options.pinned),
      created_at: Date.now() - 3_600_000,
    },
    isGenerating: false, hasCompletionUnread: true, collapsed: Boolean(options.collapsed),
    tooltipEnabled: false, batchMode: Boolean(options.batch), checked: false, selected: false,
    isCallActive: Boolean(options.active), menuVisible,
    onToggleChecked: () => {}, onConversationClick: () => { events.navigations += 1; },
    onOpenMenu: () => { events.menus += 1; setMenuVisible(true); },
    onMenuVisibleChange: (_id: string, visible: boolean) => setMenuVisible(visible),
    onEditStart: () => {}, onDelete: () => { events.deletions += 1; },
    onTogglePin: () => {}, onExport: () => {}, getJobStatus: () => 'none',
  } as unknown as ConversationRowProps;
  return (
    <LayoutContext.Provider value={{ isMobile: Boolean(options.mobile), siderCollapsed: Boolean(options.collapsed), setSiderCollapsed: () => {} }}>
      <div className={options.collapsed ? 'collapsed' : undefined} style={{ width: options.width ?? 300, background: 'var(--flowy-panel)' }}>
        <ConversationRow {...props} />
      </div>
    </LayoutContext.Provider>
  );
}

// Only the fixture stubs API reads; the component and its styles are production code.
window.fetch = (async () => Response.json({})) as typeof fetch;
const api = {
  events,
  async render(options: Options = {}) {
    flushSync(() => root.render(null));
    events.navigations = 0; events.menus = 0; events.deletions = 0;
    document.documentElement.dataset.theme = options.theme ?? 'light';
    document.documentElement.dataset.colorScheme = 'default';
    document.body.setAttribute('arco-theme', options.theme ?? 'light');
    const locale = options.locale ?? 'zh-CN';
    const i18n = createInstance();
    await i18n.init({ lng: locale, resources: { [locale]: { translation: locale === 'zh-CN' ? zh : en } }, initImmediate: false });
    flushSync(() => root.render(<I18nextProvider i18n={i18n}><Scene options={options} /></I18nextProvider>));
  },
};
Object.assign(window, { voiceRowProbe: api });
