import React from 'react';
import { useTranslation } from 'react-i18next';
import { Modal } from '@arco-design/web-react';
import type { StorePresetTemplate } from './types';
import { pickI18n } from './i18n';
import './PresetStoreDetail.css';

interface PresetStoreDetailProps {
  template: StorePresetTemplate | null;
  visible: boolean;
  onClose: () => void;
  isInstalled: boolean;
  onInstall: (template: StorePresetTemplate) => void;
}

const PresetStoreDetail: React.FC<PresetStoreDetailProps> = ({ template, visible, onClose, isInstalled, onInstall }) => {
  const { t, i18n } = useTranslation();
  if (!template) return null;

  const locale = i18n.language;
  const displayName = pickI18n(template.name, template.name_i18n, locale);
  const displayDesc = pickI18n(template.description, template.description_i18n, locale);

  return (
    <Modal
      title={displayName}
      visible={visible}
      onCancel={onClose}
      footer={null}
      closable
      maskClosable
      className='preset-store-detail-modal'
    >
      <div className='preset-store-detail'>
        <div className='preset-store-detail__header'>
          <span className='preset-store-detail__avatar'>{template.avatar}</span>
          <div className='preset-store-detail__info'>
            <h2 className='preset-store-detail__name'>{displayName}</h2>
            <span className='preset-store-detail__meta'>
              {template.installCount.toLocaleString()} {t('settings.presetStore.installs')}
            </span>
          </div>
        </div>

        <p className='preset-store-detail__desc'>{displayDesc}</p>

        <div className='preset-store-detail__section'>
          <h4>{t('settings.presetStore.skills')}</h4>
          <div className='preset-store-detail__tags'>
            {template.included_skills.map((skill) => (
              <span key={skill} className='preset-store-detail__tag'>{skill}</span>
            ))}
          </div>
        </div>

        <div className='preset-store-detail__section'>
          <h4>{t('settings.presetStore.instructions')}</h4>
          <pre className='preset-store-detail__instructions'>{template.instructions}</pre>
        </div>

        <button
          className='preset-store-detail__install-btn'
          disabled={isInstalled}
          onClick={() => { if (!isInstalled) { onInstall(template); onClose(); } }}
        >
          {isInstalled
            ? t('settings.presetStore.installed')
            : t('settings.presetStore.install')}
        </button>
      </div>
    </Modal>
  );
};

export default PresetStoreDetail;
