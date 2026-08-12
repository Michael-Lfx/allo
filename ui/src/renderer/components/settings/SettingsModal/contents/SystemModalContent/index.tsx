import { ipcBridge } from '@/common';
import type { IStartOnBootStatus } from '@/common/adapter/ipcBridge';
import { configService } from '@/common/config/configService';
import NomiSelect from '@/renderer/components/base/NomiSelect';
import FeedbackButton from '@/renderer/components/base/FeedbackButton';
import LanguageSwitcher from '@/renderer/components/settings/LanguageSwitcher';
import {
  SettingsGroup,
  SettingsList,
  SettingsPageHeader,
  SettingsRow,
  SettingsStatus,
} from '@/renderer/components/settings/SettingsPagePrimitives';
import { iconColors } from '@/renderer/styles/colors';
import { isDesktopShell } from '@/renderer/utils/platform';
import { useKeepAwake } from '@renderer/hooks/ui/useKeepAwake';
import { Button, Form, Message, Modal, Switch, Tooltip } from '@arco-design/web-react';
import { FolderSearch } from '@icon-park/react';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import ImageAnalysisModelContent from '@/renderer/pages/modelHub/ImageAnalysisModelContent';
import useSWR from 'swr';
import DeveloperModeSetting from './DeveloperModeSetting';
import DirInputItem from './DirInputItem';
import FactoryResetModal from './FactoryResetModal';

/**
 * System settings remains the owner of all persistence and migration flows;
 * this component only gives those flows a stable page/section/row hierarchy.
 */
