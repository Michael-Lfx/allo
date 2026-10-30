import type { McpServerId } from '@/common/types/ids';
import type { IMcpServer } from '@/common/config/storage';
import { Button, Tag, Tooltip } from '@arco-design/web-react';
import { Check, CloseSmall, DeleteFour, Info, Key, Link, Login, PauseOne, Server, Shop, Terminal, Write } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import type { McpOAuthStatus } from '@/renderer/hooks/mcp/useMcpOAuth';
import type { McpActivationItemState } from '@/renderer/hooks/mcp/useMcpActivationFlow';
import FeedbackButton from '@/renderer/components/base/FeedbackButton';
import { iconColors } from '@/renderer/styles/colors';
import { MCP_SERVER_TITLE_CLASS } from './mcpServerCollapse';
import { getMcpTransportLabelKey, getSafeMcpEndpoint } from './McpServerDetails';
import { getMcpMarketOrigin } from './mcpImportUtils';
import McpLoadingIndicator from './McpLoadingIndicator';

interface McpServerHeaderProps {
  server: IMcpServer;
  isTestingConnection: boolean;
  activationState?: McpActivationItemState;
  activationError?: string;
  oauthStatus?: McpOAuthStatus;
  isLoggingIn?: boolean;
  isTogglingEnabled?: boolean;
  /** Extension-contributed servers are read-only */
  isReadOnly?: boolean;
  onTestConnection: (server: IMcpServer) => void;
  onEditServer: (server: IMcpServer) => void;
  onDeleteServer: (serverId: McpServerId) => void;
  onToggleEnabled: (server: IMcpServer) => void;
  onOAuthLogin?: (server: IMcpServer) => void;
}

const STATUS_ICON_SIZE = 16;
const STATUS_ICON_SLOT_CLASS = 'inline-flex h-24px w-24px shrink-0 items-center justify-center';
const HEADER_META_CLASS = 'inline-flex h-22px shrink-0 items-center gap-4px text-12px text-t-secondary';

const getEffectiveLoading = (
  isTestingConnection: boolean,
  activationState?: McpActivationItemState,
  oauthStatus?: McpOAuthStatus
) =>
  isTestingConnection || activationState === 'queued' || activationState === 'testing' || Boolean(oauthStatus?.isChecking);

const isServerEnabled = (server: IMcpServer, activationState?: McpActivationItemState) =>
  activationState === 'enabled' || server.enabled;

const getStatusIcon = (
  server: IMcpServer,
  isEnabled: boolean,
  oauthStatus?: McpOAuthStatus,
  isLoading?: boolean,
  activationState?: McpActivationItemState
) => {
  if (isLoading) return <McpLoadingIndicator size='medium' />;
  if (activationState === 'failed' || activationState === 'config-changed' || server.last_test_status === 'error') {
    return <CloseSmall size={STATUS_ICON_SIZE} fill={iconColors.danger} />;
  }
  if (activationState === 'needs-auth' || oauthStatus?.needsLogin) {
    return <Key size={STATUS_ICON_SIZE} fill={iconColors.warning} />;
  }
  if (isEnabled) {
    return <Check size={STATUS_ICON_SIZE} fill={iconColors.success} />;
  }
  return <Info theme='outline' size={STATUS_ICON_SIZE} fill={iconColors.secondary} />;
};

const getStatusText = (
  server: IMcpServer,
  isEnabled: boolean,
  activationState: McpActivationItemState | undefined,
  oauthStatus: McpOAuthStatus | undefined,
  isLoading: boolean,
  t: (key: string, options?: Record<string, unknown>) => string
) => {
  if (activationState === 'queued') return t('settings.mcpActivationPreparing');
  if (oauthStatus?.isChecking) return t('settings.mcpOAuthChecking');
  if (activationState === 'testing' || isLoading) return t('settings.mcpActivationChecking');
  if (activationState === 'config-changed') return t('settings.mcpConfigChangedDuringTest');
  if (activationState === 'needs-auth' || oauthStatus?.needsLogin) return t('settings.mcpActivationNeedsAuthHint');
  if (activationState === 'failed' || server.last_test_status === 'error') {
    return t('settings.mcpNextStepRetry', { action: t('settings.mcpStatusCtaRetry') });
  }
  if (isEnabled) return t('settings.mcpEnabledAndSelectable');
  if (server.last_test_status === 'connected') {
    return t('settings.mcpNextStepEnable', { action: t('settings.mcpStatusCtaEnable') });
  }
  return t('settings.mcpNextStepCheck', { action: t('settings.mcpStatusCtaCheck') });
};

const getSourceMeta = (server: IMcpServer, t: (key: string) => string) => {
  if (server.builtin) {
    return { icon: <Server size='13' fill='currentColor' />, label: t('settings.mcpBuiltinSource') };
  }
  if (getMcpMarketOrigin(server)) {
    return { icon: <Shop size='13' fill='currentColor' />, label: t('settings.mcpMarketSource') };
  }
  return null;
};

