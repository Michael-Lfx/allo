import { useCallback, useRef, useState } from 'react';
import type { MarketActionState, MarketPrimaryActionConfig } from './marketContracts';
import type { MarketItemViewModel } from './marketViewModel';

export const mergeMarketActionState = (
  resolved: Exclude<MarketActionState, 'pending'>,
  itemId: string,
  activeActionItemId: string | null,
): MarketActionState => (activeActionItemId === itemId ? 'pending' : resolved);

export const canRunMarketAction = (
  resolved: Exclude<MarketActionState, 'pending'>,
  runWhenCompleted = false,
): boolean =>
  resolved === 'ready' ||
  resolved === 'error' ||
  (resolved === 'completed' && runWhenCompleted);

/** An in-flight action whose card left the list must not block the cards still on screen. */
export const shouldReleaseHiddenMarketAction = (
  activeId: string | null,
  visibleIds: ReadonlySet<string>,
): boolean => Boolean(activeId && !visibleIds.has(activeId));

export const useMarketActionState = (primaryAction: MarketPrimaryActionConfig) => {
  const [activeActionItemId, setActiveActionItemId] = useState<string | null>(null);
  const activeActionItemIdRef = useRef<string | null>(null);

  const getState = useCallback(
    (viewModel: MarketItemViewModel): MarketActionState => {
      const resolved = primaryAction.resolveState?.(viewModel.raw) ?? 'ready';
      return mergeMarketActionState(resolved, viewModel.id, activeActionItemId);
    },
    [activeActionItemId, primaryAction]
  );

  const runPrimaryAction = useCallback(
    async (viewModel: MarketItemViewModel) => {
      if (activeActionItemIdRef.current) return;
      const resolved = primaryAction.resolveState?.(viewModel.raw) ?? 'ready';
      if (!canRunMarketAction(resolved, primaryAction.runWhenCompleted)) return;
      const actionId = viewModel.id;
      activeActionItemIdRef.current = actionId;
      setActiveActionItemId(actionId);
      try {
        await primaryAction.run(viewModel.raw);
      } finally {
        if (activeActionItemIdRef.current === actionId) {
          activeActionItemIdRef.current = null;
          setActiveActionItemId(null);
        }
      }
    },
    [primaryAction],
  );

  const releaseHiddenAction = useCallback((visibleIds: ReadonlySet<string>) => {
    if (!shouldReleaseHiddenMarketAction(activeActionItemIdRef.current, visibleIds)) return;
    activeActionItemIdRef.current = null;
    setActiveActionItemId(null);
  }, []);

  return {
    activeActionItemId,
    runPrimaryAction,
    releaseHiddenAction,
    getState,
    isBusy: (id: string) => activeActionItemId === id,
    isDisabled: (viewModel: MarketItemViewModel) => {
      const state = getState(viewModel);
      return state === 'checking' || state === 'pending' ||
        (state === 'completed' && !primaryAction.runWhenCompleted) ||
        (activeActionItemId !== null && activeActionItemId !== viewModel.id);
    },
  };
};
