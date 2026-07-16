import React from 'react';
import { useTranslation } from 'react-i18next';
import type { StorePresetTemplate } from './types';
import { pickI18n } from './i18n';
import './PresetStoreCard.css';

interface PresetStoreCardProps {
  template: StorePresetTemplate;
  installing: boolean;
  isInstalled: boolean;
  onInstall: (template: StorePresetTemplate) => void;
  onDetail: (template: StorePresetTemplate) => void;
}

const PresetStoreCard: React.FC<PresetStoreCardProps> = ({ template, installing, isInstalled, onInstall, onDetail }) => {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const displayName = pickI18n(template.name, template.name_i18n, locale);
  const displayDesc = pickI18n(template.description, template.description_i18n, locale);

  const disabled = installing || isInstalled;

  return (
    <div
      className='preset-store-card'
      onClick={() => onDetail(template)}
      role='button'
      tabIndex={0}
      onKeyDown={(e) => { if (e.key === 'Enter') onDetail(template); }}
    >
      <div className='preset-store-card__header'>
        <span className='preset-store-card__avatar'>{template.avatar}</span>
        <div className='preset-store-card__info'>
          <span className='preset-store-card__name'>{displayName}</span>
          <span className='preset-store-card__install-count'>
            {template.installCount.toLocaleString()} {t('settings.presetStore.installs')}
          </span>
        </div>
      </div>
      <p className='preset-store-card__desc'>{displayDesc}</p>
      <div className='preset-store-card__footer'>
        <span className='preset-store-card__skills'>
          {template.included_skills.slice(0, 2).join(', ')}
          {template.included_skills.length > 2 && '...'}
        </span>
        <button
          className='preset-store-card__install-btn'
          disabled={disabled}
          onClick={(e) => { if (!disabled) { e.stopPropagation(); onInstall(template); } }}
        >
          {isInstalled
            ? t('settings.presetStore.installed')
            : t('settings.presetStore.install')}
        </button>
      </div>
    </div>
  );
};

export default PresetStoreCard;
