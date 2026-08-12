import classNames from 'classnames';
import React, { useId } from 'react';
import { Button } from '@arco-design/web-react';
import '@/renderer/pages/settings/components/settings.css';

type SettingsPageHeaderProps = {
  title: React.ReactNode;
  description?: React.ReactNode;
  meta?: React.ReactNode;
  action?: React.ReactNode;
};

export const SettingsPageHeader: React.FC<SettingsPageHeaderProps> = ({
  title,
  description,
  meta,
  action,
}) => (
  <header className='flowy-settings-page-header flex flex-col gap-12px sm:flex-row sm:items-start sm:justify-between'>
    <div className='min-w-0'>
      <h1 className='m-0 text-22px font-600 leading-30px text-t-primary'>{title}</h1>
      {description && <div className='mt-4px max-w-720px text-13px leading-20px text-t-secondary'>{description}</div>}
      {meta && <div className='mt-10px'>{meta}</div>}
    </div>
    {action && <div className='shrink-0'>{action}</div>}
  </header>
);

type SettingsPanelProps = {
  className?: string;
  children: React.ReactNode;
};

export const SettingsPanel: React.FC<SettingsPanelProps> = ({ className, children }) => (
  <div className={classNames('flowy-settings-panel px-16px py-16px md:px-24px', className)}>{children}</div>
);

export const SettingsNestedRows: React.FC<SettingsPanelProps> = ({ className, children }) => (
  <div
    className={classNames(
      'mb-12px ml-12px pl-12px md:ml-20px md:pl-20px',
      className
    )}
  >
    {children}
  </div>
);

export const SettingsPanelFooter: React.FC<SettingsPanelProps> = ({ className, children }) => (
  <div className={classNames('flex pt-16px', className)}>{children}</div>
);

type SettingsListProps = SettingsPanelProps & {
  'aria-label'?: string;
};

/** A single settings surface: rows are separated, rather than nested in cards. */
export const SettingsList: React.FC<SettingsListProps> = ({ className, children, ...props }) => (
  <div className={classNames('flowy-settings-list', className)} {...props}>
    {children}
  </div>
);

export type SettingsStatusTone = 'neutral' | 'success' | 'warning' | 'error' | 'restart-required';

type SettingsStatusProps = {
  children: React.ReactNode;
  tone?: SettingsStatusTone;
  className?: string;
};

export const SettingsStatus: React.FC<SettingsStatusProps> = ({ children, tone = 'neutral', className }) => (
  <span className={classNames('flowy-settings-status', `flowy-settings-status--${tone}`, className)} role={tone === 'error' ? 'alert' : 'status'}>
    {children}
  </span>
);

export type SettingsRowProps = {
  label: React.ReactNode;
  description?: React.ReactNode;
  control?: React.ReactNode;
  /**
   * The control slot is intentionally semantic rather than a one-size-fits-all
   * width. It keeps switches compact while giving fields and action groups the
   * room they need in translated interfaces.
   */
  controlLayout?: 'compact' | 'field' | 'actions' | 'compound';
  status?: React.ReactNode;
  disabled?: boolean;
  interactive?: boolean;
  className?: string;
  children?: React.ReactNode;
};

export const SettingsRow: React.FC<SettingsRowProps> = ({
  label,
  description,
  control,
  controlLayout = 'field',
  status,
  disabled = false,
  interactive = false,
  className,
  children,
}) => (
  <div
    className={classNames(
      'flowy-settings-row',
      { 'flowy-settings-row--disabled': disabled, 'flowy-settings-row--interactive': interactive },
      className
    )}
  >
    <div className='flowy-settings-row__copy'>
      <div className='flowy-settings-row__label'>{label}</div>
      {description && <div className='flowy-settings-row__description'>{description}</div>}
      {status && <div className='flowy-settings-row__status'>{status}</div>}
    </div>
    {(control || children) && (
      <div className={classNames('flowy-settings-row__control', `flowy-settings-row__control--${controlLayout}`)}>
        {control ?? children}
      </div>
    )}
  </div>
);

