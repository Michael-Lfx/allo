import { useEffect } from 'react';
import { useLatestRef } from '@/renderer/hooks/ui/useLatestRef';
import { matchAppChromeShortcut } from '@/renderer/utils/appChromeShortcuts';

type UseAppChromeShortcutsParams = {
  enabled: boolean;
  desktop: boolean;
  onNewConversation: () => void;
  onToggleSidebar: () => void;
  onToggleSettings: () => void;
  onFocusComposer: () => void;
  onToggleCheatsheet: () => void;
};

export const useAppChromeShortcuts = ({
  enabled,
  desktop,
  onNewConversation,
  onToggleSidebar,
  onToggleSettings,
  onFocusComposer,
  onToggleCheatsheet,
}: UseAppChromeShortcutsParams): void => {
  const handlersRef = useLatestRef({
    desktop,
    onNewConversation,
    onToggleSidebar,
    onToggleSettings,
    onFocusComposer,
    onToggleCheatsheet,
  });

  useEffect(() => {
    if (!enabled) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) {
        return;
      }

      const handlers = handlersRef.current;
      const matched = matchAppChromeShortcut(event, { desktop: handlers.desktop, mobile: false });
      switch (matched) {
        case 'newConversation':
          event.preventDefault();
          handlers.onNewConversation();
          return;
        case 'sidebar':
          event.preventDefault();
          handlers.onToggleSidebar();
          return;
        case 'settings':
          event.preventDefault();
          handlers.onToggleSettings();
          return;
        case 'focusComposer':
          event.preventDefault();
          handlers.onFocusComposer();
          return;
        case 'cheatsheet':
          event.preventDefault();
          handlers.onToggleCheatsheet();
          return;
        default:
          return;
      }
    };

    window.addEventListener('keydown', handleKeyDown, true);
    return () => {
      window.removeEventListener('keydown', handleKeyDown, true);
    };
  }, [enabled, handlersRef]);
};
