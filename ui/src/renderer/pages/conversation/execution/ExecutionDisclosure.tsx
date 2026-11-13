import React, { useCallback, useEffect, useId, useState } from 'react';
import { Down } from '@icon-park/react';

type DisclosureStorage = Record<string, boolean>;

const STORAGE_PREFIX = 'nomifun:execution-panel-sections:';

const readStoredCollapsed = (storageKey: string, id: ExecutionDisclosureProps['id'], defaultCollapsed: boolean): boolean => {
  try {
    const raw = localStorage.getItem(storageKey);
    if (!raw) return defaultCollapsed;
    const state = JSON.parse(raw) as DisclosureStorage;
    return typeof state[id] === 'boolean' ? state[id] : defaultCollapsed;
  } catch {
    return defaultCollapsed;
  }
};

export type ExecutionDisclosureProps = {
  executionId?: string;
  id: 'subagents' | 'adjustment';
  defaultCollapsed: boolean;
  forceExpanded?: boolean;
  title: React.ReactNode;
  summary?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
};

/** A small keyboard-accessible disclosure used by secondary execution sections. */
const ExecutionDisclosure: React.FC<ExecutionDisclosureProps> = ({
  executionId,
  id,
  defaultCollapsed,
  forceExpanded = false,
  title,
  summary,
  children,
  className,
}) => {
  const contentId = useId();
  const storageKey = executionId ? `${STORAGE_PREFIX}${executionId}` : null;
  const [collapsed, setCollapsed] = useState(() =>
    storageKey ? readStoredCollapsed(storageKey, id, defaultCollapsed) : defaultCollapsed,
  );

  useEffect(() => {
    if (forceExpanded) setCollapsed(false);
  }, [forceExpanded]);

  useEffect(() => {
    if (!storageKey) return;
    try {
      const raw = localStorage.getItem(storageKey);
      const state = (raw ? JSON.parse(raw) : {}) as DisclosureStorage;
      state[id] = collapsed;
      localStorage.setItem(storageKey, JSON.stringify(state));
    } catch {
      // Storage is optional; the disclosure remains usable for this session.
    }
  }, [collapsed, id, storageKey]);

  const expanded = !collapsed;
  const toggle = useCallback(() => setCollapsed((value) => !value), []);

  return (
    <section className={className} data-testid={`execution-disclosure-${id}`}>
      <button
        type='button'
        className='flex w-full min-w-0 items-center justify-between gap-8px border-0 bg-transparent p-0 text-left focus-visible:outline-none focus-visible:ring-2px focus-visible:ring-[var(--flowy-focus)] focus-visible:ring-offset-1px'
        aria-expanded={expanded}
        aria-controls={contentId}
        onClick={toggle}
      >
        <span className='min-w-0 flex-1'>{title}</span>
        {summary ? <span className='shrink-0'>{summary}</span> : null}
        <Down
          theme='outline'
          size='14'
          aria-hidden='true'
          className={expanded ? 'shrink-0 rotate-180 transition-transform' : 'shrink-0 transition-transform'}
        />
      </button>
      {expanded ? (
        <div id={contentId} className='mt-6px'>
          {children}
        </div>
      ) : null}
    </section>
  );
};

export default ExecutionDisclosure;
