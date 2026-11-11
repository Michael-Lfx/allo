import type { IMessageSkillLoad, TMessage } from '@/common/chat/chatLib';

const isSkillLoad = (message: TMessage): message is IMessageSkillLoad => message.type === 'skill_load';

const isUserText = (message: TMessage): boolean => message.type === 'text' && message.position === 'right';

/**
 * Skill snapshots that belong on one user bubble.
 *
 * A contiguous run of `skill_load` rows immediately after the user message is
 * that send. A run immediately before it belongs there only when an earlier
 * user message does not already own it (older rows were stored with an earlier
 * timestamp than the user text).
 */
export function skillLoadsAttachedToUserMessage(
  messages: TMessage[],
  userMessage: TMessage
): IMessageSkillLoad[] {
  const index = messages.findIndex((message) => message.id === userMessage.id);
  if (index < 0 || !isUserText(messages[index])) return [];

  const loads: IMessageSkillLoad[] = [];
  for (let cursor = index + 1; cursor < messages.length; cursor += 1) {
    const next = messages[cursor];
    if (!isSkillLoad(next)) break;
    loads.push(next);
  }
  for (let cursor = index - 1; cursor >= 0; cursor -= 1) {
    const previous = messages[cursor];
    if (!isSkillLoad(previous)) break;
    let owner = cursor - 1;
    while (owner >= 0 && isSkillLoad(messages[owner])) owner -= 1;
    if (owner >= 0 && isUserText(messages[owner])) break;
    loads.unshift(previous);
  }
  return loads;
}
