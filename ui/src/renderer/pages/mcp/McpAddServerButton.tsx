import { getAgents } from '@/renderer/hooks/agent/useAgents';
import { Button, Dropdown, Menu } from '@arco-design/web-react';
import { Down, Plus } from '@icon-park/react';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

type McpAddServerButtonProps = {
  onOpen: (mode: 'json' | 'oneclick') => void;
};

const McpAddServerButton: React.FC<McpAddServerButtonProps> = ({ onOpen }) => {
  const { t } = useTranslation();
  const [detectedAgents, setDetectedAgents] = useState<Array<{ backend: string; name: string }>>([]);

  useEffect(() => {
    void getAgents()
      .then((agents) => {
        setDetectedAgents(agents.map((agent) => ({ backend: agent.backend ?? '', name: agent.name })));
      })
      .catch((error: unknown) => {
        console.error('Failed to load agents:', error);
      });
  }, []);

  if (detectedAgents.length === 0) {
    return (
      <Button
        size='small'
        type='outline'
        className='flowy-icon-text-btn capability-hub-action-btn'
        icon={<Plus size={'16'} fill='currentColor' />}
        data-testid='btn-add-mcp'
        onClick={() => onOpen('json')}
      >
        {t('settings.mcpAddServer')}
      </Button>
    );
  }

  return (
    <Dropdown
      trigger='click'
      droplist={
        <Menu>
          <Menu.Item
            key='json'
            onClick={(event) => {
              event.stopPropagation();
              onOpen('json');
            }}
          >
            {t('settings.mcpImportFromJSON')}
          </Menu.Item>
          <Menu.Item
            key='oneclick'
            onClick={(event) => {
              event.stopPropagation();
              onOpen('oneclick');
            }}
          >
            {t('settings.mcpOneKeyImport')}
          </Menu.Item>
        </Menu>
      }
    >
      <Button
        size='small'
        type='outline'
        className='flowy-icon-text-btn capability-hub-action-btn'
        icon={<Plus size={'16'} fill='currentColor' />}
        data-testid='btn-add-mcp'
        onClick={(event) => event.stopPropagation()}
      >
        {t('settings.mcpAddServer')} <Down size='12' fill='currentColor' />
      </Button>
    </Dropdown>
  );
};

export default McpAddServerButton;