const SystemModalContent: React.FC = () => {
  const { t } = useTranslation();
  const [form] = Form.useForm();
  const [modalRaw, modalContextHolder] = Modal.useModal();
  const modal = modalRaw as Required<typeof modalRaw>;
  const [error, setError] = useState<string | null>(null);
  const [preferenceError, setPreferenceError] = useState<string | null>(null);
  const initializingRef = useRef(true);

  const [startOnBoot, setStartOnBoot] = useState<IStartOnBootStatus>({
    supported: false,
    enabled: false,
    isPackaged: false,
    platform: 'web',
  });
  const [notificationEnabled, setNotificationEnabled] = useState(true);
  const [cronNotificationEnabled, setCronNotificationEnabled] = useState(false);
  const [saveUploadToWorkspace, setSaveUploadToWorkspace] = useState(false);
  const [autoPreviewOfficeFiles, setAutoPreviewOfficeFiles] = useState(true);
  const [sendKey, setSendKey] = useState<'enter' | 'mod-enter'>('enter');
  const [factoryResetVisible, setFactoryResetVisible] = useState(false);
  const [isRelocating, setIsRelocating] = useState(false);

  const reportPreferenceError = useCallback((message = t('settings.preferenceSaveFailed')) => {
    setPreferenceError(message);
    Message.error(message);
  }, [t]);

  useEffect(() => {
    if (!isDesktopShell()) return;
    ipcBridge.application.getStartOnBootStatus
      .invoke()
      .then((result) => {
        if (result.success && result.data) setStartOnBoot(result.data);
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    setNotificationEnabled(configService.get('system.notificationEnabled') ?? true);
    setCronNotificationEnabled(configService.get('system.cronNotificationEnabled') ?? false);
    setSaveUploadToWorkspace(configService.get('upload.saveToWorkspace') ?? false);
    setAutoPreviewOfficeFiles(configService.get('system.autoPreviewOfficeFiles') ?? true);
    setSendKey(configService.get('chat.sendKey') ?? 'enter');
  }, []);

  const handleStartOnBootChange = useCallback(
    (checked: boolean) => {
      const previousStatus = startOnBoot;
      setStartOnBoot((previous) => ({ ...previous, enabled: checked }));
      ipcBridge.application.setStartOnBoot
        .invoke({ enabled: checked })
        .then((result) => {
          if (result.success && result.data) {
            setStartOnBoot(result.data);
            return;
          }
          setStartOnBoot(previousStatus);
          reportPreferenceError(result.msg || t('settings.startOnBootUpdateFailed'));
        })
        .catch(() => {
          setStartOnBoot(previousStatus);
          reportPreferenceError(t('settings.startOnBootUpdateFailed'));
        });
    },
    [reportPreferenceError, startOnBoot]
  );

  const persistBooleanPreference = useCallback(
    (
      key:
        | 'system.notificationEnabled'
        | 'system.cronNotificationEnabled'
        | 'upload.saveToWorkspace'
        | 'system.autoPreviewOfficeFiles',
      next: boolean,
      previous: boolean,
      update: (value: boolean) => void
    ) => {
      update(next);
      void configService.set(key, next).catch(() => {
        update(previous);
        configService.setLocal(key, previous);
        reportPreferenceError();
      });
    },
    [reportPreferenceError]
  );
  const persistSendKey = useCallback(
    (next: 'enter' | 'mod-enter', previous: 'enter' | 'mod-enter') => {
      setSendKey(next);
      void configService.set('chat.sendKey', next).catch(() => {
        setSendKey(previous);
        configService.setLocal('chat.sendKey', previous);
        reportPreferenceError();
      });
    },
    [reportPreferenceError]
  );

  const handleNotificationEnabledChange = useCallback(
    (checked: boolean) =>
      persistBooleanPreference('system.notificationEnabled', checked, notificationEnabled, setNotificationEnabled),
    [notificationEnabled, persistBooleanPreference]
  );
  const handleCronNotificationEnabledChange = useCallback(
    (checked: boolean) =>
      persistBooleanPreference('system.cronNotificationEnabled', checked, cronNotificationEnabled, setCronNotificationEnabled),
    [cronNotificationEnabled, persistBooleanPreference]
  );
  const handleSaveUploadToWorkspaceChange = useCallback(
    (checked: boolean) =>
      persistBooleanPreference('upload.saveToWorkspace', checked, saveUploadToWorkspace, setSaveUploadToWorkspace),
    [persistBooleanPreference, saveUploadToWorkspace]
  );
  const handleAutoPreviewOfficeFilesChange = useCallback(
    (checked: boolean) =>
      persistBooleanPreference('system.autoPreviewOfficeFiles', checked, autoPreviewOfficeFiles, setAutoPreviewOfficeFiles),
    [autoPreviewOfficeFiles, persistBooleanPreference]
  );
  const handleSendKeyChange = useCallback(
    (value: 'enter' | 'mod-enter') => persistSendKey(value, sendKey),
    [persistSendKey, sendKey]
  );

  const { keepAwake, setKeepAwake: applyKeepAwake } = useKeepAwake();
  const handleKeepAwakeChange = useCallback(
    async (checked: boolean) => {
      try {
        await applyKeepAwake(checked);
      } catch (error: unknown) {
        Message.error(String(error));
        reportPreferenceError();
      }
    },
    [applyKeepAwake, reportPreferenceError]
  );

  const { data: systemInfo } = useSWR('system.dir.info', () => ipcBridge.application.systemInfo.invoke());
  const { data: relocation, mutate: refreshRelocation } = useSWR(
    'system.work-dir.relocation',
    () => ipcBridge.application.workDirRelocation.get.invoke()
  );
  const canChangeWorkDirectory = systemInfo?.runtimeCapabilities.canChangeWorkDirectory === true;

  const handleRelocationRetry = useCallback(async () => {
    const operationId = relocation?.operation?.operationId;
    if (!operationId) return;
    setError(null);
    setIsRelocating(true);
    try {
      const result = await ipcBridge.application.workDirRelocation.retry.invoke({ operationId });
      if (result.restart_required) {
        await ipcBridge.application.restart.invoke();
      } else {
        setIsRelocating(false);
        await refreshRelocation();
      }
    } catch (caughtError: unknown) {
      setIsRelocating(false);
      setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
    }
  }, [refreshRelocation, relocation?.operation?.operationId]);

  const handleRelocationCancel = useCallback(async () => {
    const operationId = relocation?.operation?.operationId;
    if (!operationId) return;
    try {
      await ipcBridge.application.workDirRelocation.cancel.invoke({ operationId });
      await refreshRelocation();
    } catch (caughtError: unknown) {
      setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
    }
  }, [refreshRelocation, relocation?.operation?.operationId]);

  const handleOpenLogDir = useCallback(() => {
    if (!systemInfo?.logDir) return;
    void ipcBridge.shell.openFolderWith
      .invoke({ folder_path: systemInfo.logDir, tool: 'explorer' })
      .catch((caughtError) => {
        console.error('[SystemModalContent] Failed to open log directory:', caughtError);
        setError(caughtError instanceof Error ? caughtError.message : String(caughtError));
      });
  }, [systemInfo?.logDir]);

  useEffect(() => {
    if (!systemInfo) return;
    initializingRef.current = true;
    form.setFieldsValue({ workDir: systemInfo.workDir });
    try {
      if (window.sessionStorage.getItem('nomifun.relocationTarget') === systemInfo.workDir) {
        window.sessionStorage.setItem('nomifun.relocationCompleted', '1');
      }
    } catch {
      // Diagnostics are best effort and must not affect settings bootstrap.
    }
    requestAnimationFrame(() => {
      initializingRef.current = false;
    });
  }, [systemInfo, form]);

  const saveDirConfigValidate = (_values: { workDir: string }): Promise<unknown> =>
    new Promise((resolve, reject) => {
      modal.confirm({
        className: 'work-dir-relocation-confirm',
        title: t('settings.workDirChangeConfirmTitle'),
        content: t('settings.workDirChangeConfirmContent'),
        onOk: resolve,
        onCancel: reject,
      });
    });

  const savingRef = useRef(false);
  const handleValuesChange = useCallback(
    async (_changedValue: unknown, allValues: Record<string, string>) => {
      if (initializingRef.current || savingRef.current || !systemInfo) return;
      const { workDir } = allValues;
      if (!systemInfo.runtimeCapabilities.canChangeWorkDirectory || workDir === systemInfo.workDir) return;

      savingRef.current = true;
      setError(null);
      try {
        try {
          await saveDirConfigValidate({ workDir });
          setIsRelocating(true);
          const response = await ipcBridge.application.updateSystemInfo.invoke({
            cacheDir: systemInfo.cacheDir,
            workDir,
          });
          try {
            if (response.operation_id) {
              window.sessionStorage.setItem('nomifun.lastRelocationOperationId', response.operation_id);
              window.sessionStorage.setItem('nomifun.relocationTarget', workDir);
              window.sessionStorage.setItem('nomifun.relocationCompleted', '0');
            }
          } catch {
            // Diagnostics are best effort and must not affect the restart path.
          }
        } catch (persistError: unknown) {
          setIsRelocating(false);
          form.setFieldValue('workDir', systemInfo.workDir);
          if (persistError) setError(persistError instanceof Error ? persistError.message : String(persistError));
          return;
        }
        try {
          await ipcBridge.application.restart.invoke();
        } catch (restartError: unknown) {
          setIsRelocating(false);
          if (restartError) setError(restartError instanceof Error ? restartError.message : String(restartError));
        }
      } finally {
        savingRef.current = false;
      }
    },
    [form, saveDirConfigValidate, systemInfo]
  );

  const relocationStatus = relocation?.operation && ['failed', 'paused'].includes(relocation.operation.state) ? (
    <div className='space-y-6px'>
      <SettingsStatus tone='error'>{relocation.operation.error || t('settings.workDirRelocationFailed')}</SettingsStatus>
      <div className='flex gap-8px'>
        <Button size='small' type='primary' onClick={() => void handleRelocationRetry()}>{t('common.retry')}</Button>
        <Button size='small' onClick={() => void handleRelocationCancel()}>{t('common.cancel')}</Button>
      </div>
    </div>
  ) : null;

  return (
    <div className='w-full space-y-24px pb-16px'>
      {modalContextHolder}
      {isRelocating && (
        <div className='fixed inset-0 z-[10000] flex items-center justify-center bg-black/45 backdrop-blur-2px'>
          <div className='min-w-280px max-w-[calc(100vw-48px)] rounded-12px bg-2 px-24px py-20px text-center shadow-xl'>
            <div className='text-16px font-600 text-t-primary'>{t('settings.workDirRelocating')}</div>
            <div className='mt-8px text-13px text-t-secondary'>{t('settings.workDirRelocatingDesc')}</div>
          </div>
        </div>
      )}

      <SettingsPageHeader title={t('settings.system')} />

      <SettingsGroup title={t('settings.sectionGeneral')}>
        <SettingsList>
          <SettingsRow label={t('settings.language')} control={<LanguageSwitcher />} controlLayout='field' />
          <SettingsRow
            label={t('settings.sendKey')}
            description={t('settings.sendKeyDesc')}
            control={
              <NomiSelect className='w-full' value={sendKey} onChange={(value) => handleSendKeyChange(value as 'enter' | 'mod-enter')}>
                <NomiSelect.Option value='enter'>{t('settings.sendKeyEnter')}</NomiSelect.Option>
                <NomiSelect.Option value='mod-enter'>{t('settings.sendKeyModEnter')}</NomiSelect.Option>
              </NomiSelect>
            }
            controlLayout='field'
          />
          <SettingsRow
            label={t('settings.modelHub.imageAnalysis.title')}
            description={t('settings.modelHub.imageAnalysis.subtitle')}
            control={<ImageAnalysisModelContent compact />}
            controlLayout='field'
          />
          <SettingsRow
            label={t('settings.startOnBoot')}
            description={startOnBoot.supported ? t('settings.startOnBootDesc') : t('settings.startOnBootUnsupported')}
            disabled={!startOnBoot.supported}
            control={<Switch checked={startOnBoot.enabled} onChange={handleStartOnBootChange} disabled={!startOnBoot.supported} />}
            controlLayout='compact'
          />
          <SettingsRow label={t('settings.keepAwake')} description={t('settings.keepAwakeDesc')} control={<Switch checked={keepAwake} onChange={handleKeepAwakeChange} />} controlLayout='compact' />
          <SettingsRow label={t('settings.saveUploadToWorkspace')} control={<Switch checked={saveUploadToWorkspace} onChange={handleSaveUploadToWorkspaceChange} />} controlLayout='compact' />
          <SettingsRow label={t('settings.autoPreviewOfficeFiles')} description={t('settings.autoPreviewOfficeFilesDesc')} control={<Switch checked={autoPreviewOfficeFiles} onChange={handleAutoPreviewOfficeFilesChange} />} controlLayout='compact' />
        </SettingsList>
        {preferenceError && <SettingsStatus tone='error' className='mt-8px'>{preferenceError}</SettingsStatus>}
      </SettingsGroup>

      <SettingsGroup title={t('settings.sectionNotifications')}>
        <SettingsList>
          <SettingsRow label={t('settings.notification')} control={<Switch checked={notificationEnabled} onChange={handleNotificationEnabledChange} />} controlLayout='compact' />
          <SettingsRow
            label={t('settings.cronNotificationEnabled')}
            disabled={!notificationEnabled}
            status={!notificationEnabled ? <SettingsStatus>{t('settings.disabledByNotifications')}</SettingsStatus> : undefined}
            control={<Switch checked={cronNotificationEnabled} disabled={!notificationEnabled} onChange={handleCronNotificationEnabledChange} />}
            controlLayout='compact'
          />
        </SettingsList>
      </SettingsGroup>

      <SettingsGroup title={t('settings.sectionStorage')}>
        <Form form={form} layout='vertical' onValuesChange={handleValuesChange}>
          <SettingsList>
            <SettingsRow
              label={t('settings.workDir')}
              description={!canChangeWorkDirectory ? t('settings.workDirDesktopOnly') : undefined}
              disabled={!canChangeWorkDirectory}
              status={relocationStatus}
              control={<DirInputItem compact label={t('settings.workDir')} field='workDir' disabled={!canChangeWorkDirectory} />}
              controlLayout='field'
            />
            <SettingsRow
              label={t('settings.logDir')}
              control={
                <div className='nomi-dir-input flex h-[32px] items-center rounded-8px border border-solid border-transparent bg-[var(--fill-0)] pl-14px'>
                  <Tooltip content={systemInfo?.logDir || ''} position='top'>
                    <span className='min-w-0 flex-1 truncate text-13px text-t-primary'>{systemInfo?.logDir || ''}</span>
                  </Tooltip>
                  <Button
                    type='text'
                    aria-label={t('settings.logDir')}
                    style={{ borderLeft: '1px solid var(--color-border-2)', borderRadius: '0 8px 8px 0' }}
                    icon={<FolderSearch theme='outline' size='18' fill={iconColors.primary} />}
                    onClick={handleOpenLogDir}
                  />
                </div>
              }
              controlLayout='field'
            />
          </SettingsList>
        </Form>
        {systemInfo?.workDirChange?.state === 'failed' && (
          <SettingsStatus tone='error' className='mt-8px'>
            {systemInfo.workDirChange.error || t('settings.workDirRelocationFailed')}
            {systemInfo.workDirChange.rollbackCopy && ` · ${t('settings.workDirRelocationBackup')}: ${systemInfo.workDirChange.rollbackCopy}`}
          </SettingsStatus>
        )}
        {error && <SettingsStatus tone='error' className='mt-8px'>{error} <FeedbackButton className='ml-6px' /></SettingsStatus>}
      </SettingsGroup>

      <SettingsGroup title={t('settings.sectionDeveloper')}>
        <SettingsList><DeveloperModeSetting /></SettingsList>
      </SettingsGroup>

      <SettingsGroup title={t('settings.sectionDanger')}>
        <SettingsList>
          <SettingsRow
            label={t('settings.factoryReset.title')}
            description={t('settings.factoryReset.rowDesc')}
            control={<Button status='danger' onClick={() => setFactoryResetVisible(true)}>{t('settings.factoryReset.button')}</Button>}
            controlLayout='actions'
          />
        </SettingsList>
      </SettingsGroup>

      <FactoryResetModal visible={factoryResetVisible} onClose={() => setFactoryResetVisible(false)} />
    </div>
  );
};

export default SystemModalContent;
