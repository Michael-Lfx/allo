import { HERDSMAN_PLATFORM } from '@/common/types/provider/herdsman';
import { NOMIFUN_FREE_MODEL_PLATFORM } from '@/common/types/provider/managedModelService';

const modelSelectorProviderRank = (platform?: string): number => {
  if (platform === HERDSMAN_PLATFORM) return -1;
  if (platform === NOMIFUN_FREE_MODEL_PLATFORM) return 1;
  return 0;
};

/**
 * Keep the local Herdsman engine first, then supplier providers, then the
 * managed free provider. Relative priority inside each group remains exactly
 * as returned by the provider API.
 */
export const orderModelSelectorProviders = <T extends { platform?: string }>(providers: readonly T[]): T[] =>
  providers
    .map((provider, index) => ({ provider, index }))
    .sort((left, right) => {
      const rankDifference =
        modelSelectorProviderRank(left.provider.platform) - modelSelectorProviderRank(right.provider.platform);
      return rankDifference || left.index - right.index;
    })
    .map(({ provider }) => provider);
