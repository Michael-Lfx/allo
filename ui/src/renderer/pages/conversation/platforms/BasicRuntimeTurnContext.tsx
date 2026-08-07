/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ConversationTurnSurface } from '@renderer/pages/conversation/Messages/useConversationResponseMessages';
import React, { createContext, useContext } from 'react';

const BasicRuntimeTurnContext = createContext<ConversationTurnSurface | null>(null);

export const BasicRuntimeTurnProvider: React.FC<{
  value: ConversationTurnSurface;
  children: React.ReactNode;
}> = ({ value, children }) => {
  return <BasicRuntimeTurnContext.Provider value={value}>{children}</BasicRuntimeTurnContext.Provider>;
};

/** Returns the shared turn surface when inside a basic-runtime chat shell. */
export const useBasicRuntimeTurnSurface = (): ConversationTurnSurface | null => {
  return useContext(BasicRuntimeTurnContext);
};
