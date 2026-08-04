

import React, { createContext, useCallback, useContext, useMemo, useState } from 'react';
import FeedbackReportModal from '@/renderer/components/settings/SettingsModal/contents/FeedbackReportModal';
import type { ConversationErrorReportContext } from '@/renderer/features/supportChat/conversationErrorReport';
import { useSupportChat } from '@/renderer/features/supportChat/SupportChatProvider';

type OpenFeedbackOptions = {
  conversationErrorReport?: ConversationErrorReportContext;
};

type FeedbackContextValue = {
  openFeedback: (options?: OpenFeedbackOptions) => Promise<void>;
};

const FeedbackContext = createContext<FeedbackContextValue | null>(null);

export const FeedbackProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
  const { reportConversationError } = useSupportChat();
  const [visible, setVisible] = useState(false);

  const openFeedback = useCallback(
    async (options?: OpenFeedbackOptions) => {
      if (options?.conversationErrorReport) {
        reportConversationError(options.conversationErrorReport);
        return;
      }
      setVisible(true);
    },
    [reportConversationError]
  );

  const handleCancel = useCallback(() => {
    setVisible(false);
  }, []);

  const value = useMemo(() => ({ openFeedback }), [openFeedback]);

  return (
    <FeedbackContext.Provider value={value}>
      {children}
      <FeedbackReportModal visible={visible} onCancel={handleCancel} />
    </FeedbackContext.Provider>
  );
};

export const useFeedback = (): FeedbackContextValue => {
  const ctx = useContext(FeedbackContext);
  if (!ctx) {
    // Fallback so consumers don't crash when the provider isn't mounted (e.g. web build).
    return {
      openFeedback: async () => {
        /* no-op */
      },
    };
  }
  return ctx;
};
