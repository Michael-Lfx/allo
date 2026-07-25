

import type { TChatConversation } from '@/common/config/storage';

export const isConversationPinned = (conversation: TChatConversation): boolean => {
  return conversation.pinned === true;
};
