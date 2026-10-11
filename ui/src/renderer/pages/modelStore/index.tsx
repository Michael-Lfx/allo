/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Empty, Input, Message } from '@arco-design/web-react';
import { Search } from '@icon-park/react';
import { ipcBridge } from '@/common';
import type { LocalModelLaunchOptions } from '@common/types/localModel';
import SegmentedTabs from '@/renderer/components/base/SegmentedTabs';
import { matchesSearch } from './format';
import { defaultLaunchOptions } from './defaults';
import { refreshProviderCaches, useLocalModels } from './useLocalModels';
import ModelStoreCard from './ModelStoreCard';
import LocalModelDetailModal from './LocalModelDetailModal';
import LocalModelLaunchModal from './LocalModelLaunchModal';
import LocalModelStopModal from './StopModelModal';
import LocalModelUninstallModal from './UninstallModelModal';
import './modelStore.css';
import type { ModelStoreModel, ModelStoreStatus } from './types';

type ModelStorePartition = 'builtin' | 'houmo';

/** 32px field matching the toolbar height. */
const SEARCH_FIELD_CLASS = [
  'model-store-search w-280px max-w-full',
  '[&_.arco-input-inner-wrapper]:!h-32px',
  '[&_.arco-input-inner-wrapper]:!rounded-8px',
  '[&_.arco-input-inner-wrapper]:!border',
  '[&_.arco-input-inner-wrapper]:!border-solid',
  '[&_.arco-input-inner-wrapper]:!border-[var(--color-border-2)]',
  '[&_.arco-input-inner-wrapper]:!bg-[var(--color-fill-2)]',
  '[&_.arco-input-inner-wrapper]:!shadow-none',
  '[&_.arco-input-inner-wrapper:hover]:!border-[var(--color-border-3)]',
  '[&_.arco-input-inner-wrapper-focus]:!border-primary-6',
  '[&_.arco-input-inner-wrapper-focus]:!shadow-none',
  '[&_.arco-input]:!text-13px',
].join(' ');

type DialogKind = 'detail' | 'launch' | 'stop' | 'uninstall' | null;

/**
 * Model store. The builtin partition is the three locally-runnable GGUF
 * models. The Houmo partition lists the remote Houmo catalog (the same list
 * `model-cli query` reads) and downloads those files from the cloud URL.
 */
