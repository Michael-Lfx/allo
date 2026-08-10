/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Button, Form, Input, Switch } from '@arco-design/web-react';
import { HeadsetOne, LinkCloud } from '@icon-park/react';
import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import type { SpeechToTextConfig } from '@/common/types/provider/speech';
import TaskModelSelect from '@/renderer/components/model/TaskModelSelect';
import {
  DEFAULT_SPEECH_TO_TEXT_CONFIG,
  getSpeechToTextConfig,
  normalizeSpeechToTextConfig,
  saveSpeechToTextConfig,
  SPEECH_TO_TEXT_CONFIG_CHANGED_EVENT,
} from '@/renderer/services/speechToTextConfig';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';

/**
 * ASR 区：全局默认的「语音识别模型」。
 *
 * Candidates come from the authoritative catalog resolution for
 * `speech_recognition` — `TaskModelSelect` owns that query, so this panel does
 * not build its own option list and a saved model that has since disappeared
 * stays visible as an explicit "(unavailable)" option instead of blanking.
 */
const SpeechToTextContent: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [message, messageContext] = useArcoMessage({ maxCount: 2 });
  const [config, setConfig] = useState<SpeechToTextConfig>(DEFAULT_SPEECH_TO_TEXT_CONFIG);

  useEffect(() => {
    const syncConfig = () => setConfig(getSpeechToTextConfig());
    syncConfig();
    window.addEventListener(SPEECH_TO_TEXT_CONFIG_CHANGED_EVENT, syncConfig);
    return () => window.removeEventListener(SPEECH_TO_TEXT_CONFIG_CHANGED_EVENT, syncConfig);
  }, []);

  const persist = useCallback(
    (next: SpeechToTextConfig) => {
      const normalized = normalizeSpeechToTextConfig(next);
      setConfig(normalized);
      void saveSpeechToTextConfig(normalized).catch((error) => {
        console.error('Failed to save speech-to-text config:', error);
        setConfig(getSpeechToTextConfig());
        message.error(error instanceof Error ? error.message : t('settings.saveModelConfigFailed'));
      });
    },
    [message, t]
  );

  const hasSource = Boolean(config.provider_id && config.model);

  return (
    <div className='flex min-h-0 flex-col rd-16px bg-2 px-24px py-16px'>
      {messageContext}
      <header className='flex items-center gap-9px border-b border-[var(--color-border-2)] pb-14px'>
        <span className='size-30px shrink-0 flex items-center justify-center rd-9px bg-primary-1 text-primary-6'>
          <HeadsetOne theme='outline' size='18' strokeWidth={3} />
        </span>
        <div className='min-w-0'>
          <h2 className='m-0 text-20px font-650 leading-28px text-t-primary'>
            {t('settings.modelHub.speech.title')}
          </h2>
          <p className='m-0 mt-2px text-12px leading-18px text-t-secondary'>
            {t('settings.modelHub.speech.subtitle')}
          </p>
        </div>
      </header>

      <Form layout='vertical' className='mt-18px'>
        <Form.Item label={t('settings.modelHub.speech.source')}>
          <TaskModelSelect
            task='speech_recognition'
            size='default'
            value={
              config.provider_id && config.model
                ? { provider_id: config.provider_id, model: config.model }
                : null
            }
            emptyHint={t('settings.modelHub.speech.noSources')}
            onChange={({ provider_id, model }) =>
              persist({
                ...config,
                enabled: true,
                // The stored `provider` enum is legacy: transcription executes by
                // provider_id + model and the backend ignores this field. Keep the
                // 'openai' constant so persisted configs stay shape-compatible.
                provider: 'openai',
                provider_id,
                model,
              })
            }
          />
        </Form.Item>
        <Form.Item label={t('settings.modelHub.speech.defaultLanguage')}>
          <Input
            value={config.language}
            placeholder={t('settings.modelHub.speech.languagePlaceholder')}
            onBlur={() => persist(config)}
            onChange={(language) => setConfig((current) => ({ ...current, language }))}
          />
        </Form.Item>
        <Form.Item label={t('settings.modelHub.speech.enabled')}>
          <Switch
            checked={config.enabled && hasSource}
            disabled={!hasSource}
            onChange={(enabled) => persist({ ...config, enabled })}
          />
        </Form.Item>
      </Form>

      <div className='mt-6px flex items-center gap-8px flex-wrap'>
        <Button
          type='text'
          size='small'
          icon={<LinkCloud theme='outline' size='14' />}
          onClick={() => navigate('/models?section=models')}
        >
          {t('settings.modelHub.speech.manageProviders')}
        </Button>
      </div>
    </div>
  );
};

export default SpeechToTextContent;
