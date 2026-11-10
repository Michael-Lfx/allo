import React from 'react';
import { FlowyIcon } from '@renderer/components/base/FlowyIcon';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import styles from './GuidHeader.module.css';
import { BRAND_NAME } from '@/renderer/config/brand';

const GuidHeader: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();

  return (
    <div className={styles.header}>
      <div className={styles.headerInner}>
        <div className={styles.headerLeft}>
          <div className={styles.logo}>
            <FlowyIcon name="lightning" size={24} />
            <span className={styles.logoText}>{BRAND_NAME}</span>
          </div>
        </div>
        <div className={styles.headerRight}>
          <button
            className={styles.quickActionButton}
            onClick={() => navigate('/settings')}
          >
            <FlowyIcon name="settings" size={16} />
            <span>{t('guid.header.settings', { defaultValue: '设置' })}</span>
          </button>
        </div>
      </div>
    </div>
  );
};

export default GuidHeader;
