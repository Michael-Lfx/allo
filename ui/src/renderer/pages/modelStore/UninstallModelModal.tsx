/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Modal } from '@arco-design/web-react';
import type { ModelStoreModel } from './types';

interface UninstallModelModalProps {
  visible: boolean;
  model?: ModelStoreModel;
  loading?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

/**
 * Confirm dialog shown before uninstalling. Text switches on model state:
 * downloading → cancel download; running → stop then uninstall.
 */
const UninstallModelModal: React.FC<UninstallModelModalProps> = ({
  visible,
  model,
  loading,
  onCancel,
  onConfirm,
}) => {
  const { t, i18n } = useTranslation();
  const name = useMemo(() => {
    if (!model) return '';
    const isChinese = i18n.language.toLowerCase().startsWith('zh');
    return (isChinese ? model.displayName.zh : model.displayName.en) || model.id;
  }, [i18n.language, model]);

  const isDownloading = model?.status === 'downloading';
  const isRunning = model?.status === 'running' || model?.status === 'starting';

  const title = isDownloading
    ? t('modelStore.uninstall.cancelTitle')
    : t('modelStore.uninstall.title');

  const body = isDownloading
    ? t('modelStore.uninstall.cancelBody', { name })
    : isRunning
      ? t('modelStore.uninstall.runningBody', { name })
      : t('modelStore.uninstall.body', { name });

  const confirmText = isDownloading
    ? t('modelStore.actions.cancelDownload')
    : t('modelStore.actions.uninstall');

  return (
    <Modal
      title={title}
      visible={visible}
      unmountOnExit
      focusLock
      autoFocus={false}
      maskClosable={!loading}
      escToExit={!loading}
      onCancel={() => {
        if (!loading) onCancel();
      }}
      footer={
        <div className='flex justify-end gap-8px'>
          <Button disabled={loading} onClick={onCancel}>
            {t('common.cancel')}
          </Button>
          <Button type='primary' status='danger' loading={loading} onClick={onConfirm}>
            {confirmText}
          </Button>
        </div>
      }
    >
      <p className='m-0 text-14px leading-22px text-t-secondary'>{body}</p>
    </Modal>
  );
};

export default UninstallModelModal;
