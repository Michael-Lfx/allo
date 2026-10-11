/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useTranslation } from 'react-i18next';
import { Button, Modal } from '@arco-design/web-react';

interface StopModelModalProps {
  visible: boolean;
  modelName?: string;
  loading?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

/** Confirm dialog shown before stopping a running local model. */
const StopModelModal: React.FC<StopModelModalProps> = ({
  visible,
  modelName,
  loading,
  onCancel,
  onConfirm,
}) => {
  const { t } = useTranslation();
  return (
    <Modal
      title={t('modelStore.stop.title')}
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
            {t('modelStore.actions.stop')}
          </Button>
        </div>
      }
    >
      <p className='m-0 text-14px leading-22px text-t-secondary'>
        {t('modelStore.stop.body', { name: modelName ?? '' })}
      </p>
    </Modal>
  );
};

export default StopModelModal;
