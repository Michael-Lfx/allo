import React, { useEffect, useMemo } from 'react';
import { CloseSmall, Keyboard } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import {
  formatAppChromeShortcut,
  listAppChromeShortcutsForSheet,
  type AppChromeShortcutGroup,
} from '@/renderer/utils/appChromeShortcuts';
import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';
import { isDesktopShell } from '@/renderer/utils/platform';

type AppChromeShortcutsHelpProps = {
  onClose: () => void;
};

const GROUP_ORDER: AppChromeShortcutGroup[] = ['navigation', 'conversation', 'help'];

const GROUP_TITLE: Record<AppChromeShortcutGroup, { key: I18nKey; fallback: string }> = {
  navigation: { key: 'common.shortcuts.groupNavigation', fallback: '导航' },
  conversation: { key: 'common.shortcuts.groupConversation', fallback: '对话' },
  help: { key: 'common.shortcuts.groupHelp', fallback: '帮助' },
};

const AppChromeShortcutsHelp: React.FC<AppChromeShortcutsHelpProps> = ({ onClose }) => {
  const { t } = useTranslation();
  const groups = useMemo(() => {
    const items = listAppChromeShortcutsForSheet({ desktop: isDesktopShell(), mobile: false });
    return GROUP_ORDER.map((group) => ({
      group,
      items: items.filter((item) => item.group === group),
    })).filter((section) => section.items.length > 0);
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <div
      className='fixed inset-0 z-[5000] flex items-center justify-center bg-[rgba(0,0,0,0.42)] px-16px'
      data-testid='app-chrome-shortcuts-help'
      role='dialog'
      aria-modal='true'
      aria-labelledby='app-chrome-shortcuts-title'
      onClick={onClose}
    >
      <div
        className='flex max-h-[80%] w-full max-w-460px flex-col overflow-hidden rounded-16px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] shadow-[0_24px_64px_rgba(0,0,0,0.32)]'
        onClick={(event) => event.stopPropagation()}
      >
        <div className='flex items-center gap-9px border-b border-solid border-[var(--color-border-2)] border-l-0 border-r-0 border-t-0 px-18px py-14px'>
          <span
            className='grid size-26px place-items-center rounded-8px leading-none text-[rgb(var(--primary-6))] [&_.i-icon]:block [&_.i-icon]:leading-none [&_svg]:block'
            style={{ background: 'rgba(var(--primary-6),0.12)' }}
          >
            <Keyboard
              theme='outline'
              size={16}
              strokeWidth={3}
              className='block leading-none'
              style={{ display: 'block', lineHeight: 0 }}
            />
          </span>
          <span id='app-chrome-shortcuts-title' className='text-15px font-700 text-[var(--color-text-1)]'>
            {t('common.shortcuts.title', { defaultValue: '快捷键' })}
          </span>
          <div
            role='button'
            tabIndex={0}
            title={t('common.shortcuts.close', { defaultValue: '关闭' })}
            onClick={onClose}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                onClose();
              }
            }}
            className='ml-auto grid h-28px w-28px place-items-center rounded-7px cursor-pointer text-[var(--color-text-3)] hover:bg-[var(--color-fill-2)] hover:text-[var(--color-text-1)]'
          >
            <CloseSmall theme='outline' size={20} strokeWidth={3} />
          </div>
        </div>

        <div className='min-h-0 flex-1 overflow-y-auto px-18px py-10px'>
          {groups.map((section) => (
            <section key={section.group} className='mb-10px last:mb-0'>
              <h2 className='m-0 py-6px text-11px font-600 tracking-wide text-[var(--color-text-3)]'>
                {t(GROUP_TITLE[section.group].key, { defaultValue: GROUP_TITLE[section.group].fallback })}
              </h2>
              {section.items.map((item) => (
                <div key={item.id} className='flex items-center justify-between gap-16px py-7px'>
                  <span className='text-13px text-[var(--color-text-2)]'>
                    {t(item.titleKey, { defaultValue: item.id })}
                  </span>
                  <kbd className='rounded-6px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-8px py-3px text-11px font-600 text-[var(--color-text-1)] whitespace-nowrap'>
                    {formatAppChromeShortcut(item.id)}
                  </kbd>
                </div>
              ))}
            </section>
          ))}
        </div>
      </div>
    </div>
  );
};

export default AppChromeShortcutsHelp;
