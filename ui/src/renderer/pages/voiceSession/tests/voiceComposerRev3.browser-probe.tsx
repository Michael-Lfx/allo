/** Isolated component acceptance entry. Never imported by production routes. */
import React, {act} from 'react';
import {createRoot} from 'react-dom/client';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';
import '@arco-design/web-react/dist/css/arco.css';
import ConversationRow from '../../conversation/SessionList/ConversationRow';
import type {ConversationRowProps} from '../../conversation/SessionList/types';
import {LayoutContext} from '@renderer/hooks/context/LayoutContext';
import VoiceCallDock from '../VoiceCallDock';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';

(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
const output = document.createElement('pre'); output.id = 'rev3-result'; document.body.append(output);
const container = document.createElement('div'); container.style.width = '300px'; document.body.append(container);
const root = createRoot(container);
const results: string[] = [];
const check = (condition: unknown, label: string) => { if (!condition) throw new Error(label); results.push(label); };
window.fetch = (async () => Response.json({})) as typeof fetch;

async function run() {
  for (const locale of ['zh-CN', 'en-US'] as const) for (const theme of ['light', 'dark'] as const) {
    document.documentElement.dataset.theme = theme;
    const dictionary = locale === 'zh-CN' ? zh : en;
    const i18n = createInstance();
    await i18n.init({lng: locale, resources: {[locale]: {translation: {voice: dictionary}}}, initImmediate: false});
    let navigations = 0; let deletions = 0;
    const tag = `${locale}/${theme}`;
    function Row() {
      const [menuVisible, setMenu] = React.useState(false);
      const props = {conversation: {id: 'p', type: 'nomi', name: 'A deliberately long renamed voice conversation', extra: {voice: true}, created_at: Date.now()},
        isCallActive: true, isGenerating: false, hasCompletionUnread: false, collapsed: false, tooltipEnabled: false,
        batchMode: false, checked: false, selected: false, menuVisible, onToggleChecked: () => {}, onConversationClick: () => {navigations++;},
        onOpenMenu: () => setMenu(true), onMenuVisibleChange: (_id: string, visible: boolean) => setMenu(visible), onEditStart: () => {},
        onDelete: () => {deletions++;}, onTogglePin: () => {}, getJobStatus: () => 'none',
      } as unknown as ConversationRowProps;
      return <ConversationRow {...props}/>;
    }
    await act(async () => root.render(<I18nextProvider i18n={i18n}><LayoutContext.Provider value={{isMobile: false, siderCollapsed: false, setSiderCollapsed: () => {}}}>
      <Row/>
    </LayoutContext.Provider></I18nextProvider>));
    const row = container.querySelector<HTMLElement>('#c-p')!;
    const trigger = row.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]')!;
    const marker = row.querySelector<HTMLElement>('[data-testid="conversation-voice-marker"]')!;
    check(Boolean(trigger) && trigger.tabIndex >= 0 && row.tabIndex === 0, `${tag}: keyboard entry exists`);
    const title = row.querySelector('.chat-history__item-name') ?? row.querySelector('.text-ellipsis');
    const widthBefore = title?.getBoundingClientRect().width;
    await act(async () => trigger.focus());
    check(document.activeElement === trigger && getComputedStyle(trigger.parentElement!).opacity === '1', `${tag}: focus reveals More`);
    await act(async () => trigger.click());
    check(navigations === 0 && trigger === row.querySelector('button[aria-haspopup="menu"]'), `${tag}: More does not navigate or remount`);
    check(trigger.getAttribute('aria-expanded') === 'true', `${tag}: controlled menu opens`);
    check(!document.querySelector('.arco-popover-content'), `${tag}: hover card stays suppressed`);
    const disabledDelete = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find(item => item.textContent?.includes(dictionary.entry.endBeforeDelete));
    check(Boolean(disabledDelete), `${tag}: active call deletion has an explanation`);
    if (disabledDelete) await act(async () => disabledDelete.click());
    check(deletions === 0, `${tag}: active call is not deleted`);
    await act(async () => trigger.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true})));
    check(trigger.getAttribute('aria-expanded') === 'false' && document.activeElement === trigger, `${tag}: Escape restores focus`);
    check(widthBefore === title?.getBoundingClientRect().width, `${tag}: title width stays stable`);
    await act(async () => marker.click());
    check(navigations === 1, `${tag}: passive marker only navigates`);
    check(marker.getAttribute('aria-label') === dictionary.entry.callActiveMarker, `${tag}: active status has an accessible name`);
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
      check(getComputedStyle(marker.querySelector('svg')!).animationName === 'none', `${tag}: reduced motion is respected`);
    }
    await act(async () => root.render(<I18nextProvider i18n={i18n}><VoiceCallDock conversationId='p' snapshot={null}/></I18nextProvider>));
    check(container.textContent?.includes(dictionary.dock.syncing), `${tag}: unknown state is syncing`);
    check(!container.querySelector('details'), `${tag}: normal pill has no empty menu`);
    await act(async () => root.render(null));
  }
}
run().then(() => {output.dataset.ready = 'true'; output.textContent = JSON.stringify({ok: true, count: results.length, results});})
  .catch(error => {output.dataset.ready = 'true'; output.textContent = JSON.stringify({ok: false, results, error: String(error), stack: error.stack});});
