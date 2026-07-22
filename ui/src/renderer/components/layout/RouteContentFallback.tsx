/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';

/**
 * In-layout route Suspense fallback. Fills the content pane without a
 * full-viewport Spin, so the app shell (titlebar / sider) stays visible
 * during lazy chunk loads and route transitions feel continuous.
 */
const RouteContentFallback: React.FC = () => {
  return (
    <div
      className='flex flex-col flex-1 min-h-0 w-full size-full bg-base'
      data-testid='route-content-fallback'
      aria-busy='true'
      aria-live='polite'
    >
      <div className='flex flex-col gap-16px p-24px max-w-720px w-full mx-auto'>
        <div className='h-28px w-40% rd-8px bg-fill-2 animate-pulse' />
        <div className='h-16px w-70% rd-6px bg-fill-2 animate-pulse' />
        <div className='mt-8px h-160px w-full rd-12px bg-fill-2 animate-pulse' />
        <div className='flex gap-12px'>
          <div className='h-36px flex-1 rd-8px bg-fill-2 animate-pulse' />
          <div className='h-36px flex-1 rd-8px bg-fill-2 animate-pulse' />
          <div className='h-36px flex-1 rd-8px bg-fill-2 animate-pulse' />
        </div>
      </div>
    </div>
  );
};

export default RouteContentFallback;
