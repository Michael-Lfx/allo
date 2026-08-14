import type { ApprovalKind, ApprovalOption } from '@renderer/components/beautifulUi/approvalCard/ApprovalCard';
import type { TFunction } from 'i18next';

export type ApprovalCardFixture = {
  title: string;
  description: string;
  options: ApprovalOption[];
  confirmLabel: string;
  child: string;
};

export const buildApprovalCardFixture = (kind: ApprovalKind, t: TFunction): ApprovalCardFixture => ({
  title: t('beautifulUiPreview.fixtures.approvalCard.title'),
  description: t(`beautifulUiPreview.fixtures.approvalCard.descriptions.${kind}` as const),
  options: [
    { id: 'pistachio', label: t('beautifulUiPreview.fixtures.approvalCard.options.pistachio') },
    { id: 'mint', label: t('beautifulUiPreview.fixtures.approvalCard.options.mint') },
    { id: 'cone', label: t('beautifulUiPreview.fixtures.approvalCard.options.cone') },
  ],
  confirmLabel: t('beautifulUiPreview.fixtures.approvalCard.confirm'),
  child: t(`beautifulUiPreview.fixtures.approvalCard.children.${kind}` as const),
});
