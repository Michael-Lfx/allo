import { describe, expect, test } from 'bun:test';
import type { IMessageSkillLoad, IMessageText, TMessage } from '@/common/chat/chatLib';
import { skillLoadsAttachedToUserMessage } from './userMessageSkillLoads';

const user = (id: string, text: string): IMessageText =>
  ({
    id,
    type: 'text',
    position: 'right',
    content: { content: text },
  }) as IMessageText;

const skill = (id: string, name: string): IMessageSkillLoad =>
  ({
    id,
    type: 'skill_load',
    position: 'center',
    content: {
      skill_id: `builtin:${name}`,
      name,
      source: 'builtin',
      version_hash: 'abc',
      content: '# steps',
      invocation: 'force',
    },
  }) as IMessageSkillLoad;

const assistant = (id: string): TMessage =>
  ({
    id,
    type: 'text',
    position: 'left',
    content: { content: 'ok' },
  }) as IMessageText;

describe('skillLoadsAttachedToUserMessage', () => {
  test('attaches the skill loads that follow the user message', () => {
    const sent = user('user', '梵高的医生');
    const messages = [sent, skill('a', 'art-motion'), skill('b', 'officecli'), assistant('reply')];
    expect(skillLoadsAttachedToUserMessage(messages, sent).map((item) => item.content.name)).toEqual([
      'art-motion',
      'officecli',
    ]);
  });

  test('attaches older skill rows that were stored before the user timestamp', () => {
    const sent = user('user', '梵高的医生');
    const messages = [skill('a', 'art-motion'), sent, assistant('reply')];
    expect(skillLoadsAttachedToUserMessage(messages, sent).map((item) => item.id)).toEqual(['a']);
  });

  test('does not steal a skill load that already follows an earlier user message', () => {
    const first = user('first', '先说');
    const second = user('second', '再说');
    const messages = [first, skill('a', 'art-motion'), second];
    expect(skillLoadsAttachedToUserMessage(messages, second)).toEqual([]);
    expect(skillLoadsAttachedToUserMessage(messages, first).map((item) => item.id)).toEqual(['a']);
  });

  test('leaves a model-auto load that arrives after assistant work off the user bubble', () => {
    const sent = user('user', '帮我看看');
    const messages = [sent, assistant('reply'), skill('auto', 'pdf')];
    expect(skillLoadsAttachedToUserMessage(messages, sent)).toEqual([]);
  });
});
