/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useMemo, useState } from 'react';
import { COMMERCIAL_PATH_FRAMES, type CommercialPathState } from './commercialPathModel';
import { COMMERCIAL_SLICE_FLAG, isCommercialSliceEnabled } from '@renderer/utils/featureFlags/commercialSlice';
import { confirmFirstValue, trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import { markFirstWinCompleted } from '@renderer/utils/onboarding/firstWinMode';
import FirstWinOutcomeCard from '@renderer/pages/conversation/Messages/components/FirstWinOutcomeCard';
import type { FirstWinOutcomeSnapshot } from '@renderer/pages/conversation/Messages/components/firstWinOutcomeModel';
import './commercialSlice.css';

const PROTOTYPE_OUTCOME: FirstWinOutcomeSnapshot = {
  status: 'with_changes',
  summary: '根因是空指针守卫缺失。已补齐校验，相关测试已通过。',
  files: [
    { name: 'app.ts', path: 'src/app.ts', insertions: 12, deletions: 3 },
    { name: 'app.test.ts', path: 'src/app.test.ts', insertions: 8, deletions: 0 },
  ],
  hasAssistantAnswer: true,
};

const CommercialSlicePage: React.FC = () => {
  const enabled = isCommercialSliceEnabled();
  const [state, setState] = useState<CommercialPathState>('returning_user');
  const [outcomeDismissed, setOutcomeDismissed] = useState(false);
  const frame = useMemo(() => COMMERCIAL_PATH_FRAMES.find((item) => item.state === state)!, [state]);

  if (!enabled) {
    return (
      <div className='commercial-slice' data-testid='commercial-slice-disabled'>
        <p>
          Commercial slice flag is off. Set localStorage key {COMMERCIAL_SLICE_FLAG}=1 to enable.
        </p>
      </div>
    );
  }

  return (
    <div className='commercial-slice flowy-density-shell' data-testid='commercial-slice'>
      <header className='commercial-slice__header'>
        <h1 className='flowy-type-title'>成果启动台高保真原型</h1>
        <p className='flowy-type-body'>
          覆盖就绪 / 缺模型 / 缺项目 / 执行失败 / 首个成果。验证前置预检与自动续接，而非理想路径演示。
        </p>
      </header>

      <div className='commercial-slice__tabs' role='tablist' aria-label='Path states'>
        {COMMERCIAL_PATH_FRAMES.map((item) => (
          <button
            key={item.state}
            type='button'
            role='tab'
            aria-selected={item.state === state}
            className={item.state === state ? 'is-active' : undefined}
            data-testid={`commercial-state-${item.state}`}
            onClick={() => {
              setState(item.state);
              setOutcomeDismissed(false);
              if (item.state === 'task_success') {
                trackFunnelEvent('first_artifact_visible', { source: 'prototype' });
              }
            }}
          >
            {item.state}
          </button>
        ))}
      </div>

      <section className='commercial-slice__stage flowy-surface-card flowy-enter' data-scene={frame.scene}>
        <div className='commercial-slice__preview'>
          <div className='commercial-slice__preview-rail' aria-hidden='true' />
          <div className='commercial-slice__preview-main'>
            <div className={`commercial-slice__scene commercial-slice__scene--${frame.scene}`}>
              <span className='commercial-slice__badge' aria-hidden='true'>
                {frame.scene}
              </span>
              {frame.statusChips ? (
                <div className='commercial-slice__chips' data-testid='commercial-status-chips'>
                  {frame.statusChips.map((chip) => (
                    <span key={chip.id} className={`commercial-slice__chip is-${chip.state}`}>
                      {chip.label}
                    </span>
                  ))}
                </div>
              ) : null}
              {frame.planPreview ? (
                <p className='commercial-slice__plan' data-testid='commercial-plan-preview'>
                  {frame.planPreview}
                </p>
              ) : null}
              {frame.scene === 'execution' ? <div className='commercial-slice__progress' aria-hidden='true' /> : null}
              {frame.scene === 'result' && !outcomeDismissed ? (
                <div className='commercial-slice__outcome'>
                  <FirstWinOutcomeCard
                    snapshot={PROTOTYPE_OUTCOME}
                    onDismiss={() => {
                      confirmFirstValue({ source: 'prototype' });
                      markFirstWinCompleted();
                      setOutcomeDismissed(true);
                    }}
                  />
                </div>
              ) : null}
            </div>
          </div>
          <div className='commercial-slice__preview-workspace' aria-hidden='true' />
        </div>

        <div className='commercial-slice__copy'>
          <h2 className='flowy-type-title'>{frame.title}</h2>
          <p className='flowy-type-body'>{frame.body}</p>
          <div className='commercial-slice__actions'>
            <button type='button' className='commercial-slice__primary'>
              {frame.primaryAction}
            </button>
            {frame.secondaryAction ? (
              <button type='button' className='commercial-slice__secondary'>
                {frame.secondaryAction}
              </button>
            ) : null}
          </div>
        </div>
      </section>
    </div>
  );
};

export default CommercialSlicePage;
