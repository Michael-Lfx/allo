import React from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';
import ModalWrapper from '@renderer/components/base/ModalWrapper';

type CloudSessionExpiredModalProps = {
  visible: boolean;
  onCancel: () => void;
  onRelogin: () => void;
};

const CloudSessionExpiredModal: React.FC<CloudSessionExpiredModalProps> = ({
  visible,
  onCancel,
  onRelogin,
}) => {
  const { t } = useTranslation();
  return (
    <ModalWrapper
      visible={visible}
      title={t('common.cloudSessionExpired.title', { defaultValue: '登录已失效' })}
      footer={null}
      maskClosable
      unmountOnExit
      className='w-[min(400px,calc(100vw-24px))] max-w-400px rd-16px'
      onCancel={onCancel}
    >
      <p className='m-0 text-13px leading-22px text-[var(--color-text-2)]'>
        {t('common.cloudSessionExpired.description', {
          defaultValue: '云端登录已过期，请重新登录后继续。',
        })}
      </p>
      <div className='mt-16px flex justify-end gap-8px'>
        <Button onClick={onCancel}>{t('common.cancel', { defaultValue: '取消' })}</Button>
        <Button type='primary' onClick={onRelogin}>
          {t('common.cloudSessionExpired.relogin', { defaultValue: '重新登录' })}
        </Button>
      </div>
    </ModalWrapper>
  );
};

export default CloudSessionExpiredModal;
