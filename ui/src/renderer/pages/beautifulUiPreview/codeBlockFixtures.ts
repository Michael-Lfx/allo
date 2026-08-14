import type { TFunction } from 'i18next';

export type CodeBlockFixture = {
  filename: string;
  language: string;
  children: string;
};

export const buildCodeBlockFixture = (t: TFunction): CodeBlockFixture => ({
  filename: t('beautifulUiPreview.fixtures.codeBlock.filename'),
  language: t('beautifulUiPreview.fixtures.codeBlock.language'),
  children: t('beautifulUiPreview.fixtures.codeBlock.typical'),
});
