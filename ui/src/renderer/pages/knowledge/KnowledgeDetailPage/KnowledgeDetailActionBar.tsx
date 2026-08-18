/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Button, Dropdown, Menu } from '@arco-design/web-react';
import { LinkOne, More, Search } from '@icon-park/react';
import React from 'react';

export interface KnowledgeDetailActionBarProps {
  labels: {
    search: React.ReactNode;
    mountToSession: React.ReactNode;
    export: React.ReactNode;
    openFolder: React.ReactNode;
    delete: React.ReactNode;
    more: string;
  };
  onSearch: () => void;
  onMountToSession: () => void;
  onExport: () => void;
  onOpenFolder: () => void;
  onDelete: () => void;
}

/** Production-owned header action buttons used by the detail page and layout probe. */
const KnowledgeDetailActionBar: React.FC<KnowledgeDetailActionBarProps> = ({
  labels,
  onSearch,
  onMountToSession,
  onExport,
  onOpenFolder,
  onDelete,
}) => (
  <div className='flex items-center gap-8px flex-wrap'>
    <Button
      data-testid='knowledge-detail-action-search'
      shape='round'
      className='flowy-icon-text-btn'
      icon={<Search theme='outline' size='14' />}
      onClick={onSearch}
    >
      {labels.search}
    </Button>
    <Button
      data-testid='knowledge-detail-action-mount'
      type='primary'
      shape='round'
      className='flowy-icon-text-btn'
      icon={<LinkOne theme='outline' size='14' />}
      onClick={onMountToSession}
    >
      {labels.mountToSession}
    </Button>
    <Dropdown
      droplist={
        <Menu className='knowledge-detail-actions-menu'>
          <Menu.Item key='export' onClick={onExport}>
            {labels.export}
          </Menu.Item>
          <Menu.Item key='openFolder' onClick={onOpenFolder}>
            {labels.openFolder}
          </Menu.Item>
          <Menu.Item key='delete' className='knowledge-detail-danger-menu-item' onClick={onDelete}>
            {labels.delete}
          </Menu.Item>
        </Menu>
      }
      position='br'
    >
      <Button
        data-testid='knowledge-detail-action-more'
        shape='round'
        aria-label={labels.more}
        icon={<More theme='outline' size='14' />}
      />
    </Dropdown>
  </div>
);

export default KnowledgeDetailActionBar;
