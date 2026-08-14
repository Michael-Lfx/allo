/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { TurnDisclosureProcessState } from '../turnDisclosureModel';
import { TaskGroup } from '@renderer/components/beautifulUi/taskRows/TaskRows';
import { resolveTaskGroupStatus } from '@renderer/components/beautifulUi/taskRows/taskRowModel';
import { groupTurnProcessItemsByCycle } from '../turnProcessCycleModel';
import { Down } from '@icon-park/react';
import classNames from 'classnames';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

export interface TurnProcessDisclosureView<T> {
  id: string;
  processItems: T[];
  startAt: number;
  endAt: number;
  state: TurnDisclosureProcessState;
  running: boolean;
  defaultCollapsed: boolean;
}

interface TurnProcessDisclosureProps<T> {
  item: TurnProcessDisclosureView<T>;
  highlighted?: boolean;
  renderProcessItem: (item: T, expansionControls?: TurnProcessDisclosureExpansionControls) => React.ReactNode;
  getProcessItemKey: (item: T) => string;
  getProcessItemState: (item: T) => TurnDisclosureProcessState;
  getProcessItemLayoutKind?: (item: T) => string;
  getProcessItemCanExpandAll?: (item: T) => boolean;
}

export interface TurnProcessDisclosureExpansionSnapshot {
  itemId: string;
  hasProcessItems: boolean;
  defaultCollapsed: boolean;
}

export interface TurnProcessDisclosureExpansionControls {
  expanded: boolean;
  onExpandedChange: (expanded: boolean) => void;
}

const labelKeyByState: Record<TurnDisclosureProcessState, string> = {
  completed: 'messages.turnProcessed',
  running: 'messages.turnProcessed',
  waiting: 'messages.turnWaiting',
  failed: 'messages.turnProcessed',
  canceled: 'messages.turnCanceled',
};

const defaultLabelByState: Record<TurnDisclosureProcessState, string> = {
  completed: 'Processed {{duration}}',
  running: 'Processed {{duration}}',
  waiting: 'Waiting for confirmation {{duration}}',
  failed: 'Processed {{duration}}',
  canceled: 'You stopped after {{duration}}',
};

const sanitizeDomId = (value: string): string => value.replace(/[^A-Za-z0-9_-]/g, '_');

const EMPTY_PROCESS_ITEM_KEYS: string[] = [];

const getDefaultExpanded = (hasProcessItems: boolean, defaultCollapsed: boolean): boolean =>
  hasProcessItems && !defaultCollapsed;

const areSameStringLists = (left: readonly string[], right: readonly string[]): boolean => {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) return false;
  }
  return true;
};

export function shouldResetTurnProcessDisclosureExpansion(
  previous: TurnProcessDisclosureExpansionSnapshot,
  next: TurnProcessDisclosureExpansionSnapshot
): boolean {
  if (previous.itemId !== next.itemId) return true;
  if (previous.hasProcessItems !== next.hasProcessItems) return true;
  if (previous.defaultCollapsed !== next.defaultCollapsed) return true;
  return false;
}

export function stabilizeTurnProcessDisclosureKeys(
  previous: readonly string[],
  next: readonly string[]
): readonly string[] {
  return areSameStringLists(previous, next) ? previous : next;
}

const formatTurnDuration = (ms: number, t: ReturnType<typeof useTranslation>['t']): string => {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const sUnit = t('common.unit.second_short', { defaultValue: 's' });
  const mUnit = t('common.unit.minute_short', { defaultValue: 'm' });
  const hUnit = t('common.unit.hour_short', { defaultValue: 'h' });

  if (totalSeconds < 60) return `${totalSeconds}${sUnit}`;
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes < 60) return `${minutes}${mUnit} ${seconds}${sUnit}`;
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return `${hours}${hUnit} ${remainingMinutes}${mUnit}`;
};

const TurnProcessDurationLabel: React.FC<{
  state: TurnDisclosureProcessState;
  startAt: number;
  endAt: number;
  running: boolean;
}> = ({ state, startAt, endAt, running }) => {
  const { t } = useTranslation();
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!running) return;
    setNow(Date.now());
    const timer = window.setInterval(() => {
      setNow(Date.now());
    }, 1000);
    return () => window.clearInterval(timer);
  }, [running]);

  const durationEndAt = running ? now : endAt;
  const displayState = state === 'failed' ? 'completed' : state;
  return t(labelKeyByState[displayState], {
    duration: formatTurnDuration(durationEndAt - startAt, t),
    defaultValue: defaultLabelByState[displayState],
  });
};

