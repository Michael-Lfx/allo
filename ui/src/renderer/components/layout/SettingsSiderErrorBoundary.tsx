import React from 'react';
import { useTranslation } from 'react-i18next';

interface SettingsSiderErrorBoundaryProps {
  children: React.ReactNode;
  resetKey: string;
}

interface SettingsSiderErrorBoundaryState {
  error: Error | null;
  componentStack: string | null;
}

interface SettingsSiderErrorFallbackProps {
  error: Error;
  componentStack: string | null;
  requiresReload: boolean;
  onRetry: () => void;
  onReload: () => void;
}

const SettingsSiderErrorFallback: React.FC<SettingsSiderErrorFallbackProps> = ({
  error,
  componentStack,
  requiresReload,
  onRetry,
  onReload,
}) => {
  const { t } = useTranslation();

  return (
    <div
      role='alert'
      className='h-full w-full overflow-auto p-20px bg-[var(--color-bg-2)] text-t-primary'
    >
      <div className='text-15px font-600 text-[rgb(var(--danger-6))]'>
        {t('settings.settingsSiderErrorTitle')}
      </div>
      <div className='mt-8px text-13px break-all select-text'>
        {error.name}: {error.message}
      </div>
      <div className='mt-12px flex gap-8px'>
        <button
          type='button'
          className='px-12px py-5px rounded-6px bg-2 border border-solid border-border-2'
          onClick={requiresReload ? onReload : onRetry}
        >
          {requiresReload ? t('settings.reloadApplication') : t('common.retry')}
        </button>
        {requiresReload ? null : (
          <button
            type='button'
            className='px-12px py-5px rounded-6px bg-2 border border-solid border-border-2'
            onClick={onReload}
          >
            {t('settings.reloadApplication')}
          </button>
        )}
      </div>
      {componentStack ? (
        <pre className='mt-12px whitespace-pre-wrap text-11px select-text'>
          {t('settings.settingsSiderErrorDetails')}: {componentStack}
        </pre>
      ) : null}
    </div>
  );
};

/**
 * The settings rail is lazy-loaded independently from the application shell.
 * A stale Vite chunk or a settings-only render error must not be promoted to
 * the root route boundary, especially while a work-root restart is in flight.
 */
export default class SettingsSiderErrorBoundary extends React.Component<
  SettingsSiderErrorBoundaryProps,
  SettingsSiderErrorBoundaryState
> {
  state: SettingsSiderErrorBoundaryState = { error: null, componentStack: null };

  static getDerivedStateFromError(error: Error): Partial<SettingsSiderErrorBoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    console.error('[SettingsSiderErrorBoundary] settings rail failed:', error, info.componentStack);
    this.setState({ componentStack: info.componentStack ?? null });
  }

  componentDidUpdate(previousProps: SettingsSiderErrorBoundaryProps): void {
    if (previousProps.resetKey !== this.props.resetKey && this.state.error) {
      this.setState({ error: null, componentStack: null });
    }
  }

  private isDynamicImportFailure(): boolean {
    return /Failed to fetch dynamically imported module|Importing a module script failed|dynamically imported module/i.test(
      this.state.error?.message ?? ''
    );
  }

  private retry = (): void => {
    this.setState({ error: null, componentStack: null });
  };

  private reload = (): void => {
    window.location.reload();
  };

  render(): React.ReactNode {
    const { error, componentStack } = this.state;
    if (!error) return this.props.children;
    const requiresReload = this.isDynamicImportFailure();

    return (
      <SettingsSiderErrorFallback
        error={error}
        componentStack={componentStack}
        requiresReload={requiresReload}
        onRetry={this.retry}
        onReload={this.reload}
      />
    );
  }
}
