import type { SlashCommandItem } from '@/common/chat/slash/types';
import { getActiveSlashTokenRange } from '@/common/chat/slash/launcher';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';

export function matchSlashQuery(input: string, caret?: number): string | null {
  return getActiveSlashTokenRange(input, caret)?.query ?? null;
}

function getSelectionBehavior(command: SlashCommandItem): 'execute' | 'insert' {
  if (command.selectionBehavior) {
    return command.selectionBehavior;
  }
  return command.kind === 'builtin' ? 'execute' : 'insert';
}

interface UseSlashCommandControllerOptions {
  input: string;
  commands: SlashCommandItem[];
  onExecuteBuiltin?: (name: string) => void;
  onSelectTemplate?: (name: string) => void;
}

export function useSlashCommandController(options: UseSlashCommandControllerOptions) {
  const { input, commands, onExecuteBuiltin, onSelectTemplate } = options;
  const query = useMemo(() => matchSlashQuery(input), [input]);
  const [activeIndex, setActiveIndex] = useState(0);
  // Track which query was last dismissed. When query differs from the
  // last-dismissed query, the menu is eligible to open (subject to
  // filteredCommands being non-empty). This replaces the previous
  // boolean + useEffect pattern and avoids a double-render on every
  // query change.
  const [dismissedQuery, setDismissedQuery] = useState<string | null>(null);

  // Track previous query to reset activeIndex on query change.
  // We track this as a ref to avoid re-renders, and use a layout
  // effect to reset when it changes.
  const prevQueryRef = useRef(query);

  useEffect(() => {
    if (prevQueryRef.current !== query) {
      prevQueryRef.current = query;
      setActiveIndex(0);
    }
  }, [query]);

  const filteredCommands = useMemo(() => {
    if (query === null) {
      return [];
    }
    const keyword = query.trim().toLowerCase();
    if (!keyword) {
      return commands;
    }
    return commands.filter((command) => command.name.toLowerCase().startsWith(keyword));
  }, [commands, query]);

  // Menu is open when:
  // 1. There is an active slash query
  // 2. The current query hasn't been explicitly dismissed
  // 3. There are matching commands to show
  const isOpen = query !== null && dismissedQuery !== query && filteredCommands.length > 0;

  const dismiss = useCallback(() => {
    setDismissedQuery(query);
  }, [query]);

  const executeCommand = useCallback(
    (index: number) => {
      const command = filteredCommands[index];
      if (!command) {
        return false;
      }
      if (getSelectionBehavior(command) === 'insert') {
        onSelectTemplate?.(command.name);
      } else if (command.kind === 'builtin') {
        onExecuteBuiltin?.(command.name);
      } else {
        onSelectTemplate?.(command.name);
      }
      setDismissedQuery(query);
      return true;
    },
    [filteredCommands, onExecuteBuiltin, onSelectTemplate, query]
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
        setActiveIndex((prev) => (prev + 1) % filteredCommands.length);
        return true;
      }

      if (event.key === 'ArrowUp') {
        event.preventDefault();
        setActiveIndex((prev) => (prev - 1 + filteredCommands.length) % filteredCommands.length);
        return true;
      }

      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        return executeCommand(activeIndex);
      }

      return false;
    },
    [activeIndex, executeCommand, filteredCommands.length, isOpen, query]
  );

  return {
    isOpen,
    activeIndex,
    filteredCommands,
    onKeyDown,
    onSelectByIndex: executeCommand,
    dismiss,
    setActiveIndex,
  };
}
