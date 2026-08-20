import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';
import { Close } from '@icon-park/react';
import { configService } from '@/common/config/configService';
import { NOMIFUN_FREE_MODEL_PLATFORM } from '@/common/types/provider/managedModelService';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import TaskModelSelect, { type TaskModelSelection } from '@/renderer/components/agent/TaskModelSelect';
import { useModelsForTask, type TaskModelGroup } from '@/renderer/hooks/agent/useModelsForTask';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';

const STORAGE_KEY = 'tools.imageAnalysisModel';
const IMAGE_ANALYSIS_EXCLUDED_PLATFORMS = [NOMIFUN_FREE_MODEL_PLATFORM];

/** Flowy Cloud catalog ids are `AIPC-<name>`; match the displayed MiniMax-M3 name. */
export const isPreferredImageAnalysisModel = (model: string): boolean =>
  model.replace(/^AIPC-/i, '').toLowerCase() === 'minimax-m3';

export const isImageAnalysisEligibleGroup = (group: TaskModelGroup): boolean =>
  group.provider.platform !== NOMIFUN_FREE_MODEL_PLATFORM;

export const resolveAutomaticImageAnalysisModel = (
  groups: TaskModelGroup[]
): TaskModelSelection | null => {
  const eligible = groups.filter(isImageAnalysisEligibleGroup);
  const preferredGroup = eligible.find((group) => group.models.some(isPreferredImageAnalysisModel));
  if (preferredGroup) {
    const preferredModel = preferredGroup.models.find(isPreferredImageAnalysisModel);
    if (preferredModel) return { providerId: preferredGroup.provider.id, model: preferredModel };
  }

  const firstGroup = eligible[0];
  const firstModel = firstGroup?.models[0];
  return firstGroup && firstModel ? { providerId: firstGroup.provider.id, model: firstModel } : null;
};

/** Global, independent vision-model preference for text-only Nomi sessions. */
const ImageAnalysisModelContent: React.FC<{ compact?: boolean; autoRefreshCatalog?: boolean }> = ({
  compact = false,
  autoRefreshCatalog,
}) => {
  const { t } = useTranslation();
  const [message, messageContext] = useArcoMessage();
  const [stored] = useConfig(STORAGE_KEY);
  const { groups, isLoading } = useModelsForTask('chat', ['vision_input'], { autoRefreshCatalog });
  const storedSelection = useMemo<TaskModelSelection | null>(() => {
    if (!stored?.provider_id || !stored.model) return null;
    return { providerId: stored.provider_id, model: stored.model };
  }, [stored?.model, stored?.provider_id]);
  const automaticSelection = useMemo(
    () => (isLoading ? null : resolveAutomaticImageAnalysisModel(groups)),
    [groups, isLoading]
  );
  const selection = storedSelection ?? automaticSelection;

  const select = async (next: TaskModelSelection) => {
    try {
      await configService.set(STORAGE_KEY, { provider_id: next.providerId, model: next.model });
    } catch (error) {
      message.error(String(error));
    }
  };

  const clear = async () => {
    try {
      await configService.remove(STORAGE_KEY);
    } catch (error) {
      message.error(String(error));
    }
  };

  return (
    <div className={compact ? 'flex w-full items-center justify-end gap-8px' : 'flex max-w-640px flex-col gap-14px'}>
      {messageContext}
      {!compact && (
        <div>
          <div className='text-15px font-600 text-t-primary'>{t('settings.modelHub.imageAnalysis.title')}</div>
          <div className='mt-4px text-12px leading-18px text-t-tertiary'>
            {t('settings.modelHub.imageAnalysis.subtitle')}
          </div>
        </div>
      )}
      <div className='flex items-center gap-8px'>
        <TaskModelSelect
          task='chat'
          requiredTraits={['vision_input']}
          excludePlatforms={IMAGE_ANALYSIS_EXCLUDED_PLATFORMS}
          value={selection}
          onSelect={(next) => void select(next)}
          placeholder={t('settings.modelHub.imageAnalysis.autoDefault')}
          autoRefreshCatalog={autoRefreshCatalog}
        />
        {storedSelection && (
          <Button
            type='text'
            icon={<Close theme='outline' size='15' />}
            aria-label={t('settings.modelHub.imageAnalysis.useAutomatic')}
            title={t('settings.modelHub.imageAnalysis.useAutomatic')}
            onClick={() => void clear()}
          />
        )}
      </div>
    </div>
  );
};

export default ImageAnalysisModelContent;
