import { useMemo, useState, type FC } from 'react';
import { useTranslation } from 'react-i18next';
import ColorLabShell from './ColorLabShell';
import styles from './colorLab.module.css';
import {
  COLOR_LAB_IDS,
  getColorLabTokens,
  tokensToStyle,
  type ColorLabId,
  type ColorLabMode,
} from './palettes';

const TOKEN_KEYS = ['canvas', 'rail', 'accent', 'text'] as const;

const ColorLabPage: FC = () => {
  const { t } = useTranslation();
  const [scheme, setScheme] = useState<ColorLabId>('forest');
  const [mode, setMode] = useState<ColorLabMode>('light');
  const [compare, setCompare] = useState(false);
  const pageTokens = getColorLabTokens(compare ? 'now' : scheme, mode);
  const selectedTokens = getColorLabTokens(scheme, mode);
  const nowTokens = getColorLabTokens('now', mode);
  const schemeCards = useMemo(
    () =>
      COLOR_LAB_IDS.map((id) => ({
        id,
        tokens: getColorLabTokens(id, mode),
      })),
    [mode]
  );

  return (
    <div className={styles.page} style={tokensToStyle(pageTokens)} data-testid='color-lab'>
      <div className={styles.inner}>
        <header className={styles.header}>
          <div className={styles.copy}>
            <h1 className={styles.title}>{t('colorLab.title')}</h1>
            <p className={styles.subtitle}>{t('colorLab.subtitle')}</p>
          </div>
          <div className={styles.controls}>
            <button
              type='button'
              className={styles.modeBtn}
              aria-pressed={mode === 'light'}
              onClick={() => setMode('light')}
            >
              {t('colorLab.mode.light')}
            </button>
            <button
              type='button'
              className={styles.modeBtn}
              aria-pressed={mode === 'dark'}
              onClick={() => setMode('dark')}
            >
              {t('colorLab.mode.dark')}
            </button>
            <button
              type='button'
              className={styles.compareBtn}
              aria-pressed={compare}
              onClick={() => setCompare((current) => !current)}
            >
              {t('colorLab.compareLabel')}
            </button>
          </div>
        </header>

        <ul className={styles.schemes} aria-label={t('colorLab.schemeLabel')}>
          {schemeCards.map((card) => (
            <li key={card.id}>
              <button
                type='button'
                className={styles.scheme}
                aria-pressed={scheme === card.id}
                onClick={() => setScheme(card.id)}
              >
                <span className={styles.swatches} aria-hidden='true'>
                  <span className={styles.swatch} style={{ background: card.tokens.canvas }} />
                  <span className={styles.swatch} style={{ background: card.tokens.rail }} />
                  <span className={styles.swatch} style={{ background: card.tokens.accent }} />
                  <span className={styles.swatch} style={{ background: card.tokens.text }} />
                </span>
                <span className={styles.schemeName}>{t(`colorLab.schemes.${card.id}.name`)}</span>
                <span className={styles.pitch}>{t(`colorLab.schemes.${card.id}.pitch`)}</span>
                <span className={styles.fit}>{t(`colorLab.schemes.${card.id}.fit`)}</span>
              </button>
            </li>
          ))}
        </ul>

        <section className={styles.diagnosis}>
          <div>
            <h2 className={styles.diagnosisTitle}>{t('colorLab.diagnosisTitle')}</h2>
            <p className={styles.diagnosisBody}>{t('colorLab.diagnosisBody')}</p>
            <p className={styles.applyHint}>{t('colorLab.applyHint')}</p>
          </div>
          <div>
            <h2 className={styles.diagnosisTitle}>{t('colorLab.tokensLabel')}</h2>
            <div className={styles.tokens}>
              {TOKEN_KEYS.map((key) => (
                <div key={key} className={styles.token}>
                  <span className={styles.tokenChip} style={{ background: selectedTokens[key] }} />
                  <span className={styles.tokenLabel}>{t(`colorLab.token.${key}`)}</span>
                </div>
              ))}
            </div>
          </div>
        </section>

        <div className={compare ? `${styles.stage} ${styles.stageCompare}` : styles.stage}>
          {compare ? (
            <>
              <ColorLabShell tokens={nowTokens} caption={t('colorLab.compareNow')} />
              <ColorLabShell tokens={selectedTokens} caption={t('colorLab.compareCandidate')} />
            </>
          ) : (
            <ColorLabShell tokens={selectedTokens} />
          )}
        </div>
      </div>
    </div>
  );
};

export default ColorLabPage;
