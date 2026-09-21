import { useEffect } from 'react';
import { isHistoryBackShortcut, isHistoryForwardShortcut } from '@/renderer/utils/historyNavigationShortcut';

type UseHistoryNavigationShortcutsParams = {
  enabled: boolean;
  back: () => void;
  forward: () => void;
};

export const useHistoryNavigationShortcuts = ({
  enabled,
  back,
  forward,
}: UseHistoryNavigationShortcutsParams): void => {
  useEffect(() => {
    if (!enabled) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) {
        return;
      }

      if (isHistoryBackShortcut(event)) {
        event.preventDefault();
        back();
        return;
      }

      if (isHistoryForwardShortcut(event)) {
        event.preventDefault();
        forward();
      }
    };

    window.addEventListener('keydown', handleKeyDown, true);
    return () => {
      window.removeEventListener('keydown', handleKeyDown, true);
    };
  }, [back, enabled, forward]);
};
