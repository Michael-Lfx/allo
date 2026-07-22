import { Spin } from '@arco-design/web-react';
import React from 'react';

type AppLoaderProps = {
  /** Fill the parent instead of forcing a full viewport swap (shell-safe). */
  fill?: boolean;
};

const AppLoader: React.FC<AppLoaderProps> = ({ fill = false }) => {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: '100%',
        height: fill ? '100%' : '100vh',
        minHeight: fill ? 0 : '100vh',
        background: 'var(--color-bg-1, var(--bg-1, transparent))',
      }}
    >
      <Spin dot />
    </div>
  );
};

export default AppLoader;
