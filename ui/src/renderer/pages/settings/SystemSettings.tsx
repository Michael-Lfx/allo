
import React, { Suspense } from 'react';
import { useLocation } from 'react-router-dom';
import SettingsContentLoading from '@renderer/components/layout/SettingsContentLoading';
import SettingsPageWrapper from './components/SettingsPageWrapper';

const SystemModalContent = React.lazy(() => import('@/renderer/components/settings/SettingsModal/contents/SystemModalContent'));
const AboutModalContent = React.lazy(() => import('@/renderer/components/settings/SettingsModal/contents/AboutModalContent'));
const BrowserUseSettingsContent = React.lazy(() => import('@/renderer/components/settings/SettingsModal/contents/BrowserUseSettingsContent'));
const ComputerUseSettingsContent = React.lazy(() => import('@/renderer/components/settings/SettingsModal/contents/ComputerUseSettingsContent'));

type SystemSettingsRoute = 'system' | 'about' | 'browser-use' | 'computer-use';

const resolveSystemSettingsRoute = (pathname: string): SystemSettingsRoute => {
  if (pathname === '/settings/about') return 'about';
  if (pathname === '/settings/browser-use') return 'browser-use';
  if (pathname === '/settings/computer-use') return 'computer-use';
  return 'system';
};

const SystemSettingsPanel: React.FC<{ route: SystemSettingsRoute }> = ({ route }) => {
  switch (route) {
    case 'about':
      return <AboutModalContent />;
    case 'browser-use':
      return <BrowserUseSettingsContent />;
    case 'computer-use':
      return <ComputerUseSettingsContent />;
    case 'system':
      return <SystemModalContent />;
    default: {
      const exhaustive: never = route;
      return exhaustive;
    }
  }
};

const SystemSettings: React.FC = () => {
  const location = useLocation();
  const route = resolveSystemSettingsRoute(location.pathname);

  return (
    <SettingsPageWrapper contentClassName={route === 'about' ? 'max-w-640px' : undefined}>
      <Suspense fallback={<SettingsContentLoading />}>
        <SystemSettingsPanel route={route} />
      </Suspense>
    </SettingsPageWrapper>
  );
};

export default SystemSettings;
