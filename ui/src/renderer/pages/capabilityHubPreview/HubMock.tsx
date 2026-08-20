import { ApplicationOne, Plus, Puzzle, Robot, Search, Tool } from '@icon-park/react';
import type { FC } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './capabilityHubPreview.module.css';
import {
  CARD_IDS,
  HUB_IDS,
  INSTALLED_CARD_IDS,
  type HubId,
  type HubPreviewVariant,
  type HubView,
} from './fixtures';

const TAB_ICONS = {
  presets: Robot,
  skills: Puzzle,
  mcp: Tool,
  plugins: ApplicationOne,
} as const;

type HubMockProps = {
  variant: HubPreviewVariant;
  hub: HubId;
  view: HubView;
  onHubChange: (hub: HubId) => void;
  onViewChange: (view: HubView) => void;
};

const HubMock: FC<HubMockProps> = ({ variant, hub, view, onHubChange, onViewChange }) => {
  const { t } = useTranslation();
  const cards = view === 'installed' ? INSTALLED_CARD_IDS : CARD_IDS;
  const hubLabel = t(`capabilityHubPreview.tabs.${hub}`);
  const isProposed = variant === 'proposed';

  return (
    <div className={styles.hub} data-variant={variant} data-testid='capability-hub-preview-shell'>
      {isProposed ? (
        <div className={styles.headerProposed}>
          <div className={styles.tabs} role='tablist' aria-label={t('capabilityHubPreview.title')}>
            {HUB_IDS.map((id) => {
              const Icon = TAB_ICONS[id];
              return (
                <button
                  key={id}
                  type='button'
                  role='tab'
                  aria-selected={id === hub}
                  className={styles.tab}
                  onClick={() => onHubChange(id)}
                >
                  <Icon theme='outline' size={16} fill='currentColor' />
                  {t(`capabilityHubPreview.tabs.${id}`)}
                </button>
              );
            })}
          </div>
          <div className={styles.toolbarProposed}>
            <div className={styles.searchProposed}>
              <Search theme='outline' size={16} fill='currentColor' />
              {t('capabilityHubPreview.search', { hub: hubLabel })}
            </div>
            <div className={styles.segment} role='group'>
              <button
                type='button'
                className={styles.segmentBtn}
                aria-pressed={view === 'market'}
                onClick={() => onViewChange('market')}
              >
                {t('capabilityHubPreview.discover')}
              </button>
              <button
                type='button'
                className={styles.segmentBtn}
                aria-pressed={view === 'installed'}
                onClick={() => onViewChange('installed')}
              >
                {t('capabilityHubPreview.installed')}
                <span className={styles.count}>{INSTALLED_CARD_IDS.length}</span>
              </button>
            </div>
            <button type='button' className={styles.primaryBtn}>
              <Plus theme='outline' size={14} fill='currentColor' /> {t('capabilityHubPreview.add')}
            </button>
          </div>
        </div>
      ) : (
        <div className={styles.headerNow}>
          <div className={styles.tabs} role='tablist' aria-label={t('capabilityHubPreview.title')}>
            {HUB_IDS.map((id) => {
              const Icon = TAB_ICONS[id];
              return (
                <button
                  key={id}
                  type='button'
                  role='tab'
                  aria-selected={id === hub}
                  className={styles.tab}
                  onClick={() => onHubChange(id)}
                >
                  <Icon theme='outline' size={16} fill='currentColor' />
                  {t(`capabilityHubPreview.tabs.${id}`)}
                </button>
              );
            })}
          </div>
          <div className={styles.searchNow}>
            <Search theme='outline' size={14} fill='currentColor' />
            {t('capabilityHubPreview.search', { hub: hubLabel })}
          </div>
          <div className={styles.actionsNow}>
            <button
              type='button'
              className={styles.ghostBtn}
              aria-pressed={view === 'installed'}
              onClick={() => onViewChange(view === 'installed' ? 'market' : 'installed')}
            >
              {t('capabilityHubPreview.myInstalled')}
              <span className={styles.count}>{INSTALLED_CARD_IDS.length}</span>
            </button>
            <button type='button' className={styles.primaryBtn}>
              {t('capabilityHubPreview.add')}
            </button>
          </div>
        </div>
      )}

      <div className={styles.hubBody}>
        <div className={styles.grid}>
          {cards.map((id) => (
            <article key={`${hub}-${id}`} className={styles.card}>
              <div className={styles.cardHead}>
                <span className={styles.icon} aria-hidden='true'>
                  {id}
                </span>
                <h3 className={styles.cardTitle}>{t(`capabilityHubPreview.cards.${id}.title`)}</h3>
              </div>
              <p className={styles.cardBody}>{t(`capabilityHubPreview.cards.${id}.body`)}</p>
              <div className={styles.cardFoot}>
                <span className={styles.tag}>{t(`capabilityHubPreview.cards.${id}.tag`)}</span>
                <button type='button' className={styles.addBtn}>
                  {view === 'installed' ? t('capabilityHubPreview.added') : t('capabilityHubPreview.add')}
                </button>
              </div>
            </article>
          ))}
        </div>
      </div>
    </div>
  );
};

export default HubMock;
