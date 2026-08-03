

import NomiModal from '@/renderer/components/base/NomiModal';
import React from 'react';
import { useTranslation } from 'react-i18next';

const COPYRIGHT = '© 2025-2026 Flowy';

type FeedbackReportModalProps = {
  visible: boolean;
  onCancel: () => void;
};

/**
 * “联系我们”面板：不再展示外部官网 / GitHub / 联系页链接。
 */
const FeedbackReportModal: React.FC<FeedbackReportModalProps> = ({ visible, onCancel }) => {
  const { t } = useTranslation();

  return (
    <NomiModal
      title={t('settings.contactTitle')}
      visible={visible}
      onCancel={onCancel}
      onOk={onCancel}
      okText={t('common.confirm', { defaultValue: '确定' })}
      cancelButtonProps={{ style: { display: 'none' } }}
      alignCenter
      className='w-[min(460px,calc(100vw-32px))] max-w-460px rd-16px'
      autoFocus={false}
      wrapStyle={{ zIndex: 1050 }}
      maskStyle={{ zIndex: 1050 }}
    >
      <div className='px-24px pb-8px pt-2px'>
        <p className='m-0 text-13px leading-20px text-t-secondary'>
          {t('settings.contactDescription')}
        </p>
        <div className='mt-12px text-center text-12px text-t-tertiary'>{COPYRIGHT}</div>
      </div>
    </NomiModal>
  );
};

export default FeedbackReportModal;
