import type { McpServerId } from '@/common/types/ids';
import type { IMcpServer } from '@/common/config/storage';
import { Button, Dropdown, Menu, Tag, Tooltip } from '@arco-design/web-react';
import { Check, CloseSmall, Code, DeleteFour, Info, Key, Link, Login, More, Server, Shop, Terminal, Write } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import type { McpOAuthStatus } from '@/renderer/hooks/mcp/useMcpOAuth';
import type { McpActivationItemState } from '@/renderer/hooks/mcp/useMcpActivationFlow';
import FeedbackButton from '@/renderer/components/base/FeedbackButton';
import { iconColors } from '@/renderer/styles/colors';
import { MCP_SERVER_TITLE_CLASS } from './mcpServerCollapse';
import { getSafeMcpEndpoint } from './McpServerDetails';
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

const getEffectiveLoading = (
  isTestingConnection: boolean,
  activationState?: McpActivationItemState,
  oauthStatus?: McpOAuthStatus
) =>
  isTestingConnection || activationState === 'queued' || activationState === 'testing' || Boolean(oauthStatus?.isChecking);

const getStatusIcon = (
  server: IMcpServer,
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
  if (server.last_test_status === 'connected' || activationState === 'enabled' || oauthStatus?.isAuthenticated) {
    return <Check size={STATUS_ICON_SIZE} fill={iconColors.success} />;
  }
  return <Info theme='outline' size={STATUS_ICON_SIZE} fill={iconColors.secondary} />;
};

const formatStatusTimestamp = (timestamp?: number): string | null => {
  if (!timestamp) return null;
  return new Date(timestamp).toLocaleString();
};

const getStatusText = (
  server: IMcpServer,
  activationState: McpActivationItemState | undefined,
  oauthStatus: McpOAuthStatus | undefined,
  isLoading: boolean,
  t: (key: string, options?: Record<string, unknown>) => string
) => {
  if (activationState === 'queued') return t('settings.mcpActivationPreparing');
  if (oauthStatus?.isChecking) return t('settings.mcpOAuthChecking');
  if (activationState === 'testing' || isLoading) return t('settings.mcpActivationChecking');
  if (activationState === 'config-changed') return t('settings.mcpConfigChangedDuringTest');
  if (activationState === 'needs-auth' || oauthStatus?.needsLogin) return t('settings.mcpNeedsLogin');
  if (activationState === 'failed' || server.last_test_status === 'error') return t('settings.mcpCheckFailedSimple');
  if (server.enabled) return t('settings.mcpEnabledAndSelectable');
  if (server.last_test_status === 'connected') return t('settings.mcpCheckPassedSimple');
  return t('settings.mcpDisconnected');
};

const MCP_MARKET_ORIGIN_KEY = '_nomifun_market';

type McpSourceKind = 'builtin' | 'market' | 'local';

const getSourceKind = (server: IMcpServer): McpSourceKind => {
  if (server.builtin) return 'builtin';
  try {
    const parsed = JSON.parse(server.original_json) as Record<string, unknown>;
    const market = parsed[MCP_MARKET_ORIGIN_KEY];
    if (market && typeof market === 'object' && !Array.isArray(market)) {
      const itemId = (market as Record<string, unknown>).item_id;
      if (typeof itemId === 'string' && itemId.trim()) return 'market';
    }
  } catch {
    // A malformed original JSON should not prevent the installed card from rendering.
  }
  return 'local';
};

const getSourceLabel = (sourceKind: McpSourceKind, t: (key: string) => string) => {
  if (sourceKind === 'builtin') return t('settings.mcpBuiltinSource');
  if (sourceKind === 'market') return t('settings.mcpMarketSource');
  return t('settings.mcpLocalSource');
};

const getTransportIcon = (transport: IMcpServer['transport']) => {
  if (transport.type === 'stdio') return <Terminal size='13' fill='currentColor' />;
  return <Link size='13' fill='currentColor' />;
};

