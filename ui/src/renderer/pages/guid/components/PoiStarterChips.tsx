

import { ipcBridge } from '@/common';
import type { IPoiStarter } from '@/common/adapter/ipcBridge';
import { resolveLocaleKey } from '@/common/utils';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';

const DEFAULT_PAGE_SIZE = 4;

export interface PoiStarterChipsProps {
  onSetInput: (text: string) => void;
  onFocusInput: () => void;
}

/**
 * Interest-based conversation starters above the Guid composer.
 * Local POI starters when available; otherwise remote preset (backend placeholder).
 */
const PoiStarterChips: React.FC<PoiStarterChipsProps> = ({ onSetInput, onFocusInput }) => {
  const { t, i18n } = useTranslation();
  const localeKey = resolveLocaleKey(i18n.language);
  const [starters, setStarters] = useState<IPoiStarter[]>([]);
  const [total, setTotal] = useState(0);
  const [source, setSource] = useState<'local' | 'preset' | string>('local');
  const [seed, setSeed] = useState(() => Math.floor(Math.random() * 1_000_000_000));
  const [loading, setLoading] = useState(true);

  const load = useCallback(
    async (nextSeed: number) => {
      setLoading(true);
      try {
        const res = await ipcBridge.poi.listStarters.invoke({
          limit: DEFAULT_PAGE_SIZE,
          offset: 0,
          seed: nextSeed,
          locale: localeKey,
        });
        setStarters(res.starters ?? []);
        setTotal(res.total ?? 0);
        setSource(res.source ?? 'local');
      } catch {
        setStarters([]);
        setTotal(0);
      } finally {
        setLoading(false);
      }
    },
    [localeKey]
  );

  useEffect(() => {
    void load(seed);
  }, [load, seed]);

  const canRefresh = useMemo(() => total > DEFAULT_PAGE_SIZE, [total]);

  const handleRefresh = useCallback(() => {
    setSeed((prev) => prev + 1 + Math.floor(Math.random() * 97));
  }, []);

  if (loading && starters.length === 0) {
    return null;
  }
  if (starters.length === 0) {
    return null;
  }

  const hintKey =
    source === 'preset' ? 'guid.poiStartersPresetHint' : 'guid.poiStartersHint';

  return (
    <div className={styles.poiStarterSection} data-testid='poi-starter-chips'>
      <div className={styles.poiStarterHeader}>
        <span className={styles.assistantPromptHint}>
          {t(hintKey, {
            defaultValue:
              source === 'preset' ? 'Try a starter topic:' : 'Suggested from your interests:',
          })}
        </span>
        {canRefresh ? (
          <button
            type='button'
            className={styles.poiStarterRefresh}
            onClick={handleRefresh}
            data-testid='poi-starter-refresh'
          >
            {t('guid.poiStartersRefresh', { defaultValue: '换一批' })}
          </button>
        ) : null}
      </div>
      <div className={styles.poiStarterChips}>
        {starters.map((starter) => (
          <div
            key={starter.id}
            className={`${styles.assistantPromptChip} ${styles.poiStarterChip} px-12px py-6px text-2 text-13px rd-16px cursor-pointer transition-colors shadow-sm`}
            onClick={() => {
              onSetInput(starter.text);
              onFocusInput();
            }}
            title={starter.topicLabel || undefined}
          >
            {starter.text}
          </div>
        ))}
      </div>
    </div>
  );
};

export default PoiStarterChips;
