import type { ISkillMarketItem } from '@/common/adapter/ipcBridge';

export type MarketActionState = 'checking' | 'ready' | 'pending' | 'completed' | 'error';

/** The only market-specific decision owned by a page consumer. */
export type MarketPrimaryActionConfig = {
  label: string;
  pendingLabel: string;
  completedLabel?: string;
  /** Allow the primary action to be used again for an installed item. */
  runWhenCompleted?: boolean;
  resolveState?: (item: ISkillMarketItem) => Exclude<MarketActionState, 'pending'>;
  run: (item: ISkillMarketItem) => void | Promise<void>;
};