type SettingsControlGroupProps = {
  children: React.ReactNode;
  className?: string;
};

/**
 * Keeps related buttons together without allowing icon and label pairs to
 * break apart. The group itself may wrap when a translated action set is wide.
 */
export const SettingsControlGroup: React.FC<SettingsControlGroupProps> = ({ children, className }) => (
  <div className={classNames('flowy-settings-control-group', className)}>{children}</div>
);

export type SettingsPermissionRowProps = Omit<SettingsRowProps, 'status'> & {
  state: 'ready' | 'attention' | 'unsupported' | 'restart-required';
  stateLabel: React.ReactNode;
};

export const SettingsPermissionRow: React.FC<SettingsPermissionRowProps> = ({
  state,
  stateLabel,
  disabled,
  ...props
}) => (
  <SettingsRow
    {...props}
    disabled={disabled || state === 'unsupported'}
    status={<SettingsStatus tone={state === 'ready' ? 'success' : state === 'restart-required' ? 'restart-required' : state === 'attention' ? 'warning' : 'neutral'}>{stateLabel}</SettingsStatus>}
  />
);

type SettingsEmptyStateProps = {
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  className?: string;
};

export const SettingsEmptyState: React.FC<SettingsEmptyStateProps> = ({ title, description, action, className }) => (
  <div className={classNames('flowy-settings-empty-state', className)}>
    <div className='text-14px font-500 text-t-primary'>{title}</div>
    {description && <div className='mt-4px max-w-480px text-13px leading-20px text-t-secondary'>{description}</div>}
    {action && <div className='mt-12px'>{action}</div>}
  </div>
);

type SettingsActionBarProps = {
  visible: boolean;
  saveLabel: React.ReactNode;
  onSave: () => void;
  resetLabel?: React.ReactNode;
  onReset?: () => void;
  loading?: boolean;
  error?: React.ReactNode;
  status?: React.ReactNode;
  className?: string;
};

/** Appears only for an explicit, dirty save flow. Auto-saved settings never use it. */
export const SettingsActionBar: React.FC<SettingsActionBarProps> = ({
  visible,
  saveLabel,
  onSave,
  resetLabel,
  onReset,
  loading = false,
  error,
  status,
  className,
}) => {
  if (!visible) return null;

  return (
    <div className={classNames('flowy-settings-action-bar', className)} role='status'>
      <div className='min-w-0 text-12px text-t-secondary'>
        {error ? <SettingsStatus tone='error'>{error}</SettingsStatus> : status}
      </div>
      <div className='flex shrink-0 gap-8px'>
        {onReset && resetLabel && <Button type='secondary' disabled={loading} onClick={onReset}>{resetLabel}</Button>}
        <Button type='primary' loading={loading} onClick={onSave}>{saveLabel}</Button>
      </div>
    </div>
  );
};

type SettingsGroupProps = {
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  className?: string;
  children: React.ReactNode;
};

export const SettingsGroup: React.FC<SettingsGroupProps> = ({
  title,
  description,
  action,
  className,
  children,
}) => {
  const titleId = useId();

  return (
    <section aria-labelledby={titleId} className={classNames('flowy-settings-section', className)}>
      <header className='mb-10px flex flex-col gap-8px px-4px sm:flex-row sm:items-end sm:justify-between'>
        <div className='min-w-0'>
          <h2 id={titleId} className='m-0 text-15px font-600 leading-22px text-t-primary'>
            {title}
          </h2>
          {description && <div className='mt-2px max-w-680px text-12px leading-18px text-t-tertiary'>{description}</div>}
        </div>
        {action && <div className='shrink-0'>{action}</div>}
      </header>
      {children}
    </section>
  );
};

export const SettingsSection = SettingsGroup;
