/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IProvider } from '@/common/config/storage';
import {
  catalogCreditRateForModel,
  formatCreditRateMultiplier,
} from '@/renderer/utils/model/creditRate';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';

type ModelCreditRateHintProps = {
  provider?: Pick<IProvider, 'models_detail'> | null;
  modelName?: string;
  className?: string;
};

/**
 * Secondary credit-multiplier hint for model picker rows.
 * Renders nothing when the catalog does not advertise a positive rate.
 * Parent rows should place this on the trailing edge (e.g. `justify-between`).
 */
const ModelCreditRateHint: React.FC<ModelCreditRateHintProps> = ({
  provider,
  modelName,
  className,
}) => {
  const { t } = useTranslation();
  const formatted = formatCreditRateMultiplier(catalogCreditRateForModel(provider, modelName));
  if (!formatted) return null;

  return (
    <span
      className={classNames(
        'shrink-0 text-11px leading-none text-t-tertiary tabular-nums tracking-tight opacity-80',
        className
      )}
      title={t('common.modelCreditRate', { rate: formatted })}
      aria-label={t('common.modelCreditRate', { rate: formatted })}
    >
      {formatted}
    </span>
  );
};

export default ModelCreditRateHint;
