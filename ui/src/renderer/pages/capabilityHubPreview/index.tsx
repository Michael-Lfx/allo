import { useState, type FC } from 'react';
import { useTranslation } from 'react-i18next';
import { getColorLabTokens, tokensToStyle, type ColorLabMode } from '../colorLab/palettes';
import HubMock from './HubMock';
import styles from './capabilityHubPreview.module.css';
import type { HubId, HubPreviewVariant, HubView } from './fixtures';

const CapabilityHubPreviewPage: FC = () => {
  const { t } = useTranslation();
  const [variant, setVariant] = useState<HubPreviewVariant>('proposed');
  const [mode, setMode] = useState<ColorLabMode>('light');
  const [hub, setHub] = useState<HubId>('skills');
  const [view, setView] = useState<HubView>('market');
  const tokens = getColorLabTokens(variant === 'proposed' ? 'forest' : 'now', mode);

  return (
    <div className={styles.page} style={tokensToStyle(tokens)} data-testid='capability-hub-preview'>
      <div className={styles.inner}>
        <header className={styles.chrome}>
          <div>
            <h1 className={styles.title}>{t('capabilityHubPreview.title')}</h1>
            <p className={styles.subtitle}>{t('capabilityHubPreview.subtitle')}</p>
          </div>
          <div className={styles.controls}>
            <button
              type='button'
              className={styles.toggle}
              aria-pressed={variant === 'now'}
              onClick={() => setVariant('now')}
            >
              {t('capabilityHubPreview.now')}
            </button>
            <button
              type='button'
              className={styles.toggle}
              aria-pressed={variant === 'proposed'}
              onClick={() => setVariant('proposed')}
            >
              {t('capabilityHubPreview.proposed')}
            </button>
            <button
              type='button'
              className={styles.toggle}
              aria-pressed={mode === 'light'}
              onClick={() => setMode('light')}
            >
              {t('capabilityHubPreview.light')}
            </button>
            <button
              type='button'
              className={styles.toggle}
              aria-pressed={mode === 'dark'}
              onClick={() => setMode('dark')}
            >
              {t('capabilityHubPreview.dark')}
            </button>
          </div>
        </header>
        <p className={styles.hint}>{t('capabilityHubPreview.hint')}</p>
        <HubMock variant={variant} hub={hub} view={view} onHubChange={setHub} onViewChange={setView} />
      </div>
    </div>
  );
};

export default CapabilityHubPreviewPage;
