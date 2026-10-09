import { describe, expect, test } from 'bun:test';
import type { IResponseMessage } from '@/common/adapter/ipcBridge';
import { parseConversationId, parseMessageId } from '@/common/types/ids';
import { petMessageFingerprint, petMessageFromResponseStream } from './conversationStream';

const CID = parseConversationId('019b0000-0000-7000-8000-0000000000aa');
const TID = parseMessageId('019b0000-0000-7000-8000-0000000000bb');
const MID = parseMessageId('019b0000-0000-7000-8000-0000000000cc');

function stream(partial: Partial<IResponseMessage> & Pick<IResponseMessage, 'type'>): IResponseMessage {
  return {
    conversation_id: CID,
    msg_id: MID,
    turn_id: TID,
    data: {},
    ...partial,
  };
}

describe('petMessageFromResponseStream', () => {
  test('ignores companion-owned and completed projections', () => {
    expect(petMessageFromResponseStream(stream({ type: 'thought', companion: true }))).toBeNull();
    expect(petMessageFromResponseStream(stream({ type: 'content', stream_complete: true }))).toBeNull();
  });

  test('maps a thought subject onto thinkingOn without dumping the chain-of-thought', () => {
    const msg = petMessageFromResponseStream(
      stream({
        type: 'thought',
        data: { subject: 'Search auth flow', description: 'long private reasoning that must stay off the desk' },
      })
    );
    expect(msg?.titleKey).toBe('nomi.petMessage.conversation.thinkingOn');
    expect(msg?.titleParams).toEqual({ topic: 'Search auth flow' });
    expect(msg?.detail).toBeUndefined();
  });

  test('keeps a generic thinking label when the subject is empty boilerplate', () => {
    const msg = petMessageFromResponseStream(stream({ type: 'thought', data: { subject: 'Thinking', description: 'secret' } }));
    expect(msg?.titleKey).toBe('nomi.petMessage.conversation.thinking');
    expect(msg?.titleParams).toBeUndefined();
  });

  test('names the tool and target while preparing and executing', () => {
    const preparing = petMessageFromResponseStream(
      stream({
        type: 'tool_preparing',
        data: { call_id: 'c1', name: 'Write', preview: { file_path: 'src/auth.ts' } },
      })
    );
    expect(preparing?.titleKey).toBe('nomi.petMessage.conversation.callingOn');
    expect(preparing?.titleParams).toEqual({ tool: 'Write', target: 'src/auth.ts' });

    const executing = petMessageFromResponseStream(
      stream({
        type: 'tool_group',
        data: [{ status: 'Executing', name: 'Grep', description: { pattern: 'turn.started' } }],
      })
    );
    expect(executing?.titleKey).toBe('nomi.petMessage.conversation.callingOn');
    expect(executing?.titleParams).toEqual({ tool: 'Grep', target: 'turn.started' });
  });

  test('surfaces a confirmation with the action being asked', () => {
    const msg = petMessageFromResponseStream(
      stream({
        type: 'tool_group',
        data: [{ status: 'Confirming', name: 'Edit', description: { file_path: 'src/App.tsx' } }],
      })
    );
    expect(msg?.phase).toBe('waiting');
    expect(msg?.titleKey).toBe('nomi.petMessage.conversation.waitingOn');
    expect(msg?.titleParams?.action).toBe('Edit · src/App.tsx');
  });

  test('does not let an idle tool_group clobber a live chip', () => {
    expect(
      petMessageFromResponseStream(
        stream({ type: 'tool_group', data: [{ status: 'Success', name: 'Write' }] })
      )
    ).toBeNull();
  });

  test('maps write frames to streaming without putting reply text on the desk', () => {
    const msg = petMessageFromResponseStream(stream({ type: 'content', data: 'secret user answer' }));
    expect(msg?.titleKey).toBe('nomi.petMessage.conversation.streaming');
    expect(msg?.detail).toBeUndefined();
  });

  test('collapses identical content fingerprints so token chunks do not retrigger', () => {
    const first = petMessageFromResponseStream(stream({ type: 'content', data: 'a' }));
    const second = petMessageFromResponseStream(stream({ type: 'content', data: 'ab' }));
    expect(first).toBeTruthy();
    expect(second).toBeTruthy();
    expect(petMessageFingerprint(first!)).toBe(petMessageFingerprint(second!));
  });
});
