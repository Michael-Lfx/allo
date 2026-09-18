import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import type { IMcpServer } from '@/common/config/storage';
import type { McpActivationItemState } from '@/renderer/hooks/mcp/useMcpActivationFlow';
import McpLoadingIndicator from './McpLoadingIndicator';

interface McpServerToolsListProps {
  server: IMcpServer;
  activationState?: McpActivationItemState;
  loading?: boolean;
  hasChecked?: boolean;
}

const McpServerToolsList: React.FC<McpServerToolsListProps> = ({ server, activationState, loading = false, hasChecked = false }) => {
  const { t } = useTranslation();
  const isFailed =
    activationState === 'failed' || activationState === 'config-changed' || activationState === 'needs-auth' || server.last_test_status === 'error';
  const hasSuccessfulCheck = hasChecked || activationState === 'enabled' || server.last_test_status === 'connected';

  if (!server.tools || server.tools.length === 0) {
    if (loading) {
      return (
        <div className='text-12px leading-18px text-t-secondary' role='status' aria-live='polite'>
          <McpLoadingIndicator label={t('settings.mcpToolsLoading')} />
        </div>
      );
    }

    const emptyMessage = isFailed
      ? t('settings.mcpToolsUnavailable')
      : hasSuccessfulCheck
        ? t('settings.mcpToolsEmpty')
        : t('settings.mcpToolsBeforeCheck');

    return <div className='text-12px leading-18px text-t-tertiary'>{emptyMessage}</div>;
  }

  return (
    <div>
      {server.tools.map((tool, index) => (
        <div key={index} className='flex items-baseline gap-12px border-b border-b-solid border-arco-2 py-8px first:pt-0 last:border-b-0 last:pb-0'>
          <div className='w-1/3 min-w-0 flex-shrink-0'>
            <div className='break-words text-12px font-medium text-t-primary'>{tool.name}</div>
          </div>
          <div className='min-w-0 flex-1'>
            <Tooltip content={tool.description || t('settings.mcpNoDescription')}>
              <div className='line-clamp-2 cursor-pointer text-12px leading-18px text-t-secondary'>
                {tool.description || t('settings.mcpNoDescription')}
              </div>
            </Tooltip>
          </div>
        </div>
      ))}
    </div>
  );
};

export default McpServerToolsList;
