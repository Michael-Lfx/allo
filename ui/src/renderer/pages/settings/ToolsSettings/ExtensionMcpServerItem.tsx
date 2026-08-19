import { Collapse, Tooltip } from '@arco-design/web-react';
import { Info } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import type { ExtensionMcpServerContribution } from '@/renderer/hooks/mcp/extensionCatalog';
import { iconColors } from '@/renderer/styles/colors';
import { MCP_SERVER_COLLAPSE_CLASS, MCP_SERVER_TITLE_CLASS } from './mcpServerCollapse';

interface ExtensionMcpServerItemProps {
  server: ExtensionMcpServerContribution;
  isCollapsed: boolean;
  onToggleCollapse: () => void;
}

/** Read-only presentation for an extension contribution, with no canonical MCP actions. */
const ExtensionMcpServerItem: React.FC<ExtensionMcpServerItemProps> = ({
  server,
  isCollapsed,
  onToggleCollapse,
}) => {
  const { t } = useTranslation();
  const hasDescription = Boolean(server.description);

  return (
    <Collapse
      activeKey={hasDescription && isCollapsed ? ['1'] : []}
      onChange={hasDescription ? onToggleCollapse : undefined}
      className={MCP_SERVER_COLLAPSE_CLASS}
    >
      <Collapse.Item
        header={
          <div className='flex min-w-0 items-center gap-8px'>
            <span className={MCP_SERVER_TITLE_CLASS}>{server.name}</span>
            <Tooltip content={t('settings.mcpDisconnected')} position='top'>
              <span className='inline-flex h-24px w-24px shrink-0 items-center justify-center cursor-default'>
                <Info theme='outline' size={16} fill={iconColors.secondary} />
              </span>
            </Tooltip>
          </div>
        }
        name='1'
        disabled={!hasDescription}
        showExpandIcon={hasDescription}
        className='[&_div.arco-collapse-item-content-box]:py-3'
      >
        {hasDescription ? (
          <div className='text-13px leading-20px text-t-secondary whitespace-pre-wrap break-words'>
            {server.description}
          </div>
        ) : null}
      </Collapse.Item>
    </Collapse>
  );
};

export default ExtensionMcpServerItem;
