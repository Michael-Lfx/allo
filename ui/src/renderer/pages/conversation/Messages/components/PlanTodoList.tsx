/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import type { PinnedPlanData } from './pinnedPlanModel';
import styles from './planTodoList.module.css';

type PlanTodoEntry = PinnedPlanData['entries'][number];

export type PlanTodoListVariant = 'compact' | 'panel';

export type PlanOrbState = 'working' | 'settled';

type PlanTodoListProps = {
  entries: PinnedPlanData['entries'];
  variant: PlanTodoListVariant;
  listTestId?: string;
  inProgressRowRef?: React.Ref<HTMLDivElement>;
};

export const PlanThinkingOrb: React.FC<{ state: PlanOrbState; className?: string }> = ({ state, className }) => (
  <span
    className={`${styles.node} ${state === 'settled' ? styles.nodeSettled : styles.nodeWorking} ${className ?? ''}`.trim()}
    data-testid='conversation-plan-orb'
    data-state={state}
    aria-hidden='true'
  />
);

const statusRowClass = (status: PlanTodoEntry['status']): string => {
  switch (status) {
    case 'in_progress':
      return `${styles.row} ${styles.inProgress}`;
    case 'completed':
      return `${styles.row} ${styles.completed}`;
    case 'pending':
      return `${styles.row} ${styles.pending}`;
    default: {
      const _exhaustive: never = status;
      return _exhaustive;
    }
  }
};

const PlanTodoStatusGlyph: React.FC<{ status: PlanTodoEntry['status'] }> = ({ status }) => {
  switch (status) {
    case 'completed':
      return <PlanThinkingOrb state='settled' />;
    case 'in_progress':
      return <PlanThinkingOrb state='working' />;
    case 'pending':
      return <span className={styles.orbPending} />;
    default: {
      const _exhaustive: never = status;
      return _exhaustive;
    }
  }
};

const PlanTodoList: React.FC<PlanTodoListProps> = ({ entries, variant, listTestId, inProgressRowRef }) => (
  <div
    data-testid={listTestId}
    className={variant === 'compact' ? `${styles.list} ${styles.compact}` : `${styles.list} ${styles.panel}`}
  >
    {entries.map((item, index) => {
      const isInProgress = item.status === 'in_progress';
      return (
        <div
          key={index}
          ref={isInProgress ? inProgressRowRef : undefined}
          data-status={item.status}
          className={statusRowClass(item.status)}
        >
          <span className={styles.glyph}>
            <PlanTodoStatusGlyph status={item.status} />
          </span>
          <span className={styles.content}>{item.content}</span>
        </div>
      );
    })}
  </div>
);

export default PlanTodoList;
