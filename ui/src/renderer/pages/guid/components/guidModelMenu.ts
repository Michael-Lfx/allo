/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { compositeKey } from '@/common/utils/compositeKey';

export type ChatModelMenuGroup<T extends { id: string }> = {
  provider: T;
  models: string[];
};

export const findChatModelForMenuKey = <T extends { id: string }>(
  groups: readonly ChatModelMenuGroup<T>[],
  key: string,
): { provider: T; modelName: string } | undefined => {
  for (const group of groups) {
    for (const modelName of group.models) {
      if (compositeKey(group.provider.id, modelName) === key) {
        return { provider: group.provider, modelName };
      }
    }
  }
  return undefined;
};
