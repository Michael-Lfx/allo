/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Dropdown, Menu, Progress } from '@arco-design/web-react';
import {
  Close,
  Download,
  More,
  PlayOne,
  SquareSmall,
} from '@icon-park/react';
import type { ModelStoreModel } from './types';
import { resolveCardActions } from './actions';
import {
  cleanDescription,
  formatDownloadError,
  formatSize,
  formatSpeed,
  getAvatarHue,
  getAvatarText,
  getCapabilityTags,
  pickLocalized,
  resolveDownloadPhase,
} from './format';

interface ModelStoreCardProps {
  model: ModelStoreModel;
  busy?: boolean;
  onDownload: (model: ModelStoreModel) => void;
  onCancel: (model: ModelStoreModel) => void;
  onPlay: (model: ModelStoreModel) => void;
  onStop: (model: ModelStoreModel) => void;
  onDetails: (model: ModelStoreModel) => void;
  onLaunchConfig: (model: ModelStoreModel) => void;
  onUninstall: (model: ModelStoreModel) => void;
}

const LIVE_STATUS_KEYS: Record<string, string> = {
  installed: 'modelStore.statusTag.installed',
  running: 'modelStore.statusTag.running',
  starting: 'modelStore.statusTag.starting',
  downloading: 'modelStore.statusTag.downloading',
};

