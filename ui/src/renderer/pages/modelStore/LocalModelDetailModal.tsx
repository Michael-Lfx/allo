/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Modal } from '@arco-design/web-react';
import type { LocalModelRunStatus } from '@common/types/localModel';
import type { ModelStoreModel } from './types';
import { formatSize } from './format';

interface LocalModelDetailModalProps {
  visible: boolean;
  model?: ModelStoreModel;
  onClose: () => void;
  onOpenLocation: (model: ModelStoreModel) => void;
}

const STATUS_KEY: Record<LocalModelRunStatus, string> = {
  uninstalled: 'modelStore.statusTag.uninstalled',
  downloading: 'modelStore.statusTag.downloading',
  installed: 'modelStore.statusTag.installed',
  starting: 'modelStore.statusTag.starting',
  running: 'modelStore.statusTag.running',
};

const DetailRow: React.FC<{ label: string; value: React.ReactNode }> = ({ label, value }) => (
  <div className='flex min-w-0 flex-col gap-2px'>
    <span className='text-11px text-t-tertiary'>{label}</span>
    <span className='min-w-0 break-words text-13px text-t-primary'>{value || '—'}</span>
  </div>
);

/** Model detail dialog: two-column metadata + open-location action. */
const LocalModelDetailModal: React.FC<LocalModelDetailModalProps> = ({
  visible,
  model,
  onClose,
  onOpenLocation,
}) => {
  const { t, i18n } = useTranslation();
  const name = useMemo(() => {
    if (!model) return '';
    const isChinese = i18n.language.toLowerCase().startsWith('zh');
    return (isChinese ? model.displayName.zh : model.displayName.en) || model.id;
  }, [i18n.language, model]);

  const installed = model ? model.status !== 'uninstalled' && model.status !== 'downloading' : false;
  const capabilities = model
    ? [...model.capabilities, ...model.scenarios].slice(0, 8).join(', ')
    : '';

  return (
    <Modal
      title={t('modelStore.detail.title')}
      visible={visible}
      unmountOnExit
      autoFocus={false}
      style={{ width: 640, maxWidth: '92vw' }}
      onCancel={onClose}
      footer={
        <div className='flex items-center justify-end gap-8px'>
          <Button disabled={!installed} onClick={() => model && onOpenLocation(model)}>
            {t('modelStore.detail.openLocation')}
          </Button>
        </div>
      }
    >
      {model && (
        <div className='flex flex-col gap-16px'>
          <div className='flex min-w-0 items-center gap-12px'>
            <span className='flex size-48px shrink-0 items-center justify-center overflow-hidden rounded-12px border border-solid border-[var(--color-border-1)] bg-[var(--color-bg-1)]'>
              {model.icon ? <img src={model.icon} alt='' className='size-full object-contain' /> : null}
            </span>
            <div className='min-w-0 flex-1'>
              <div className='truncate text-16px font-600 text-t-primary' title={name}>
                {name}
              </div>
              <div className='mt-2px text-12px text-t-secondary'>{model.id}</div>
            </div>
          </div>
          <p className='m-0 text-13px leading-20px text-t-secondary'>
            {(i18n.language.toLowerCase().startsWith('zh') ? model.description.zh : model.description.en) || ''}
          </p>
          <div className='grid grid-cols-2 gap-x-24px gap-y-14px'>
            <DetailRow label={t('modelStore.detail.name')} value={name} />
            <DetailRow label={t('modelStore.detail.status')} value={t(STATUS_KEY[model.status])} />
            <DetailRow label={t('modelStore.detail.source')} value={model.id} />
            <DetailRow label={t('modelStore.detail.type')} value={model.type} />
            <DetailRow
              label={t('modelStore.detail.parameters')}
              value={model.parameterCount > 0 ? `${model.parameterCount}B` : '—'}
            />
            <DetailRow label={t('modelStore.detail.size')} value={formatSize(model.sizeBytes)} />
            <DetailRow label={t('modelStore.detail.vendor')} value={model.author ?? '—'} />
            <DetailRow label={t('modelStore.detail.engine')} value={model.engines.join(', ')} />
            <DetailRow label={t('modelStore.detail.installPath')} value={model.installPath ?? '—'} />
            <DetailRow label={t('modelStore.detail.mainGguf')} value={model.mainGguf} />
            <DetailRow label={t('modelStore.detail.mmproj')} value={model.mmproj ?? '—'} />
            <DetailRow label={t('modelStore.detail.capabilities')} value={capabilities} />
            {model.baseUrl && <DetailRow label={t('modelStore.detail.baseUrl')} value={model.baseUrl} />}
          </div>
        </div>
      )}
    </Modal>
  );
};

export default LocalModelDetailModal;
