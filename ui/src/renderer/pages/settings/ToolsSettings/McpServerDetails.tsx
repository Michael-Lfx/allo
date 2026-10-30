import type { IMcpServer } from '@/common/config/storage';
import React from 'react';
import { useTranslation } from 'react-i18next';
import type { McpActivationItemState } from '@/renderer/hooks/mcp/useMcpActivationFlow';
import McpServerToolsList from './McpServerToolsList';

type McpServerDetailsProps = {
  server: IMcpServer;
  activationState?: McpActivationItemState;
};

const formatCheckedAt = (timestamp?: number): string | null => {
  if (!timestamp) return null;
  return new Date(timestamp).toLocaleString();
};

export const getMcpTransportLabelKey = (
  transport: IMcpServer['transport']
): 'settings.mcpTransportLocal' | 'settings.mcpTransportRemote' =>
  transport.type === 'stdio' ? 'settings.mcpTransportLocal' : 'settings.mcpTransportRemote';

const McpServerDetails: React.FC<McpServerDetailsProps> = ({ server, activationState }) => {
  const { t } = useTranslation();
  const isActivationPending = activationState === 'queued' || activationState === 'testing';
  const isActivationFailed =
    activationState === 'failed' || activationState === 'config-changed' || server.last_test_status === 'error';
  const hasChecked = server.last_test_status === 'connected' || server.last_test_status === 'error' || isActivationFailed;
  const configuredNames =
    server.transport.type === 'stdio'
      ? Object.keys(server.transport.env ?? {})
      : Object.keys(server.transport.headers ?? {});
  const checkedAt = server.last_test_status === 'connected' ? formatCheckedAt(server.last_connected) : null;

  return (
    <div className='space-y-12px text-12px leading-18px'>
      {configuredNames.length > 0 ? (
        <div className='grid grid-cols-[96px_minmax(0,1fr)] gap-x-12px'>
          <span className='text-t-tertiary'>
            {t(server.transport.type === 'stdio' ? 'settings.mcpConfiguredEnv' : 'settings.mcpConfiguredHeaders')}
          </span>
          <span className='break-words text-t-secondary'>{configuredNames.join(', ')}</span>
        </div>
      ) : null}
      {checkedAt ? <div className='text-t-tertiary'>{t('settings.mcpCheckedAt', { time: checkedAt })}</div> : null}
      <div className='border-t border-solid border-arco-2 pt-12px'>
        <div className='mb-8px font-medium text-t-primary'>
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
