import type { ToolChipItem, ToolChipStatus } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import type { TFunction } from 'i18next';
import { TOOL_CHIP_STATUSES, type ToolChipContentMode } from './catalog';

const typicalNames = ['writeFile', 'runCommand', 'search'] as const;

export const buildToolChipItems = (
  status: ToolChipStatus,
  content: ToolChipContentMode,
  t: TFunction
): ToolChipItem[] => {
  if (content === 'empty') return [];
  if (content === 'mixed') {
    return TOOL_CHIP_STATUSES.map((chipStatus, index) => {
      const name = typicalNames[index % typicalNames.length];
      return {
        id: chipStatus,
        name: t(`beautifulUiPreview.fixtures.toolChips.${name}.name` as const),
        detail: t(`beautifulUiPreview.toolChipStatuses.${chipStatus}` as const),
        status: chipStatus,
      };
    });
  }
  if (content === 'long') {
    return [
      {
        id: 'long',
        name: t('beautifulUiPreview.fixtures.toolChips.long.name'),
        detail: t('beautifulUiPreview.fixtures.toolChips.long.detail'),
        status,
      },
    ];
  }
  return typicalNames.map((item) => ({
    id: item,
    name: t(`beautifulUiPreview.fixtures.toolChips.${item}.name` as const),
    detail: t(`beautifulUiPreview.fixtures.toolChips.${item}.detail` as const),
    status,
  }));
};
