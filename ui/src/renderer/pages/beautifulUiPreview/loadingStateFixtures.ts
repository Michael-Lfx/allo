import type { LoadingStateVariant } from '@renderer/components/beautifulUi/loadingState/LoadingState';
import type { TFunction } from 'i18next';

export type LoadingStateFixture = {
  variant: LoadingStateVariant;
  label: string;
  elapsedSeconds: number;
};

const labelKey = (variant: LoadingStateVariant) => {
  switch (variant) {
    case 'drive':
      return 'beautifulUiPreview.fixtures.loadingState.drive' as const;
    case 'dots':
      return 'beautifulUiPreview.fixtures.loadingState.dots' as const;
    case 'orbit':
      return 'beautifulUiPreview.fixtures.loadingState.orbit' as const;
    default: {
      const exhaustive: never = variant;
      return exhaustive;
    }
  }
};

export const buildLoadingStateFixture = (
  variant: LoadingStateVariant,
  t: TFunction
): LoadingStateFixture => ({
  variant,
  label: t(labelKey(variant)),
  elapsedSeconds: 4,
});
