/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect } from 'react';
import { Spin } from '@arco-design/web-react';
import type { WorkspaceTabProps } from '../../types';
import MigrationSection from './MigrationSection';
import DangerZoneSection from './DangerZoneSection';
import KnowledgeTab from '../../../tabs/KnowledgeTab';
import SecretsTab from '../../../tabs/SecretsTab';

/**
 * 其他 — the drawer for everything that is neither daily nor dangerous-by-
 * accident: the companion's knowledge bindings, its browser credentials, moving
 * it between machines, and destroying it. Nothing here fires without a confirm
 * or a native file dialog.
 *
 * The tab never raises the attention dot — nothing in it is ever "waiting".
 */
const OtherTab: React.FC<WorkspaceTabProps> = ({ companionId, companion, onAttentionChange }) => {
  const { profile } = companion;

  useEffect(() => {
    onAttentionChange?.(false);
  }, [onAttentionChange]);

  if (!profile) {
    return (
      <div className='flex justify-center py-40px'>
        <Spin />
      </div>
    );
  }

  return (
    <div className='flex flex-col gap-16px py-8px'>
      <KnowledgeTab companion={companion} />
      <SecretsTab companion={companion} />
      <MigrationSection companionId={companionId} companionName={profile.name} />
      <DangerZoneSection companionId={companionId} companionName={profile.name} />
    </div>
  );
};

export default OtherTab;