const getSourceIcon = (sourceKind: McpSourceKind) => {
  if (sourceKind === 'builtin') return <Server size='13' fill='currentColor' />;
  if (sourceKind === 'market') return <Shop size='13' fill='currentColor' />;
  return <Code size='13' fill='currentColor' />;
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

  if (activationState === 'queued' || activationState === 'testing' || isTestingConnection || isCheckingOAuth) {
    return (
      <Button
        size='mini'
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

  if (server.enabled) {
    return (
      <Button size='mini' className='flowy-icon-text-btn' onClick={() => onTestConnection(server)}>
        {t('settings.mcpRetestConnection')}
      </Button>
    );
  }

  if (activationState === 'enabled' || server.last_test_status === 'connected') {
    return (
      <Button
        size='mini'
        type='primary'
        className='flowy-icon-text-btn'
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
      type={canRetry ? 'secondary' : 'primary'}
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
  const isLoading = getEffectiveLoading(isTestingConnection, activationState, oauthStatus);
  const statusText = getStatusText(server, activationState, oauthStatus, isLoading, t);
  const statusIcon = getStatusIcon(server, oauthStatus, isLoading, activationState);
  const checkedAt = formatStatusTimestamp(server.last_connected || server.updated_at);
  const sourceKind = getSourceKind(server);
  const sourceLabel = getSourceLabel(sourceKind, t);
  const isEnabledForDisplay = activationState === 'enabled' || server.enabled;
  const stateTagLabel = isLoading
    ? t(
        activationState === 'queued'
          ? 'settings.mcpActivationPreparing'
          : oauthStatus?.isChecking
            ? 'settings.mcpOAuthChecking'
            : 'settings.mcpActivationChecking'
      )
    : t(isEnabledForDisplay ? 'settings.mcpEnabled' : 'settings.mcpDisabled');
  const isError =
    activationState === 'failed' || activationState === 'config-changed' || server.last_test_status === 'error';
  const needsAuth = activationState === 'needs-auth' || Boolean(oauthStatus?.needsLogin);
  const isFocusedActivation = activationState === 'queued' || activationState === 'testing';
  const statusDescription = activationError || (isError ? t('settings.mcpInlineConfigHint') : statusText);

  return (
    <div className='grid w-full min-w-0 grid-cols-[minmax(0,1fr)_auto] items-start gap-x-16px gap-y-12px max-[900px]:grid-cols-1'>
      <div className='flex min-w-0 items-start gap-10px'>
        <Tooltip content={statusDescription} position='top'>
          <span className={`${STATUS_ICON_SLOT_CLASS} mt-2px cursor-default`} aria-label={statusText}>
            {statusIcon}
          </span>
        </Tooltip>
        <div className='min-w-0 flex-1'>
          <div className='flex min-w-0 flex-wrap items-center gap-x-8px gap-y-6px'>
            <span className={`${MCP_SERVER_TITLE_CLASS} truncate text-14px font-medium text-t-primary`}>{server.name}</span>
            <span className='inline-flex h-22px shrink-0 items-center gap-4px rd-6px bg-fill-2 px-6px text-11px text-t-secondary'>
              {getTransportIcon(server.transport)}
              {server.transport.type}
            </span>
            <span className='inline-flex h-22px shrink-0 items-center gap-4px text-11px text-t-tertiary'>
              {getSourceIcon(sourceKind)}
              {sourceLabel}
            </span>
            <Tag size='small' bordered={false} color={isEnabledForDisplay && !isLoading ? 'green' : 'gray'} className='!flex-shrink-0 !text-11px'>
              {stateTagLabel}
            </Tag>
            {isError && !isFocusedActivation ? <FeedbackButton /> : null}
          </div>
          <div className='mt-8px flex min-w-0 items-center gap-6px font-mono text-12px text-t-tertiary' title={getSafeMcpEndpoint(server)}>
            <Code size='13' fill='currentColor' className='shrink-0' aria-hidden='true' />
            <span className='truncate'>{getSafeMcpEndpoint(server)}</span>
          </div>
          <div
            className='mt-8px flex flex-wrap items-center gap-x-10px gap-y-4px text-12px text-t-secondary'
            role={isLoading ? 'status' : undefined}
            aria-live={isLoading ? 'polite' : undefined}
          >
            <span>{statusText}</span>
            <span className='text-t-tertiary' aria-hidden='true'>
              ·
            </span>
            <span>{t('settings.mcpDiscoveredTools', { count: server.tools?.length ?? 0 })}</span>
            {checkedAt ? (
              <span className='text-t-tertiary'>{`${t('settings.mcpCheckedAtLabel')} ${checkedAt}`}</span>
            ) : null}
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
              <Tooltip content={t('settings.mcpViewConfig')}>
                <Button
                  size='mini'
                  className='flowy-button-icon'
                  icon={<Write size='14' />}
                  aria-label={t('settings.mcpViewConfig')}
                  onClick={() => onEditServer(server)}
                />
              </Tooltip>
              <Dropdown
                trigger='click'
                droplist={
                  <Menu>
                    {server.enabled ? (
                      <Menu.Item key='disable' onClick={() => onToggleEnabled(server)}>
                        {t('settings.mcpDisable')}
                      </Menu.Item>
                    ) : null}
                    <Menu.Item key='delete' onClick={() => onDeleteServer(server.mcp_server_id)}>
                      <div className='flex items-center gap-2 text-red-500'>
                        <DeleteFour size='14' />
                        {t('settings.mcpDeleteServer')}
                      </div>
                    </Menu.Item>
                  </Menu>
                }
              >
                <Button size='mini' className='flowy-button-icon' icon={<More size='14' />} aria-label={t('common.more')} />
              </Dropdown>
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  );
};

export default McpServerHeader;