const ModelStorePage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const { models, houmoError, isLoading, refresh } = useLocalModels();
  const [partition, setPartition] = useState<ModelStorePartition>('builtin');
  const [status, setStatus] = useState<ModelStoreStatus>('all');
  const [query, setQuery] = useState('');
  const [dialog, setDialog] = useState<DialogKind>(null);
  const [selected, setSelected] = useState<ModelStoreModel | undefined>(undefined);
  const [busyId, setBusyId] = useState<string | null>(null);

  const inPartition = useMemo(
    () => models.filter((model) => model.source === partition),
    [models, partition]
  );

  const searched = useMemo(
    () => inPartition.filter((model) => matchesSearch(model, query, i18n.language)),
    [i18n.language, inPartition, query]
  );

  const installedCount = inPartition.filter(
    (model) => model.status === 'installed' || model.status === 'starting' || model.status === 'running'
  ).length;
  const runningCount = inPartition.filter(
    (model) => model.status === 'running' || model.status === 'starting'
  ).length;

  const visible = useMemo(() => {
    if (status === 'installed') {
      return searched.filter(
        (model) => model.status === 'installed' || model.status === 'starting' || model.status === 'running'
      );
    }
    if (status === 'running') {
      return searched.filter((model) => model.status === 'running' || model.status === 'starting');
    }
    return searched;
  }, [searched, status]);

  const partitionItems = [
    { key: 'builtin', label: t('modelStore.partition.local') },
    { key: 'houmo', label: t('modelStore.partition.houmo') },
  ];

  const statusItems = [
    { key: 'all', label: `${t('modelStore.status.all')} (${searched.length})` },
    { key: 'installed', label: `${t('modelStore.status.installed')} (${installedCount})` },
    { key: 'running', label: `${t('modelStore.status.running')} (${runningCount})` },
  ];

  const openDialog = (kind: DialogKind, model: ModelStoreModel) => {
    setSelected(model);
    setDialog(kind);
  };

  const closeDialog = () => setDialog(null);

  const handleError = (error: unknown, fallbackKey: string) => {
    const message = error instanceof Error && error.message ? error.message : t(fallbackKey);
    Message.error(message);
  };

  const handleDownload = async (model: ModelStoreModel) => {
    setBusyId(model.id);
    try {
      await ipcBridge.localModel.install.invoke({ id: model.id });
      await refresh();
    } catch (error) {
      handleError(error, 'modelStore.error.install');
    } finally {
      setBusyId(null);
    }
  };

  const handleCancel = async (model: ModelStoreModel) => {
    try {
      await ipcBridge.localModel.cancel.invoke({ id: model.id });
      await refresh();
    } catch (error) {
      handleError(error, 'modelStore.error.cancel');
    }
  };

  const handleStart = async (model: ModelStoreModel, options?: LocalModelLaunchOptions) => {
    setBusyId(model.id);
    try {
      await ipcBridge.localModel.start.invoke({
        id: model.id,
        options: options ?? model.lastLaunchOptions ?? defaultLaunchOptions(model),
      });
      await refresh();
      await refreshProviderCaches();
      closeDialog();
    } catch (error) {
      handleError(error, 'modelStore.error.start');
    } finally {
      setBusyId(null);
    }
  };

  const requestStop = (model: ModelStoreModel) => openDialog('stop', model);

  const handleConfirmStop = async () => {
    if (!selected) return;
    setBusyId(selected.id);
    try {
      await ipcBridge.localModel.stop.invoke({ id: selected.id });
      await refresh();
      await refreshProviderCaches();
      closeDialog();
    } catch (error) {
      handleError(error, 'modelStore.error.stop');
    } finally {
      setBusyId(null);
    }
  };

  const requestUninstall = (model: ModelStoreModel) => openDialog('uninstall', model);

  const handleConfirmUninstall = async () => {
    if (!selected) return;
    setBusyId(selected.id);
    try {
      if (selected.status === 'downloading') {
        await ipcBridge.localModel.cancel.invoke({ id: selected.id });
      } else {
        await ipcBridge.localModel.uninstall.invoke({ id: selected.id });
      }
      await refresh();
      await refreshProviderCaches();
      closeDialog();
    } catch (error) {
      handleError(error, 'modelStore.error.uninstall');
    } finally {
      setBusyId(null);
    }
  };

  const handleOpenLocation = async (model: ModelStoreModel) => {
    if (!model.installPath) return;
    try {
      await ipcBridge.shell.showItemInFolder.invoke(model.installPath);
    } catch (error) {
      handleError(error, 'modelStore.error.openLocation');
    }
  };

  return (
    <div className='app-page-shell w-full min-h-0 flex-1 box-border overflow-hidden flex flex-col'>
      <div className='mx-auto flex h-full min-h-0 w-full max-w-1280px flex-col gap-16px'>
        <header className='flex shrink-0 items-center justify-between gap-16px'>
          <h1 className='!m-0 shrink-0 text-20px font-600 leading-32px text-t-primary'>{t('modelStore.title')}</h1>
          <Input
            value={query}
            onChange={setQuery}
            allowClear
            aria-label={t('modelStore.searchPlaceholder')}
            className={SEARCH_FIELD_CLASS}
            placeholder={t('modelStore.searchPlaceholder')}
            prefix={<Search theme='outline' size={14} fill='currentColor' className='text-t-tertiary' />}
          />
        </header>

        <SegmentedTabs
          items={partitionItems}
          activeKey={partition}
          onChange={(key) => {
            setPartition(key as ModelStorePartition);
            setStatus('all');
          }}
          className='self-start'
        />

        <SegmentedTabs
          items={statusItems}
          activeKey={status}
          onChange={(key) => setStatus(key as ModelStoreStatus)}
          size='sm'
          className='self-start'
        />

        <div className='min-h-0 flex-1 overflow-y-auto pb-16px'>
          {visible.length === 0 ? (
            <div className='flex h-full min-h-240px items-center justify-center'>
              <Empty
                description={
                  partition === 'houmo' && houmoError
                    ? t('modelStore.partition.error')
                    : partition === 'houmo' && isLoading
                      ? t('modelStore.partition.loading')
                      : t('modelStore.empty')
                }
              />
            </div>
          ) : (
            <div className='grid grid-cols-1 items-stretch gap-16px sm:grid-cols-2 xl:grid-cols-3'>
              {visible.map((model) => (
                <ModelStoreCard
                  key={model.id}
                  model={model}
                  busy={busyId === model.id}
                  onDownload={handleDownload}
                  onCancel={handleCancel}
                  onPlay={(item) => handleStart(item)}
                  onStop={requestStop}
                  onDetails={(item) => openDialog('detail', item)}
                  onLaunchConfig={(item) => openDialog('launch', item)}
                  onUninstall={requestUninstall}
                />
              ))}
            </div>
          )}
        </div>
      </div>

      <LocalModelDetailModal
        visible={dialog === 'detail'}
        model={selected}
        onClose={closeDialog}
        onOpenLocation={handleOpenLocation}
      />
      <LocalModelLaunchModal
        visible={dialog === 'launch'}
        model={selected}
        busy={busyId === selected?.id}
        onClose={closeDialog}
        onStart={handleStart}
        onStop={requestStop}
      />
      <LocalModelStopModal
        visible={dialog === 'stop'}
        modelName={selected ? selected.displayName[i18n.language.toLowerCase().startsWith('zh') ? 'zh' : 'en'] : ''}
        loading={busyId === selected?.id}
        onCancel={closeDialog}
        onConfirm={handleConfirmStop}
      />
      <LocalModelUninstallModal
        visible={dialog === 'uninstall'}
        model={selected}
        loading={busyId === selected?.id}
        onCancel={closeDialog}
        onConfirm={handleConfirmUninstall}
      />
    </div>
  );
};

export default ModelStorePage;
