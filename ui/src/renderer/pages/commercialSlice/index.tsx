/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useMemo, useState } from 'react';
import { COMMERCIAL_PATH_FRAMES, type CommercialPathState } from './commercialPathModel';
import { COMMERCIAL_SLICE_FLAG, isCommercialSliceEnabled } from '@renderer/utils/featureFlags/commercialSlice';
import { confirmFirstValue, trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import './commercialSlice.css';

const CommercialSlicePage: React.FC = () => {
  const enabled = isCommercialSliceEnabled();
  const [state, setState] = useState<CommercialPathState>('returning_user');
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
              if (item.state === 'task_success') {
                confirmFirstValue({ source: 'prototype' });
                trackFunnelEvent('first_artifact_visible', { source: 'prototype' });
              }
            }}
          >
            {item.state}
          </button>
        ))}
      </div>

      <section className='commercial-slice__stage flowy-surface-card flowy-enter' data-scene={frame.scene}>
        <div className='commercial-slice__preview' aria-hidden='true'>
          <div className='commercial-slice__preview-rail' />
          <div className='commercial-slice__preview-main'>
            <div className={`commercial-slice__scene commercial-slice__scene--${frame.scene}`}>
              <span className='commercial-slice__badge'>{frame.scene}</span>
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
              {frame.scene === 'execution' ? <div className='commercial-slice__progress' /> : null}
              {frame.scene === 'result' ? (
                <ul className='commercial-slice__files flowy-task-reveal'>
                  <li>verified · tests green</li>
                  <li>diff · src/app.ts</li>
                  <li>summary · 根因与修复说明</li>
                </ul>
              ) : null}
            </div>
          </div>
          <div className='commercial-slice__preview-workspace' />
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