function TurnProcessDisclosure<T>({
  item,
  highlighted = false,
  renderProcessItem,
  getProcessItemKey,
  getProcessItemState,
  getProcessItemLayoutKind,
  getProcessItemCanExpandAll,
}: TurnProcessDisclosureProps<T>) {
  const { t } = useTranslation();
  const hasProcessItems = item.processItems.length > 0;
  const [expanded, setExpanded] = useState(() => getDefaultExpanded(hasProcessItems, item.defaultCollapsed));
  const [expandAllProcessItemKeys, setExpandAllProcessItemKeys] = useState<Set<string>>(() => new Set());
  const expansionSnapshotRef = useRef<TurnProcessDisclosureExpansionSnapshot>({
    itemId: item.id,
    hasProcessItems,
    defaultCollapsed: item.defaultCollapsed,
  });

  const expandableProcessItemKeysRef = useRef<readonly string[]>(EMPTY_PROCESS_ITEM_KEYS);

  useEffect(() => {
    const nextSnapshot: TurnProcessDisclosureExpansionSnapshot = {
      itemId: item.id,
      hasProcessItems,
      defaultCollapsed: item.defaultCollapsed,
    };
    const shouldReset = shouldResetTurnProcessDisclosureExpansion(expansionSnapshotRef.current, nextSnapshot);
    expansionSnapshotRef.current = nextSnapshot;

    if (shouldReset) {
      setExpanded(getDefaultExpanded(hasProcessItems, item.defaultCollapsed));
      // Bail out when already empty — `new Set()` is always a new reference.
      setExpandAllProcessItemKeys((previous) => (previous.size > 0 ? new Set() : previous));
    }
  }, [hasProcessItems, item.defaultCollapsed, item.id]);

  useEffect(() => {
    if (highlighted && hasProcessItems) setExpanded(true);
  }, [hasProcessItems, highlighted]);

  const currentItemKey = useMemo(() => {
    const activeItem = item.processItems.findLast((processItem) => {
      const state = getProcessItemState(processItem);
      return state === 'running' || state === 'waiting';
    });
    const failedItem =
      activeItem ??
      item.processItems.findLast((processItem) => {
        const state = getProcessItemState(processItem);
        return state === 'failed' || state === 'canceled';
      });
    const latestItem = failedItem ?? item.processItems.at(-1);
    return latestItem ? getProcessItemKey(latestItem) : undefined;
  }, [getProcessItemKey, getProcessItemState, item.processItems]);

  const expandableProcessItemKeys = useMemo(() => {
    const nextKeys = !getProcessItemCanExpandAll
      ? EMPTY_PROCESS_ITEM_KEYS
      : item.processItems.filter(getProcessItemCanExpandAll).map(getProcessItemKey);
    const stabilized = stabilizeTurnProcessDisclosureKeys(expandableProcessItemKeysRef.current, nextKeys);
    expandableProcessItemKeysRef.current = stabilized;
    return stabilized;
  }, [getProcessItemCanExpandAll, getProcessItemKey, item.processItems]);

  useEffect(() => {
    if (!expandableProcessItemKeys.length) {
      setExpandAllProcessItemKeys((previous) => (previous.size > 0 ? new Set() : previous));
      return;
    }

    const validKeys = new Set(expandableProcessItemKeys);
    setExpandAllProcessItemKeys((previous) => {
      const next = new Set([...previous].filter((key) => validKeys.has(key)));
      return next.size === previous.size ? previous : next;
    });
  }, [expandableProcessItemKeys]);

  const hasExpandableProcessItems = expandableProcessItemKeys.length > 0;
  const allExpandableProcessItemsExpanded =
    hasExpandableProcessItems && expandableProcessItemKeys.every((itemKey) => expandAllProcessItemKeys.has(itemKey));

  const handleToggleAllProcessItems = useCallback(() => {
    if (allExpandableProcessItemsExpanded) {
      setExpandAllProcessItemKeys((previous) => (previous.size > 0 ? new Set() : previous));
      return;
    }
    setExpandAllProcessItemKeys((previous) => {
      if (
        previous.size === expandableProcessItemKeys.length &&
        expandableProcessItemKeys.every((key) => previous.has(key))
      ) {
        return previous;
      }
      return new Set(expandableProcessItemKeys);
    });
  }, [allExpandableProcessItemsExpanded, expandableProcessItemKeys]);

  const getExpansionControls = useCallback(
    (itemKey: string): TurnProcessDisclosureExpansionControls => ({
      expanded: expandAllProcessItemKeys.has(itemKey),
      onExpandedChange: (nextExpanded) => {
        setExpandAllProcessItemKeys((previous) => {
          const hasKey = previous.has(itemKey);
          if (nextExpanded === hasKey) return previous;
          const next = new Set(previous);
          if (nextExpanded) {
            next.add(itemKey);
          } else {
            next.delete(itemKey);
          }
          return next;
        });
      },
    }),
    [expandAllProcessItemKeys]
  );

  // Defensive compatibility: historical or third-party callers may still
  // provide an aggregate `failed` state. The header must remain lifecycle-only;
  // detailed rows continue to render their own failure state below.
  const displayState = item.state === 'failed' ? 'completed' : item.state;
  const bodyId = `turn-process-disclosure-body-${sanitizeDomId(item.id)}`;
  const disclosureExpanded = hasProcessItems && expanded;
  const hasHeaderActions = disclosureExpanded && hasExpandableProcessItems;
  const processGroups = useMemo(
    () =>
      groupTurnProcessItemsByCycle(
        item.processItems,
        (processItem) => getProcessItemLayoutKind?.(processItem) ?? 'other'
      ),
    [getProcessItemLayoutKind, item.processItems]
  );

  const renderDisclosureItem = (processItem: T) => {
    const itemKey = getProcessItemKey(processItem);
    const state = getProcessItemState(processItem);
    const layoutKind = getProcessItemLayoutKind?.(processItem) ?? 'other';
    const expansionControls = getProcessItemCanExpandAll?.(processItem)
      ? getExpansionControls(itemKey)
      : undefined;
    const content = renderProcessItem(processItem, expansionControls);
    if (content == null) return null;
    return (
      <div
        key={itemKey}
        className={classNames(
          'turn-process-disclosure__item',
          `turn-process-disclosure__item--${layoutKind}`,
          `turn-process-disclosure__item--${state}`,
          itemKey === currentItemKey && 'turn-process-disclosure__item--current'
        )}
      >
        {content}
      </div>
    );
  };

  return (
    <div className={classNames('turn-process-disclosure', `turn-process-disclosure--${displayState}`)}>
      <div
        className={classNames(
          'turn-process-disclosure__header',
          hasHeaderActions && 'turn-process-disclosure__header--with-actions',
          !hasProcessItems && 'turn-process-disclosure__header--static'
        )}
      >
        <TaskGroup
          title={
            <TurnProcessDurationLabel
              state={item.state}
              startAt={item.startAt}
              endAt={item.endAt}
              running={item.running}
            />
          }
          status={resolveTaskGroupStatus(item.state)}
          expandable={hasProcessItems}
          expanded={disclosureExpanded}
          onToggle={hasProcessItems ? () => setExpanded((value) => !value) : undefined}
          ariaControls={bodyId}
          className={
            hasProcessItems
              ? 'turn-process-disclosure__toggle'
              : 'turn-process-disclosure__label turn-process-disclosure__label--static'
          }
        />
        {hasProcessItems && hasHeaderActions && (
          <div className='turn-process-disclosure__header-actions'>
            <button
              type='button'
              className='turn-process-disclosure__expand-thinking'
              onClick={handleToggleAllProcessItems}
            >
              <Down
                theme='outline'
                size='14'
                fill='currentColor'
                className={classNames(
                  'turn-process-disclosure__expand-thinking-icon',
                  allExpandableProcessItemsExpanded && 'turn-process-disclosure__expand-thinking-icon--open'
                )}
              />
              <span>
                {allExpandableProcessItemsExpanded
                  ? t('messages.turnProcess.collapseAllThinkingProcess', {
                      defaultValue: 'Collapse all thinking process',
                    })
                  : t('messages.turnProcess.expandAllThinkingProcess', {
                      defaultValue: 'Expand all thinking process',
                    })}
              </span>
            </button>
          </div>
        )}
      </div>
      {disclosureExpanded && (
        <div id={bodyId} className='turn-process-disclosure__body'>
          {processGroups.map((group) => {
            switch (group.type) {
              case 'item':
                return renderDisclosureItem(group.item);
              case 'cycle': {
                const [header, ...children] = group.items;
                if (!header) return null;
                return (
                  <div key={getProcessItemKey(header)} className='turn-process-disclosure__cycle'>
                    {renderDisclosureItem(header)}
                    {children.length > 0 ? (
                      <div className='turn-process-disclosure__cycle-body'>
                        {children.map(renderDisclosureItem)}
                      </div>
                    ) : null}
                  </div>
                );
              }
              default: {
                const exhaustive: never = group;
                return exhaustive;
              }
            }
          })}
        </div>
      )}
    </div>
  );
}

export default TurnProcessDisclosure;
