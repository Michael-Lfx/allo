/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useMemo, useState } from 'react';
import { COMMERCIAL_PATH_FRAMES, type CommercialPathState } from './commercialPathModel';
import { COMMERCIAL_SLICE_FLAG, isCommercialSliceEnabled } from '@renderer/utils/featureFlags/commercialSlice';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import './commercialSlice.css';

const CommercialSlicePage: React.FC = () => {
  const enabled = isCommercialSliceEnabled();
  const [state, setState] = useState<CommercialPathState>('first_user');
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
        <h1 className='flowy-type-title'>核心链路高保真原型</h1>
        <p className='flowy-type-body'>覆盖首次 / 回访 / 缺模型 / 网络失败 / 模型失败 / 成功六态，不单做理想路径。</p>
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
                trackFunnelEvent('first_value_confirmed', { source: 'prototype' });
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
              {frame.scene === 'execution' ? <div className='commercial-slice__progress' /> : null}
              {frame.scene === 'result' ? (
                <ul className='commercial-slice__files flowy-task-reveal'>
                  <li>diff · src/app.ts</li>
                  <li>summary · 首任务成果</li>
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
