import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';
import { Close } from '@icon-park/react';
import { configService } from '@/common/config/configService';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import TaskModelSelect, { type TaskModelSelection } from '@/renderer/components/agent/TaskModelSelect';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';

const STORAGE_KEY = 'tools.imageAnalysisModel';

/** Global, independent vision-model preference for text-only Nomi sessions. */
const ImageAnalysisModelContent: React.FC = () => {
  const { t } = useTranslation();
  const [message, messageContext] = useArcoMessage();
  const [stored] = useConfig(STORAGE_KEY);
  const selection = useMemo<TaskModelSelection | null>(() => {
    if (!stored?.provider_id || !stored.model) return null;
    return { providerId: stored.provider_id, model: stored.model };
  }, [stored?.model, stored?.provider_id]);

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
    <div className='flex max-w-640px flex-col gap-14px'>
      {messageContext}
      <div>
        <div className='text-15px font-600 text-t-primary'>{t('settings.modelHub.imageAnalysis.title')}</div>
        <div className='mt-4px text-12px leading-18px text-t-tertiary'>
          {t('settings.modelHub.imageAnalysis.subtitle')}
        </div>
      </div>
      <div className='flex items-center gap-8px'>
        <TaskModelSelect
          task='chat'
          requiredTraits={['vision_input']}
          value={selection}
          onSelect={(next) => void select(next)}
          placeholder={t('settings.modelHub.imageAnalysis.autoDefault')}
        />
        {selection && (
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
