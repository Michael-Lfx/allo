/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import { formatPhaseDuration, phaseLabelKey, toolPhaseBreakdown } from './toolPhases';
import type { ProjectedToolExecution } from './useAgentTraces';

const ToolPhaseBreakdown: React.FC<{ tool: ProjectedToolExecution }> = ({ tool }) => {
  const { t } = useTranslation();
  const breakdown = toolPhaseBreakdown(tool);
  if (!breakdown) return null;

  return (
    <div className='session-logs-phases'>
      <div className='session-logs-phases__head'>
        <span>{t('conversation.agentTrace.phaseTitle')}</span>
        <span className='session-logs-phases__total'>{formatPhaseDuration(breakdown.totalUs)}</span>
      </div>
      <ul className='session-logs-phases__list'>
        {breakdown.rows.map((row, index) => (
          <li
            key={`${row.name}-${index}`}
            className={row.nested ? 'session-logs-phases__row is-nested' : 'session-logs-phases__row'}
            title={row.name}
          >
            <span className='session-logs-phases__name'>
              {t(phaseLabelKey(row.name), { defaultValue: row.name })}
            </span>
            <span className='session-logs-phases__track' aria-hidden='true'>
              <span
                className='session-logs-phases__bar'
                style={{ width: `${Math.max(row.share * 100, row.us > 0 ? 1 : 0)}%` }}
              />
            </span>
            <span className='session-logs-phases__value'>{formatPhaseDuration(row.us)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
};

export default ToolPhaseBreakdown;
