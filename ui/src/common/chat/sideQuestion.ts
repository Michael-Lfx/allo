

import type { TChatConversation } from '@/common/config/storage';

type SideQuestionConversationType = TChatConversation['type'];

export type SideQuestionEligibilityTarget = {
  backend?: string;
  type: SideQuestionConversationType;
};

export function isSideQuestionSupported(target: SideQuestionEligibilityTarget): boolean {
  return target.type === 'acp' && target.backend === 'claude';
}
