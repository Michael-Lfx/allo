

import React from 'react';
import { captureException } from '@/renderer/utils/analytics/telemetry';
import { useTranslation } from 'react-i18next';
import { claimDynamicImportReload, isDynamicImportFailure } from './routeErrorRecovery';

interface RouteErrorBoundaryProps {
  children: React.ReactNode;
  /** Clears a captured route error when navigation changes the rendered target. */
  resetKey?: string;
  /** Root failures need an application reload; route failures can retry in place. */
  scope?: 'route' | 'application';
}

interface RouteErrorBoundaryState {
  error: Error | null;
  componentStack: string | null;
  dynamicImportReloadExhausted: boolean;
}

interface RouteErrorFallbackProps {
  error: Error;
  componentStack: string | null;
  isApplicationFailure: boolean;
  actionKey: RouteErrorActionKey;
  onAction: () => void;
}

type RouteErrorActionKey = 'reloadAgain' | 'reloadApplication' | 'retry';

const RouteErrorFallback: React.FC<RouteErrorFallbackProps> = ({
  error,
  componentStack,
  isApplicationFailure,
  actionKey,
  onAction,
}) => {
  const { t } = useTranslation();
  const actionLabel = t(`common.routeError.${actionKey}`, {
    defaultValue:
      actionKey === 'reloadAgain'
        ? '再次重新加载应用'
        : actionKey === 'reloadApplication'
          ? '重新加载应用'
          : '重试',
  });

  return (
    <div
      role='alert'
      style={{
        height: '100%',
        width: '100%',
        overflow: 'auto',
        padding: '24px',
        boxSizing: 'border-box',
        background: 'var(--flowy-panel, var(--color-bg-1, Canvas))',
        color: 'var(--flowy-text-primary, var(--color-text-1, CanvasText))',
        fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
        fontSize: '13px',
        lineHeight: 1.55,
      }}
    >
      <div
        style={{
          fontSize: '15px',
          fontWeight: 700,
          color: 'var(--flowy-danger, rgb(var(--danger-6)))',
          marginBottom: '12px',
        }}
      >
        {isApplicationFailure
          ? t('common.routeError.applicationTitle', {
              defaultValue: '应用渲染出错（已捕获，未显示空白窗口）',
            })
          : t('common.routeError.routeTitle', {
              defaultValue: '页面渲染出错（已被路由错误边界捕获，未影响其它页面）',
            })}
      </div>
      <div style={{ fontWeight: 700, marginBottom: '8px', userSelect: 'text' }}>
        {error.name}: {error.message}
      </div>
      <button
        type='button'
        onClick={onAction}
        style={{
          marginBottom: '16px',
          padding: '4px 12px',
          border: '1px solid var(--flowy-danger, rgb(var(--danger-6)))',
          borderRadius: '6px',
          background: 'var(--flowy-interactive, var(--color-bg-2, transparent))',
          color: 'var(--flowy-text-primary, var(--color-text-1, CanvasText))',
          cursor: 'pointer',
        }}
      >
        {actionLabel}
      </button>
      {error.stack ? (
        <>
          <div style={{ opacity: 0.7, marginBottom: '4px' }}>
            {t('common.routeError.stack', { defaultValue: 'Stack' })}
          </div>
          <pre style={{ whiteSpace: 'pre-wrap', userSelect: 'text', margin: '0 0 16px' }}>{error.stack}</pre>
        </>
      ) : null}
      {componentStack ? (
        <>
          <div style={{ opacity: 0.7, marginBottom: '4px' }}>
            {t('common.routeError.componentStack', { defaultValue: 'Component stack' })}
          </div>
          <pre style={{ whiteSpace: 'pre-wrap', userSelect: 'text', margin: 0 }}>{componentStack}</pre>
        </>
      ) : null}
    </div>
  );
};

/**
 * RouteErrorBoundary — 路由级错误边界
 *
 * The app previously had NO error boundary, so any render/throw inside a route
 * blanked the entire window (white screen) with no visible cause. This boundary
 * wraps each lazily-loaded route element (via `withRouteFallback`) so a crash in
 * one page renders a readable error panel — message + stack + React component
 * stack — instead of taking down the whole shell. The surrounding app chrome
 * (titlebar, primary sidebar) stays alive, and the error text is selectable so
 * it can be copied for diagnosis.
 *
 * React Router can reuse a boundary instance when only a route parameter
 * changes. `resetKey` therefore clears stale failures explicitly on navigation.
 */
class RouteErrorBoundary extends React.Component<RouteErrorBoundaryProps, RouteErrorBoundaryState> {
  state: RouteErrorBoundaryState = {
    error: null,
    componentStack: null,
    dynamicImportReloadExhausted: false,
  };

  static getDerivedStateFromError(error: Error): Partial<RouteErrorBoundaryState> {
    return { error, dynamicImportReloadExhausted: false };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo): void {
    // Surface to the console too (devtools, if available) — keep the on-screen
    // panel as the primary channel since release builds may not expose devtools.
    // eslint-disable-next-line no-console
    console.error(
      `[RouteErrorBoundary] ${this.props.scope === 'application' ? 'application' : 'route'} crashed:`,
      error,
      info.componentStack
    );
    captureException(error);
    this.setState({ componentStack: info.componentStack ?? null });
  }

  componentDidUpdate(previousProps: RouteErrorBoundaryProps): void {
    if (previousProps.resetKey !== this.props.resetKey && this.state.error) {
      this.setState({ error: null, componentStack: null, dynamicImportReloadExhausted: false });
    }
  }

  private handleReset = (): void => {
    if (isDynamicImportFailure(this.state.error)) {
      let storage: Storage | undefined;
      try {
        storage = window.sessionStorage;
      } catch {
        storage = undefined;
      }
      const shouldReload = claimDynamicImportReload(storage, window.location.href);
      if (shouldReload) {
        this.handleApplicationReload();
        return;
      }
      this.setState({ dynamicImportReloadExhausted: true });
      return;
    }
    this.setState({ error: null, componentStack: null, dynamicImportReloadExhausted: false });
  };

  private handleApplicationReload = (): void => {
    window.location.reload();
  };

  render(): React.ReactNode {
    const { error, componentStack, dynamicImportReloadExhausted } = this.state;
    if (!error) return this.props.children;
    const isApplicationFailure = this.props.scope === 'application';
    const isDynamicImportError = isDynamicImportFailure(error);
    const requiresApplicationReload = isApplicationFailure || isDynamicImportError;
    const retryHandler =
      isApplicationFailure || (isDynamicImportError && dynamicImportReloadExhausted)
        ? this.handleApplicationReload
        : this.handleReset;
    const actionKey: RouteErrorActionKey =
      isDynamicImportError && dynamicImportReloadExhausted
        ? 'reloadAgain'
        : requiresApplicationReload
          ? 'reloadApplication'
          : 'retry';

    return (
      <RouteErrorFallback
        error={error}
        componentStack={componentStack}
        isApplicationFailure={isApplicationFailure}
        actionKey={actionKey}
        onAction={retryHandler}
      />
    );
  }
}

export default RouteErrorBoundary;