const getTransportIcon = (transport: IMcpServer['transport']) => {
  if (transport.type === 'stdio') return <Terminal size='13' fill='currentColor' />;
  return <Link size='13' fill='currentColor' />;
};

const StatusActionCta: React.FC<{
  server: IMcpServer;
  activationState?: McpActivationItemState;
  isTestingConnection: boolean;
  isCheckingOAuth?: boolean;
  isTogglingEnabled?: boolean;
  needsLogin: boolean;
  needsAuth: boolean;
  isLoggingIn?: boolean;
  onTestConnection: (server: IMcpServer) => void;
  onEditServer: (server: IMcpServer) => void;
  onToggleEnabled: (server: IMcpServer) => void;
  onOAuthLogin?: (server: IMcpServer) => void;
}> = ({
  server,
  activationState,
  isTestingConnection,
  isCheckingOAuth,
  isTogglingEnabled,
  needsLogin,
  needsAuth,
  isLoggingIn,
  onTestConnection,
  onEditServer,
  onToggleEnabled,
  onOAuthLogin,
}) => {
  const { t } = useTranslation();
  const isEnabled = isServerEnabled(server, activationState);

  if (activationState === 'queued' || activationState === 'testing' || isTestingConnection || isCheckingOAuth) {
    return (
      <Button
        size='mini'
        type='outline'
        disabled
        className='flowy-icon-text-btn !min-w-150px justify-center'
        icon={<McpLoadingIndicator size='small' />}
        aria-busy='true'
      >
        {activationState === 'queued'
          ? t('settings.mcpActivationPreparing')
          : isCheckingOAuth
            ? t('settings.mcpOAuthChecking')
            : t('settings.mcpActivationChecking')}
      </Button>
    );
  }

  if (needsLogin && onOAuthLogin) {
    return (
      <Button
        size='mini'
        type='primary'
        className='flowy-icon-text-btn'
        icon={<Login size='14' />}
        loading={isLoggingIn}
        onClick={() => onOAuthLogin(server)}
      >
        {t('settings.mcpLogin')}
      </Button>
    );
  }

  if (needsAuth) {
    return (
      <Button
        size='mini'
        type='primary'
        className='flowy-icon-text-btn'
        icon={<Write size='14' />}
        onClick={() => onEditServer(server)}
      >
        {t('settings.mcpViewConfig')}
      </Button>
    );
  }

  if (isEnabled) {
    return (
      <Button size='mini' type='outline' className='flowy-icon-text-btn' onClick={() => onTestConnection(server)}>
        {t('settings.mcpRetestConnection')}
      </Button>
    );
  }

  if (server.last_test_status === 'connected') {
    return (
      <Button
        size='mini'
        type='primary'
        className='flowy-icon-text-btn !border-[var(--color-text-1)] !text-[var(--color-bg-1)]'
        style={{ '--flowy-accent': 'var(--color-text-1)' } as React.CSSProperties}
        loading={isTogglingEnabled}
        onClick={() => onToggleEnabled(server)}
      >
        {t('settings.mcpStatusCtaEnable')}
      </Button>
    );
  }

  const canRetry =
    activationState === 'failed' || activationState === 'config-changed' || server.last_test_status === 'error';

  return (
    <Button
      size='mini'
      type={canRetry ? 'outline' : 'primary'}
      className='flowy-icon-text-btn'
      onClick={() => onTestConnection(server)}
    >
      {canRetry ? t('settings.mcpStatusCtaRetry') : t('settings.mcpStatusCtaCheck')}
    </Button>
  );
};

