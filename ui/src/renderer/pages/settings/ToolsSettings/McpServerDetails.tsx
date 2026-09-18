import type { IMcpServer } from '@/common/config/storage';
import { Tag } from '@arco-design/web-react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import type { McpActivationItemState } from '@/renderer/hooks/mcp/useMcpActivationFlow';
import McpServerToolsList from './McpServerToolsList';

type McpServerDetailsProps = {
  server: IMcpServer;
  activationState?: McpActivationItemState;
  activationError?: string;
};

const McpServerDetails: React.FC<McpServerDetailsProps> = ({ server, activationState, activationError }) => {
  const { t } = useTranslation();
  const endpoint = getSafeMcpEndpoint(server);
  const isActivationPending = activationState === 'queued' || activationState === 'testing';
  const isActivationFailed =
    activationState === 'failed' || activationState === 'config-changed' || server.last_test_status === 'error';
  const isNeedsAuth = activationState === 'needs-auth';
  const isEnabled = activationState === 'enabled' || server.enabled;
  const hasChecked = server.last_test_status === 'connected' || server.last_test_status === 'error' || isActivationFailed;
  const headerKeys =
    server.transport.type === 'stdio'
      ? Object.keys(server.transport.env ?? {})
      : Object.keys(server.transport.headers ?? {});
  const detailHint = isActivationPending
    ? t('settings.mcpActivationPendingHint')
    : isNeedsAuth
      ? t('settings.mcpActivationNeedsAuthHint')
      : isActivationFailed
        ? activationError || t('settings.mcpActivationFailedHint')
        : !server.builtin && !isEnabled
          ? t('settings.mcpDisabledHint')
          : null;
  const availabilityText = isActivationPending
    ? t('settings.mcpActivationPendingHint')
    : isNeedsAuth
      ? t('settings.mcpActivationNeedsAuthHint')
      : isActivationFailed
        ? t('settings.mcpActivationFailedHint')
        : t(isEnabled ? 'settings.mcpAvailableToAgent' : 'settings.mcpUnavailableToAgent');

  return (
    <div className='space-y-16px'>
      {detailHint ? (
        <div
          className='rd-8px border border-solid border-orange-6 bg-[rgba(var(--orange-6),0.08)] px-10px py-8px text-12px leading-18px text-t-secondary'
          role={isActivationFailed || isNeedsAuth ? 'alert' : undefined}
        >
          {detailHint}
        </div>
      ) : null}
      <div className='grid grid-cols-[96px_minmax(0,1fr)] gap-x-12px gap-y-8px text-12px leading-18px'>
        <span className='text-t-tertiary'>{t('settings.mcpTransport')}</span>
        <div><Tag size='small' bordered={false}>{server.transport.type}</Tag></div>
        <span className='text-t-tertiary'>{t('settings.mcpEndpoint')}</span>
        <code className='break-all font-mono text-t-primary'>{endpoint}</code>
        {headerKeys.length > 0 ? (
          <>
            <span className='text-t-tertiary'>{t('settings.mcpConfiguredHeaders')}</span>
            <span className='break-words text-t-secondary'>{headerKeys.join(', ')}</span>
          </>
        ) : null}
      </div>
      <div className='flex items-center gap-8px text-12px leading-18px'>
        <span
          className={`h-6px w-6px rounded-full ${isEnabled ? 'bg-green-6' : isActivationFailed || isNeedsAuth ? 'bg-orange-6' : 'bg-gray-6'}`}
          aria-hidden='true'
        />
        <span className='text-t-secondary'>
          {availabilityText}
        </span>
      </div>
      <div className='border-t border-solid border-arco-2 pt-12px'>
        <div className='mb-8px text-12px font-medium text-t-primary'>
          {t('settings.mcpDiscoveredTools', { count: server.tools?.length ?? 0 })}
        </div>
        <McpServerToolsList
          server={server}
          activationState={activationState}
          loading={isActivationPending || server.last_test_status === 'testing'}
          hasChecked={hasChecked}
        />
      </div>
    </div>
  );
};

const SENSITIVE_ARGUMENT = /token|secret|password|passwd|authorization|api[_-]?key|access[_-]?key/i;

export const getSafeMcpEndpoint = (server: IMcpServer): string => {
  if (server.transport.type !== 'stdio') {
    try {
      const url = new URL(server.transport.url);
      return `${url.origin}${url.pathname}`;
    } catch {
      return server.transport.url.split(/[?#]/, 1)[0];
    }
  }

  const args = [...(server.transport.args ?? [])];
  for (let index = 0; index < args.length; index += 1) {
    const previous = args[index - 1] ?? '';
    if (SENSITIVE_ARGUMENT.test(previous) || SENSITIVE_ARGUMENT.test(args[index])) {
      args[index] = '••••';
    }
  }
  return [server.transport.command, ...args].join(' ');
};

export default McpServerDetails;
