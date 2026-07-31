import classNames from 'classnames';
import React, { useId } from 'react';

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
  <header className='flex flex-col gap-12px sm:flex-row sm:items-start sm:justify-between'>
    <div className='min-w-0'>
      <h1 className='m-0 text-20px font-600 leading-28px text-t-primary'>{title}</h1>
      {description && <div className='mt-4px max-w-680px text-13px leading-20px text-t-secondary'>{description}</div>}
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
  <div className={classNames('bg-2 rd-16px px-[12px] py-16px md:px-[32px]', className)}>{children}</div>
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
    <section aria-labelledby={titleId} className={className}>
      <header className='mb-8px flex flex-col gap-8px px-4px sm:flex-row sm:items-end sm:justify-between'>
        <div className='min-w-0'>
          <h2 id={titleId} className='m-0 text-13px font-500 leading-20px text-t-secondary'>
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
