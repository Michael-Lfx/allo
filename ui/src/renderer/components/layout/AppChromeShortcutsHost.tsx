import React, { useCallback, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import AppChromeShortcutsHelp from '@/renderer/components/layout/AppChromeShortcutsHelp';
import { useSettingsNavigationTransition } from '@/renderer/components/layout/SettingsNavigationTransition';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { useAppChromeShortcuts } from '@/renderer/hooks/ui/useAppChromeShortcuts';
import { emitter, useAddEventListener } from '@/renderer/utils/emitter';
import { isDesktopShell } from '@/renderer/utils/platform';
import { resolveSettingsTogglePath } from '@/renderer/utils/settingsToggle';

const AppChromeShortcutsHost: React.FC = () => {
  const layout = useLayoutContext();
  const navigate = useNavigate();
  const location = useLocation();
  const { navigateWithSettingsTransition } = useSettingsNavigationTransition();
  const [helpOpen, setHelpOpen] = useState(false);

  const closeHelp = useCallback(() => setHelpOpen(false), []);
  const toggleHelp = useCallback(() => setHelpOpen((open) => !open), []);

  useAddEventListener('app.shortcuts.open', () => setHelpOpen(true), []);

  useAppChromeShortcuts({
    enabled: !layout?.isMobile,
    desktop: isDesktopShell(),
    onNewConversation: () => {
      void navigate('/guid', { state: { resetPreset: true } });
    },
    onToggleSidebar: () => {
      if (!layout?.setSiderCollapsed) return;
      layout.setSiderCollapsed(!layout.siderCollapsed);
    },
    onToggleSettings: () => {
      const target = resolveSettingsTogglePath(location.pathname);
      const go = () => {
        void navigate(target.path);
      };
      if (target.enter) {
        navigateWithSettingsTransition(target.path, go);
        return;
      }
      go();
    },
    onFocusComposer: () => {
      emitter.emit('composer.focus');
    },
    onToggleCheatsheet: toggleHelp,
  });

  if (!helpOpen) {
    return null;
  }

  return <AppChromeShortcutsHelp onClose={closeHelp} />;
};

export default AppChromeShortcutsHost;
