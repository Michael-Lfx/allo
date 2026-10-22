import { describe, expect, test } from 'bun:test';
import { parseConversationId } from '@/common/types/ids';
import type { TChatConversation } from '@/common/config/storage';
import {
  dispatchNomiSend,
  resolveVoiceConversationKind,
  type ConversationVoiceKind,
  type NomiSendDispatchDependencies,
} from './nomiSendDispatch';

const voiceConvId = parseConversationId('019b0000-0000-7000-8000-000000000001');
const ordinaryConvId = parseConversationId('019b0000-0000-7000-8000-000000000002');

describe('nomi send box dispatch behavior', () => {
  test('voice conversation initial metadata failure keeps unknown, rejects ordinary queue, and sends once upon retry', async () => {
    let metadataAttempt = 0;
    const fetchMock = async (id: string): Promise<TChatConversation | null> => {
      metadataAttempt += 1;
      if (metadataAttempt === 1) {
        throw new Error('500 Internal Server Error: metadata fetch failed');
      }
      return {
        id: voiceConvId,
        name: 'Voice Session',
        type: 'nomi',
        extra: { voice: true, workspace: 'D:/workspace/voice' },
      } as unknown as TChatConversation;
    };

    let hostSendCount = 0;
    let lastHostCommandId = '';
    let lastHostMessage = '';
    const sendVoiceKeyboardMessage = async (
      convId: string,
      cmdId: string,
      msg: string
    ) => {
      hostSendCount += 1;
      lastHostCommandId = cmdId;
      lastHostMessage = msg;
    };

    let ordinaryQueueCount = 0;
    const enqueue = (_item: { input: string; files: string[]; workspace_path?: string }) => {
      ordinaryQueueCount += 1;
      return true;
    };

    let fileClearCount = 0;
    let errorToast = '';
    const voiceSendAttempts = new Map<string, string>();

    const makeDeps = (currentKind: ConversationVoiceKind): NomiSendDispatchDependencies => ({
      conversationId: voiceConvId,
      conversationVoiceKind: currentKind,
      workspacePath: 'D:/workspace/voice',
      filesToSend: [],
      message: '用户语音命令测试',
      voiceSendAttempts,
      resolveConversationKind: async (targetId) => {
        const { kind } = await resolveVoiceConversationKind({
          targetId,
          fetchConversation: fetchMock,
        });
        return kind;
      },
      sendVoiceKeyboardMessage,
      enqueue,
      clearFiles: () => {
        fileClearCount += 1;
      },
      emitFileClear: () => {},
      notifyError: (msg) => {
        errorToast = msg;
      },
      plainTextOnlyError: '语音会话仅支持纯文本消息',
      loadFailedError: '加载会话失败，请重试',
      generateId: () => 'fixed-voice-command-uuid',
    });

    // 1. Initial attempt: metadata fails
    let threwFirst = false;
    try {
      await dispatchNomiSend(makeDeps('unknown'));
    } catch (err) {
      threwFirst = true;
      expect((err as Error).message).toContain('500 Internal Server Error');
    }
    expect(threwFirst).toBe(true);
    expect(errorToast).toBe('加载会话失败，请重试');
    // Invariant: Ordinary queue MUST NOT be called during failure
    expect(ordinaryQueueCount).toBe(0);
    // Invariant: Host messages MUST NOT be called during failure
    expect(hostSendCount).toBe(0);
    expect(fileClearCount).toBe(0);

    // 2. Retry attempt: network recovers, metadata returns voice: true
    let threwSecond = false;
    try {
      await dispatchNomiSend(makeDeps('unknown'));
    } catch {
      threwSecond = true;
    }
    expect(threwSecond).toBe(false);
    // Invariant: Host messages receives EXACTLY ONE delivery
    expect(hostSendCount).toBe(1);
    expect(lastHostCommandId).toBe('fixed-voice-command-uuid');
    expect(lastHostMessage).toBe('用户语音命令测试');
    // Invariant: Ordinary queue was NEVER touched
    expect(ordinaryQueueCount).toBe(0);
  });

  test('ordinary conversation sends through local command queue and never calls voice host', async () => {
    let hostSendCount = 0;
    let queuedItems: Array<{ input: string; files: string[]; workspace_path?: string }> = [];
    let fileClearCount = 0;
    const voiceSendAttempts = new Map<string, string>();

    const fetchMock = async (_id: string): Promise<TChatConversation | null> => ({
      id: ordinaryConvId,
      name: 'Ordinary Session',
      type: 'nomi',
      extra: { voice: false, workspace: 'D:/workspace/ordinary' },
    } as unknown as TChatConversation);

    const deps: NomiSendDispatchDependencies = {
      conversationId: ordinaryConvId,
      conversationVoiceKind: 'ordinary',
      workspacePath: 'D:/workspace/ordinary',
      filesToSend: ['D:/workspace/file.txt'],
      message: '普通会话消息',
      voiceSendAttempts,
      resolveConversationKind: async (targetId) => {
        const { kind } = await resolveVoiceConversationKind({
          targetId,
          fetchConversation: fetchMock,
        });
        return kind;
      },
      sendVoiceKeyboardMessage: async () => {
        hostSendCount += 1;
      },
      enqueue: (item) => {
        queuedItems.push(item);
        return true;
      },
      clearFiles: () => {
        fileClearCount += 1;
      },
      emitFileClear: () => {},
      notifyError: () => {},
      plainTextOnlyError: '语音会话仅支持纯文本消息',
      loadFailedError: '加载会话失败，请重试',
    };

    await dispatchNomiSend(deps);

    expect(hostSendCount).toBe(0);
    expect(queuedItems).toHaveLength(1);
    expect(queuedItems[0]).toEqual({
      input: '普通会话消息',
      files: ['D:/workspace/file.txt'],
      workspace_path: 'D:/workspace/ordinary',
    });
    expect(fileClearCount).toBe(1);
  });

  test('voice conversation rejects attachments and preserves drafts without queueing or sending', async () => {
    let hostSendCount = 0;
    let ordinaryQueueCount = 0;

    const deps: NomiSendDispatchDependencies = {
      conversationId: voiceConvId,
      conversationVoiceKind: 'voice',
      workspacePath: 'D:/workspace/voice',
      filesToSend: ['attachment.png'],
      message: '测试语音附带文件',
      voiceSendAttempts: new Map(),
      resolveConversationKind: async () => 'voice',
      sendVoiceKeyboardMessage: async () => {
        hostSendCount += 1;
      },
      enqueue: () => {
        ordinaryQueueCount += 1;
        return true;
      },
      clearFiles: () => {},
      emitFileClear: () => {},
      notifyError: () => {},
      plainTextOnlyError: '语音会话仅支持纯文本消息',
      loadFailedError: '加载会话失败，请重试',
    };

    let threw = false;
    try {
      await dispatchNomiSend(deps);
    } catch (err) {
      threw = true;
      expect((err as Error).message).toBe('语音会话仅支持纯文本消息');
    }

    expect(threw).toBe(true);
    expect(hostSendCount).toBe(0);
    expect(ordinaryQueueCount).toBe(0);
  });

  test('swr cached metadata is immediately reused without additional network fetch', async () => {
    let fetchCount = 0;
    const fetchMock = async (_id: string): Promise<TChatConversation | null> => {
      fetchCount += 1;
      return null;
    };

    const cachedConversation: TChatConversation = {
      id: voiceConvId,
      name: 'Cached Voice Session',
      type: 'nomi',
      extra: { voice: true, workspace: 'D:/workspace/cached' },
    } as unknown as TChatConversation;

    const result = await resolveVoiceConversationKind({
      targetId: voiceConvId,
      fetchConversation: fetchMock,
      getCachedConversation: (id) => (id === voiceConvId ? cachedConversation : undefined),
    });

    expect(result.kind).toBe('voice');
    expect(result.workspace).toBe('D:/workspace/cached');
    expect(fetchCount).toBe(0);
  });

  test('voice send attempt retains idempotency key across transport retry', async () => {
    const attempts = new Map<string, string>();
    let generatedIds = ['idempotency-key-uuid-1', 'idempotency-key-uuid-2'];
    let attemptCount = 0;
    let usedCommandIds: string[] = [];

    const sendVoiceKeyboardMessage = async (
      _convId: string,
      cmdId: string,
      _msg: string
    ) => {
      attemptCount += 1;
      usedCommandIds.push(cmdId);
      if (attemptCount === 1) {
        throw new Error('Transport timeout during initial send');
      }
    };

    const deps: NomiSendDispatchDependencies = {
      conversationId: voiceConvId,
      conversationVoiceKind: 'voice',
      workspacePath: 'D:/workspace/voice',
      filesToSend: [],
      message: '幂等重试命令',
      voiceSendAttempts: attempts,
      resolveConversationKind: async () => 'voice',
      sendVoiceKeyboardMessage,
      enqueue: () => true,
      clearFiles: () => {},
      emitFileClear: () => {},
      notifyError: () => {},
      plainTextOnlyError: '语音会话仅支持纯文本消息',
      loadFailedError: '加载会话失败，请重试',
      generateId: () => generatedIds.shift()!,
    };

    // First attempt fails at transport layer
    let threwFirst = false;
    try {
      await dispatchNomiSend(deps);
    } catch {
      threwFirst = true;
    }
    expect(threwFirst).toBe(true);
    expect(usedCommandIds).toEqual(['idempotency-key-uuid-1']);

    // Second attempt (retry by user) reuses the same idempotency key
    let threwSecond = false;
    try {
      await dispatchNomiSend(deps);
    } catch {
      threwSecond = true;
    }
    expect(threwSecond).toBe(false);
    expect(usedCommandIds).toEqual(['idempotency-key-uuid-1', 'idempotency-key-uuid-1']);
    // Upon success, the attempt key is cleared
    expect(attempts.size).toBe(0);
  });
});
