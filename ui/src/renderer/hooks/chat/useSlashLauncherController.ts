import {
  filterSlashLauncherItems,
  type SlashLauncherItem,
} from '@/common/chat/slash/launcher';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import { matchSlashQuery } from './useSlashCommandController';

interface UseSlashLauncherControllerOptions {
  input: string;
  items: SlashLauncherItem[];
  onExecuteSystem: (item: SlashLauncherItem) => void;
  onSelectSkill: (item: SlashLauncherItem) => void;
  onSelectAgent: (item: SlashLauncherItem) => void;
}

export function useSlashLauncherController(options: UseSlashLauncherControllerOptions) {
  const { input, items, onExecuteSystem, onSelectSkill, onSelectAgent } = options;
  const query = useMemo(() => matchSlashQuery(input), [input]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [dismissedQuery, setDismissedQuery] = useState<string | null>(null);
  const previousQuery = useRef(query);

  useEffect(() => {
    if (previousQuery.current !== query) {
      previousQuery.current = query;
      setActiveIndex(0);
    }
  }, [query]);

  const filteredItems = useMemo(
    () => (query === null ? [] : filterSlashLauncherItems(items, query)),
    [items, query],
  );
  const isOpen = query !== null && dismissedQuery !== query && filteredItems.length > 0;

  const selectByIndex = useCallback(
    (index: number) => {
      const item = filteredItems[index];
      if (!item) {
        return false;
      }
      if (item.kind === 'system') {
        onExecuteSystem(item);
      } else if (item.kind === 'skill') {
        onSelectSkill(item);
      } else {
        onSelectAgent(item);
      }
      setDismissedQuery(query);
      return true;
    },
    [filteredItems, onExecuteSystem, onSelectAgent, onSelectSkill, query],
  );

  const onKeyDown = useCallback(
    (event: ReactKeyboardEvent) => {
      if (!isOpen) {
        return false;
      }
      if (event.key === 'Escape') {
        event.preventDefault();
        setDismissedQuery(query);
        return true;
      }
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        setActiveIndex((current) => (current + 1) % filteredItems.length);
        return true;
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault();
        setActiveIndex((current) => (current - 1 + filteredItems.length) % filteredItems.length);
        return true;
      }
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        return selectByIndex(activeIndex);
      }
      return false;
    },
    [activeIndex, filteredItems.length, isOpen, query, selectByIndex],
  );

  return {
    activeIndex,
    filteredItems,
    isOpen,
    onKeyDown,
    onSelectByIndex: selectByIndex,
    setActiveIndex,
  };
}
