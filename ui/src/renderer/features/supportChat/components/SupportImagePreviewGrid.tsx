/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import type { SupportImagePreviewItem } from '../supportImageAttachments';

type SupportImagePreviewGridProps = {
  items: SupportImagePreviewItem[];
  disabled?: boolean;
  onRemove: (id: string) => void;
  className?: string;
};

const SupportImagePreviewGrid: React.FC<SupportImagePreviewGridProps> = ({
  items,
  disabled,
  onRemove,
  className = '',
}) => {
  const { t } = useTranslation();

  if (items.length === 0) return null;

  return (
    <div className={`support-image-preview-grid ${className}`}>
      {items.map((item) => (
        <div key={item.id} className='support-image-preview-item'>
          <img src={item.url} alt='' className='support-image-preview-item__image' />
          <button
            type='button'
            className='support-image-preview-item__remove'
            onClick={() => onRemove(item.id)}
            disabled={disabled}
            aria-label={t('common.supportChat.removeImage', { defaultValue: '移除图片' })}
          >
            <svg className='block size-16px' viewBox='0 0 16 16' fill='none' aria-hidden>
              <path
                d='M4 4l8 8M12 4 4 12'
                stroke='currentColor'
                strokeWidth='1.6'
                strokeLinecap='round'
              />
            </svg>
          </button>
        </div>
      ))}
    </div>
  );
};

export default SupportImagePreviewGrid;
