/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ICloudWhoami } from '@/common/adapter/ipcBridge';

export type SupportLogUserInfo = {
  userId?: string;
  username?: string;
  email?: string;
  plan?: string;
  planCode?: string;
  serverBaseUrl?: string;
  collectedAt: string;
};

export function collectSupportLogUserInfo(whoami: ICloudWhoami | null | undefined): SupportLogUserInfo {
  return {
    userId: whoami?.userId,
    username: whoami?.username,
    email: whoami?.email,
    plan: whoami?.plan,
    planCode: whoami?.planCode,
    serverBaseUrl: whoami?.serverBaseUrl,
    collectedAt: new Date().toISOString(),
  };
}
