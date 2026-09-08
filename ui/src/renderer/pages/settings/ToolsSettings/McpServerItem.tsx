import { Collapse } from '@arco-design/web-react';
import React from 'react';
import type { IMcpServer } from '@/common/config/storage';
import type { McpServerId } from '@/common/types/ids';
import type { McpOAuthStatus } from '@/renderer/hooks/mcp/useMcpOAuth';
import type { McpActivationItemState } from '@/renderer/hooks/mcp/useMcpActivationFlow';
import McpServerHeader from './McpServerHeader';
import McpServerDetails from './McpServerDetails';
import { MCP_SERVER_COLLAPSE_CLASS } from './mcpServerCollapse';

interface McpServerItemProps {
  server: IMcpServer;
  isCollapsed: boolean;
  isTestingConnection: boolean;
  activationState?: McpActivationItemState;
  activationError?: string;
  oauthStatus?: McpOAuthStatus;
  isLoggingIn?: boolean;
  isTogglingEnabled?: boolean;
  /** Extension-contributed servers are read-only (no edit/delete) */
  isReadOnly?: boolean;
  onToggleCollapse: () => void;
  onTestConnection: (server: IMcpServer) => void;
  onEditServer: (server: IMcpServer) => void;
  onDeleteServer: (serverId: McpServerId) => void;
  onToggleEnabled: (server: IMcpServer) => void;
  onOAuthLogin?: (server: IMcpServer) => void;
}

const McpServerItem: React.FC<McpServerItemProps> = ({
  server,
  isCollapsed,
  isTestingConnection,
  activationState,
  activationError,
  oauthStatus,
  isLoggingIn,
  isTogglingEnabled,
  isReadOnly,
  onToggleCollapse,
  onTestConnection,
  onEditServer,
  onDeleteServer,
  onToggleEnabled,
  onOAuthLogin,
}) => {
  return (
    <div
      data-mcp-server-id={server.mcp_server_id}
      data-mcp-activation-state={activationState}
      aria-busy={activationState === 'queued' || activationState === 'testing' || isTestingConnection || oauthStatus?.isChecking || undefined}
      tabIndex={-1}
      className='mcp-server-item outline-none'
    >
      <Collapse
        key={server.mcp_server_id}
        activeKey={isCollapsed ? ['1'] : []}
        onChange={onToggleCollapse}
        className={`${MCP_SERVER_COLLAPSE_CLASS} !mb-0 overflow-hidden rd-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] transition-colors duration-180 [&_.arco-collapse-item-header]:!px-16px [&_.arco-collapse-item-header]:!py-14px [&_.arco-collapse-item-content-box]:!px-16px [&_.arco-collapse-item-content-box]:!pb-16px [&_.arco-collapse-item-content-box]:!pt-0`}
      >
        <Collapse.Item
          header={
            <McpServerHeader
              server={server}
              isTestingConnection={isTestingConnection}
              activationState={activationState}
              activationError={activationError}
              oauthStatus={oauthStatus}
              isLoggingIn={isLoggingIn}
              isTogglingEnabled={isTogglingEnabled}
              isReadOnly={isReadOnly}
              onTestConnection={onTestConnection}
              onEditServer={onEditServer}
              onDeleteServer={onDeleteServer}
              onToggleEnabled={onToggleEnabled}
              onOAuthLogin={onOAuthLogin}
            />
          }
          name='1'
          className={'[&_div.arco-collapse-item-content-box]:py-3'}
        >
          <McpServerDetails server={server} activationState={activationState} activationError={activationError} />
        </Collapse.Item>
      </Collapse>
    </div>
  );
};

export default McpServerItem;
