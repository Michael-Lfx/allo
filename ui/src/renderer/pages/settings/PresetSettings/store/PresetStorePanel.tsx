import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Message } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import { getStoreData, getTemplatesByCategory } from './data';
import PresetStoreCard from './PresetStoreCard';
import PresetStoreDetail from './PresetStoreDetail';
import type { StorePresetTemplate } from './types';
import './PresetStorePanel.css';

const CARD_GRID_COLS = 'repeat(auto-fill, minmax(min(232px, 100%), 1fr))';

interface PresetStorePanelProps {
  onInstalled?: () => void;
}

const PresetStorePanel: React.FC<PresetStorePanelProps> = ({ onInstalled }) => {
  const { t } = useTranslation();
  const storeData = useMemo(() => getStoreData(), []);
  const [activeCategory, setActiveCategory] = useState('all');
  const [detailTemplate, setDetailTemplate] = useState<StorePresetTemplate | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);

  const filteredTemplates = useMemo(
    () => getTemplatesByCategory(activeCategory),
    [activeCategory]
  );

  const handleInstall = useCallback(async (template: StorePresetTemplate) => {
    setInstalling(template.id);
    try {
      await ipcBridge.presets.create.invoke({
        name: template.name,
        name_i18n: template.name_i18n,
        description: template.description,
        description_i18n: template.description_i18n,
        avatar: template.avatar,
        instructions: template.instructions,
        instructions_i18n: {},
        included_skills: template.included_skills.map((s) => ({ skill_name: s, required: false })),
        audience_tags: template.audience_tags,
        scenario_tags: template.scenario_tags,
        targets: ['conversation'],
      });
      Message.success(
        t('settings.presetStore.installSuccess', { name: template.name })
      );
      onInstalled?.();
    } catch (e) {
      Message.error(
        t('settings.presetStore.installFailed', { name: template.name })
      );
    } finally {
      setInstalling(null);
    }
  }, [onInstalled, t]);

  return (
    <div className='preset-store-panel'>
      {/* 分类导航 */}
      <div className='preset-store-panel__categories'>
        {storeData.categories.map((cat) => (
          <button
            key={cat.key}
            className={`preset-store-panel__category-btn ${activeCategory === cat.key ? 'active' : ''}`}
            onClick={() => setActiveCategory(cat.key)}
          >
            {t(`settings.presetStore.category.${cat.key}`)}
          </button>
        ))}
      </div>

      {/* 模板网格 */}
      {filteredTemplates.length > 0 ? (
        <div className='preset-store-panel__grid' style={{ gridTemplateColumns: CARD_GRID_COLS }}>
          {filteredTemplates.map((template) => (
            <PresetStoreCard
              key={template.id}
              template={template}
              installing={installing === template.id}
              onInstall={handleInstall}
              onDetail={setDetailTemplate}
            />
          ))}
        </div>
      ) : (
        <div className='preset-store-panel__empty'>
          {t('settings.presetStore.noTemplates')}
        </div>
      )}

      {/* 详情弹窗 */}
      <PresetStoreDetail
        template={detailTemplate}
        visible={detailTemplate !== null}
        onClose={() => setDetailTemplate(null)}
        onInstall={handleInstall}
      />
    </div>
  );
};

export default PresetStorePanel;
