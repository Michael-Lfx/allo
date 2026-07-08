/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { verifyDeveloperModePassword } from '@/common/config/developerMode';
import { Button, Input, Modal } from '@arco-design/web-react';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

interface DeveloperModePasswordModalProps {
  visible: boolean;
  loading?: boolean;
  onClose: () => void;
  onConfirm: () => void | Promise<void>;
}

const DeveloperModePasswordModal: React.FC<DeveloperModePasswordModalProps> = ({
  visible,
  loading = false,
  onClose,
  onConfirm,
}) => {
  const { t } = useTranslation();
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!visible) {
      setPassword('');
      setError(null);
    }
  }, [visible]);

  const handleConfirm = async () => {
    if (loading) return;
    if (!verifyDeveloperModePassword(password)) {
      setError(t('settings.developerMode.passwordIncorrect'));
      return;
    }
    setError(null);
    await onConfirm();
  };

  return (
    <Modal
      title={t('settings.developerMode.passwordModalTitle')}
      visible={visible}
      onCancel={loading ? undefined : onClose}
      maskClosable={!loading}
      escToExit={!loading}
      autoFocus={false}
      focusLock
      footer={
        <div className='flex justify-end gap-8px'>
          <Button onClick={onClose} disabled={loading}>
            {t('common.cancel')}
          </Button>
          <Button type='primary' loading={loading} disabled={!password.trim()} onClick={handleConfirm}>
            {t('settings.developerMode.enableConfirm')}
          </Button>
        </div>
      }
    >
      <div className='space-y-12px'>
        <p className='text-13px text-t-secondary leading-22px m-0'>{t('settings.developerMode.passwordModalDesc')}</p>
        <Input.Password
          value={password}
          onChange={(value) => {
            setPassword(value);
            if (error) setError(null);
          }}
          placeholder={t('settings.developerMode.passwordPlaceholder')}
          disabled={loading}
          visibilityToggle
          autoComplete='off'
          onPressEnter={handleConfirm}
        />
        {error ? <div className='text-12px text-[rgb(var(--danger-6))] leading-20px'>{error}</div> : null}
      </div>
    </Modal>
  );
};

export default DeveloperModePasswordModal;