/** Catalog card with herdsman-style hover actions and live install state. */
const ModelStoreCard: React.FC<ModelStoreCardProps> = ({
  model,
  busy,
  onDownload,
  onCancel,
  onPlay,
  onStop,
  onDetails,
  onLaunchConfig,
  onUninstall,
}) => {
  const { i18n, t } = useTranslation();
  const [iconFailed, setIconFailed] = useState(false);
  const name = pickLocalized(model.displayName, i18n.language) || model.id;
  const description = cleanDescription(pickLocalized(model.description, i18n.language));
  const hue = getAvatarHue(model.id);
  const tags = getCapabilityTags(model);
  const showIcon = Boolean(model.icon) && !iconFailed;
  const actions = resolveCardActions(model.status);
  const liveKey = LIVE_STATUS_KEYS[model.status];
  const downloadPhase = resolveDownloadPhase(model);

  const menu = (
    <Menu
      onClickMenuItem={(key) => {
        if (key === 'details') onDetails(model);
        else if (key === 'launch-config') onLaunchConfig(model);
        else if (key === 'uninstall') onUninstall(model);
      }}
    >
      <Menu.Item key='details'>{t('modelStore.actions.details')}</Menu.Item>
      {actions.launchConfig && (
        <Menu.Item key='launch-config'>{t('modelStore.actions.launchConfig')}</Menu.Item>
      )}
      {actions.uninstall && (
        <Menu.Item key='uninstall'>{t('modelStore.actions.uninstall')}</Menu.Item>
      )}
    </Menu>
  );

  return (
    <article className='group box-border flex h-auto min-w-0 flex-col gap-10px self-stretch rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-14px transition-colors hover:border-[var(--color-border-3)] focus-within:border-[var(--color-border-3)]'>
      <header className='flex min-w-0 items-center gap-10px'>
        {showIcon ? (
          <span className='flex size-38px shrink-0 items-center justify-center overflow-hidden rounded-10px border border-solid border-[var(--color-border-1)] bg-[var(--color-bg-1)]'>
            <img
              src={model.icon}
              alt=''
              loading='lazy'
              className='size-full object-contain'
              onError={() => setIconFailed(true)}
            />
          </span>
        ) : (
          <span
            aria-hidden='true'
            className='flex size-38px shrink-0 items-center justify-center rounded-10px text-14px font-600'
            style={{
              backgroundColor: `hsl(${hue} 60% 50% / 0.16)`,
              color: `hsl(${hue} 70% 45%)`,
            }}
          >
            {getAvatarText(name)}
          </span>
        )}
        <div className='min-w-0 flex-1'>
          <div className='flex min-w-0 items-center gap-6px'>
            <span className='min-w-0 flex-1 truncate text-14px font-[500] leading-20px text-t-primary' title={name}>
              {name}
            </span>
            {liveKey && (
              <span
                className={[
                  'shrink-0 rounded-4px px-5px py-1px text-10px font-500',
                  model.status === 'running'
                    ? 'bg-success-1 text-success-6'
                    : model.status === 'starting' || model.status === 'downloading'
                      ? 'bg-primary-1 text-primary-6'
                      : 'bg-[var(--color-fill-2)] text-t-tertiary',
                ].join(' ')}
              >
                {t(liveKey)}
              </span>
            )}
          </div>
          <div className='mt-3px flex min-w-0 items-center gap-5px text-11px text-t-tertiary'>
            <span className='truncate' translate='no'>
              {model.type}
            </span>
            {model.parameterCount > 0 && (
              <span className='shrink-0 tabular-nums' translate='no'>
                · {model.parameterCount}B
              </span>
            )}
            {model.sizeBytes > 0 && (
              <span className='shrink-0 tabular-nums'>· {formatSize(model.sizeBytes)}</span>
            )}
          </div>
        </div>
      </header>

      <p className='line-clamp-2 min-h-36px text-12px leading-18px text-t-secondary' title={description || undefined}>
        {description || t('modelStore.card.noDescription')}
      </p>

      <div className='flex min-h-22px flex-nowrap items-center gap-4px overflow-hidden'>
        {tags.map((tag) => (
          <span
            key={tag}
            className='shrink-0 whitespace-nowrap rounded-4px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-2)] px-6px py-2px text-11px text-t-secondary'
            translate='no'
          >
            {tag}
          </span>
        ))}
      </div>

      {actions.progress && (
        <div className='flex items-center gap-8px'>
          <Progress
            percent={Math.round(model.downloadProgress)}
            size='small'
            showText={false}
            className='min-w-0 flex-1'
          />
          <span className='shrink-0 whitespace-nowrap text-11px text-t-tertiary'>
            {t(downloadPhase.labelKey)}
          </span>
          {downloadPhase.showPercent && (
            <span className='shrink-0 text-11px tabular-nums text-t-tertiary'>
              {Math.round(model.downloadProgress)}%
            </span>
          )}
          {downloadPhase.showSpeed && (
            <span className='shrink-0 text-11px tabular-nums text-t-tertiary'>
              {formatSpeed(model.downloadSpeed)}
            </span>
          )}
        </div>
      )}

      {model.downloadError && (
        <p className='m-0 text-11px leading-16px text-danger-6'>
          {formatDownloadError(t, model.downloadError)}
        </p>
      )}

      <div className='flex items-center justify-end gap-6px opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100'>
        {actions.download && (
          <Button
            type='primary'
            size='small'
            loading={busy}
            className='flowy-icon-text-btn model-store-action-btn model-store-themed-primary'
            onClick={() => onDownload(model)}
            icon={<Download theme='outline' size={14} fill='#ffffff' />}
          >
            {t('modelStore.actions.download')}
          </Button>
        )}
        {actions.cancel && (
          <Button
            size='small'
            className='flowy-icon-text-btn model-store-action-btn'
            onClick={() => onCancel(model)}
            icon={<Close theme='outline' size={14} fill='currentColor' />}
          >
            {t('modelStore.actions.cancel')}
          </Button>
        )}
        {actions.play && (
          <Button
            type='primary'
            status='success'
            shape='circle'
            size='small'
            loading={busy}
            className='model-store-action-btn'
            aria-label={t('modelStore.actions.start')}
            title={t('modelStore.actions.start')}
            onClick={() => onPlay(model)}
            icon={<PlayOne theme='outline' size={14} fill='#ffffff' />}
          />
        )}
        {actions.stop && (
          <Button
            type='primary'
            shape='circle'
            size='small'
            className='model-store-action-btn model-store-themed-primary'
            aria-label={t('modelStore.actions.stop')}
            title={t('modelStore.actions.stop')}
            onClick={() => onStop(model)}
            icon={<SquareSmall theme='filled' size={14} fill='#ffffff' />}
          />
        )}
        <Dropdown droplist={menu} trigger='click' position='br'>
          <Button
            size='small'
            shape='circle'
            className='model-store-action-btn'
            aria-label={t('modelStore.actions.more')}
            title={t('modelStore.actions.more')}
            icon={<More theme='outline' size={14} fill='currentColor' />}
          />
        </Dropdown>
      </div>
    </article>
  );
};

export default ModelStoreCard;
