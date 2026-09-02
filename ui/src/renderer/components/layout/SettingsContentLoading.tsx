import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';
import LoadingState from '@/renderer/components/beautifulUi/loadingState/LoadingState';

type SettingsContentLoadingProps = {
  className?: string;
  label?: string;
};

/** Shared first-paint loading state for settings route and data boundaries. */
const SettingsContentLoading: React.FC<SettingsContentLoadingProps> = ({ className, label }) => {
  const { t } = useTranslation();

  return (
    <div
      className={classNames(
        'settings-content-loading flex min-h-260px w-full flex-1 items-center justify-center bg-base',
        className
      )}
      data-testid='settings-content-loading'
      aria-busy='true'
    >
      <LoadingState variant='drive' label={label ?? t('common.loading', { defaultValue: 'Loading...' })} />
    </div>
  );
};

export default SettingsContentLoading;