const McpServerHeader: React.FC<McpServerHeaderProps> = ({
  server,
  isTestingConnection,
  activationState,
  activationError,
  oauthStatus,
  isLoggingIn,
  isTogglingEnabled,
  isReadOnly,
  onTestConnection,
  onEditServer,
  onDeleteServer,
  onToggleEnabled,
  onOAuthLogin,
}) => {
  const { t } = useTranslation();
  const oauthCapable =
    server.transport.type === 'http' || server.transport.type === 'sse' || server.transport.type === 'streamable_http';
  const needsLogin = Boolean(oauthCapable && (activationState === 'needs-auth' || oauthStatus?.needsLogin));
  const isEnabled = isServerEnabled(server, activationState);
  const isLoading = getEffectiveLoading(isTestingConnection, activationState, oauthStatus);
  const statusText = getStatusText(server, isEnabled, activationState, oauthStatus, isLoading, t);
  const statusIcon = getStatusIcon(server, isEnabled, oauthStatus, isLoading, activationState);
  const sourceMeta = getSourceMeta(server, t);
  const isError =
    activationState === 'failed' || activationState === 'config-changed' || server.last_test_status === 'error';
  const needsAuth = activationState === 'needs-auth' || Boolean(oauthStatus?.needsLogin);
  const isFocusedActivation = activationState === 'queued' || activationState === 'testing';

  return (
    <div className='grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-start gap-x-16px gap-y-12px max-[900px]:grid-cols-1'>
      <div className='flex min-w-0 items-start gap-10px'>
        <Tooltip content={activationError || statusText} position='top'>
          <span className={`${STATUS_ICON_SLOT_CLASS} mt-2px cursor-default`} aria-label={statusText}>
            {statusIcon}
          </span>
        </Tooltip>
        <div className='min-w-0 flex-1'>
          <div className='flex min-w-0 flex-wrap items-center gap-x-8px gap-y-6px'>
            <span className={`${MCP_SERVER_TITLE_CLASS} truncate text-15px font-semibold text-t-primary`}>{server.name}</span>
            <Tag size='small' bordered={false} color={isEnabled ? 'green' : 'gray'} className='!flex-shrink-0 !text-11px'>
              {t(isEnabled ? 'settings.mcpEnabled' : 'settings.mcpDisabled')}
            </Tag>
            {isError && !isFocusedActivation ? <FeedbackButton /> : null}
          </div>
          <div className='mt-6px flex min-w-0 items-center gap-x-10px text-12px text-t-secondary'>
            <span className={HEADER_META_CLASS} title={server.transport.type}>
              {getTransportIcon(server.transport)}
              {t(getMcpTransportLabelKey(server.transport))}
            </span>
            {sourceMeta ? (
              <span className={HEADER_META_CLASS}>
                {sourceMeta.icon}
                {sourceMeta.label}
              </span>
            ) : null}
            <span className='min-w-0 truncate font-mono' title={getSafeMcpEndpoint(server)}>
              {getSafeMcpEndpoint(server)}
            </span>
          </div>
          <div
            className='mt-6px text-13px leading-20px text-t-primary'
            role={isLoading ? 'status' : undefined}
            aria-live={isLoading ? 'polite' : undefined}
          >
            {statusText}
          </div>
          {activationError ? (
            <div className='mt-4px truncate text-12px text-[var(--warning)]' role='alert' title={activationError}>
              {activationError}
            </div>
          ) : null}
        </div>
      </div>

      {!isReadOnly ? (
        <div
          className='flex shrink-0 items-center justify-end gap-8px max-[900px]:col-span-full max-[900px]:justify-start max-[768px]:w-full max-[768px]:flex-wrap'
          onClick={(event) => event.stopPropagation()}
        >
          {!needsLogin || !onOAuthLogin ? (
            <StatusActionCta
              server={server}
              activationState={activationState}
              isTestingConnection={isTestingConnection}
              isCheckingOAuth={Boolean(oauthStatus?.isChecking)}
              isTogglingEnabled={isTogglingEnabled}
              needsLogin={false}
              needsAuth={needsAuth}
              onTestConnection={onTestConnection}
              onEditServer={onEditServer}
              onToggleEnabled={onToggleEnabled}
            />
          ) : null}
          {needsLogin && onOAuthLogin ? (
            <StatusActionCta
              server={server}
              activationState={activationState}
              isTestingConnection={isTestingConnection}
              isCheckingOAuth={Boolean(oauthStatus?.isChecking)}
              isTogglingEnabled={isTogglingEnabled}
              needsLogin
              needsAuth
              isLoggingIn={isLoggingIn}
              onTestConnection={onTestConnection}
              onEditServer={onEditServer}
              onToggleEnabled={onToggleEnabled}
              onOAuthLogin={onOAuthLogin}
            />
          ) : null}
          {!server.builtin ? (
            <>
              {server.enabled ? (
                <Tooltip content={t('settings.mcpDisable')}>
                  <Button
                    size='mini'
                    type='text'
                    className='flowy-button-icon'
                    icon={<PauseOne size='14' />}
                    aria-label={t('settings.mcpDisable')}
                    onClick={() => onToggleEnabled(server)}
                  />
                </Tooltip>
              ) : null}
              <Tooltip content={t('settings.mcpViewConfig')}>
                <Button
                  size='mini'
                  type='text'
                  className='flowy-button-icon'
                  icon={<Write size='14' />}
                  aria-label={t('settings.mcpViewConfig')}
                  onClick={() => onEditServer(server)}
                />
              </Tooltip>
              <Tooltip content={t('settings.mcpDeleteServer')}>
                <Button
                  size='mini'
                  type='text'
                  className='flowy-button-icon'
                  icon={<DeleteFour size='14' fill={iconColors.danger} />}
                  aria-label={t('settings.mcpDeleteServer')}
                  onClick={() => onDeleteServer(server.mcp_server_id)}
                />
              </Tooltip>
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  );
};

export default McpServerHeader;
