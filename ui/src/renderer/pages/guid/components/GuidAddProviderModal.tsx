

import { ipcBridge } from '@/common';
import type { IProvider, TProviderWithModel } from '@/common/config/storage';
import AddPlatformModal from '@renderer/pages/settings/components/AddPlatformModal';
import { useProvidersQuery } from '@/renderer/hooks/agent/useModelProviderList';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import React, { useCallback, useImperativeHandle } from 'react';
import { useTranslation } from 'react-i18next';

export type GuidAddProviderHandle = {
  open: () => void;
};

type GuidAddProviderModalProps = {
  onConfigured?: (model: TProviderWithModel) => void | Promise<void>;
};

/**
 * Mounts AddPlatformModal on Guid so users can add a provider key in place
 * without leaving the first-task surface for /models.
 */
const GuidAddProviderModal = React.forwardRef<GuidAddProviderHandle, GuidAddProviderModalProps>(
  ({ onConfigured }, ref) => {
    const { t } = useTranslation();
    const [message, messageHolder] = useArcoMessage();
    const { data: providers, mutate } = useProvidersQuery();

    const persistPlatform = useCallback(
      async (platform: IProvider) => {
        const existing = (providers || []).some((item) => item.id === platform.id);
        if (existing) {
          const { id, ...body } = platform;
          await ipcBridge.mode.updateProvider.invoke({ id, ...body });
        } else {
          await ipcBridge.mode.createProvider.invoke(platform);
        }
      },
      [providers]
    );

    const [addPlatformModalCtrl, addPlatformModalContext] = AddPlatformModal.useModal({
      async onSubmit(platform) {
        const nextArray = (providers || []).some((item) => item.id === platform.id)
          ? (providers || []).map((item) => (item.id === platform.id ? { ...item, ...platform } : item))
          : [...(providers || []), platform];
        void mutate(nextArray, false);
        try {
          await persistPlatform(platform);
          await mutate();
          const firstModel = platform.models?.[0];
          if (firstModel) {
            await onConfigured?.({ ...platform, use_model: firstModel });
          }
          message.success(t('guid.modelProviderSaved', { defaultValue: '模型已就绪，可以开始任务' }));
        } catch (error) {
          void mutate();
          console.error('[GuidAddProviderModal] Failed to save provider:', error);
          const msg = error instanceof Error ? error.message : String(error);
          if (msg.includes('409')) {
            message.error(
              t('settings.providerIdConflict', { defaultValue: 'Provider id already exists, retry.' })
            );
          } else {
            message.error(t('settings.saveModelConfigFailed'));
          }
          throw error;
        }
      },
    });

    useImperativeHandle(
      ref,
      () => ({
        open: () => addPlatformModalCtrl.open(),
      }),
      [addPlatformModalCtrl]
    );

    return (
      <>
        {messageHolder}
        {addPlatformModalContext}
      </>
    );
  }
);

GuidAddProviderModal.displayName = 'GuidAddProviderModal';

export default GuidAddProviderModal;
