import { describe, expect, test } from 'bun:test';
import type { ICloudImLogUploadResponse } from '@/common/adapter/ipcBridge';
import type { SupportPendingMessage } from './api/supportChatTypes';
import type {
  ConversationErrorReportContext,
  ConversationErrorReportDraft,
} from './conversationErrorReport';
import type { ConversationErrorReportSubmissionDependencies } from './conversationErrorReportSubmission';
import { submitConversationErrorReport } from './conversationErrorReportSubmission';

const context: ConversationErrorReportContext = {
  error: {
    message: 'Provider unavailable',
    code: 'PROVIDER_UNAVAILABLE',
    retryable: true,
  },
  conversationId: 'conversation-1',
  messageId: 'message-1',
  turnId: 'turn-1',
  occurredAt: '2026-08-30T10:00:00.000Z',
};

const screenshot: ConversationErrorReportDraft['screenshots'][number] = {
  file: new File(['screen'], 'screen.png', { type: 'image/png' }),
  fileName: 'screen.png',
  previewUrl: 'blob:screen',
};

const upload = (name: string, url: string): ICloudImLogUploadResponse => ({
  ossId: 1,
  name,
  url,
  contentType: name.endsWith('.zip') ? 'application/zip' : 'image/png',
  byteSize: 16,
});

function createDependencies(
  overrides: Partial<ConversationErrorReportSubmissionDependencies> = {}
): ConversationErrorReportSubmissionDependencies {
  return {
    isCurrent: () => true,
    packLogs: async () => ({
      zipPath: 'C:/logs/report.zip',
      fileName: 'report.zip',
      byteSize: 16,
      includedFiles: [],
      truncated: false,
    }),
    collectDevice: async () => ({ collectedAt: '2026-08-30T10:00:00.000Z' }),
    uploadScreenshot: async () => upload('screen.png', 'https://cdn/screen.png'),
    uploadLogFromPath: async () => upload('report.zip', 'https://cdn/report.zip'),
    account: { collectedAt: '2026-08-30T10:00:00.000Z' },
    addPending: () => undefined,
    markPendingFailed: () => undefined,
    send: async () => undefined,
    onAuthExpired: () => undefined,
    defaultContent: '默认反馈内容',
    now: () => Date.parse('2026-08-30T10:00:00.000Z'),
    createClientMsgId: (() => {
      let index = 0;
      return () => `report-${++index}`;
    })(),
    ...overrides,
  };
}

describe('submitConversationErrorReport', () => {
  test('prepares one text report followed by screenshots in order', async () => {
    const pending: SupportPendingMessage[] = [];
    const sent: Array<{ id: string; content: string; type: string }> = [];
    const deps = createDependencies({
      addPending: (message) => pending.push(message),
      send: async (id, content, options) => {
        sent.push({ id, content, type: options.msgType });
      },
    });

    const result = await submitConversationErrorReport(
      context,
      { description: '  复现步骤  ', screenshots: [screenshot] },
      deps
    );

    expect(result).toEqual({ status: 'success' });
    expect(pending).toHaveLength(2);
    expect(pending[0]?.content).toBe('复现步骤');
    expect(pending[1]?.previewUrl).toBe('blob:screen');
    expect(sent.map((item) => item.type)).toEqual(['text', 'image']);
    expect(sent.map((item) => item.id)).toEqual(['report-1', 'report-2']);
  });

  test('keeps the report unsubmitted when preparation fails', async () => {
    const pending: SupportPendingMessage[] = [];
    const sent: string[] = [];
    const deps = createDependencies({
      packLogs: async () => {
        throw new Error('pack failed');
      },
      addPending: (message) => pending.push(message),
      send: async (id) => {
        sent.push(id);
      },
    });

    const result = await submitConversationErrorReport(context, { description: '', screenshots: [] }, deps);

    expect(result).toEqual({ status: 'preparation-failed' });
    expect(pending).toHaveLength(0);
    expect(sent).toHaveLength(0);
  });

  test('stops after the auth operation becomes stale during preparation', async () => {
    let active = true;
    let resolvePack: ((value: Awaited<ReturnType<ConversationErrorReportSubmissionDependencies['packLogs']>>) => void) | undefined;
    const pending: SupportPendingMessage[] = [];
    let uploadLogCalls = 0;
    const deps = createDependencies({
      isCurrent: () => active,
      packLogs: () =>
        new Promise((resolve) => {
          resolvePack = resolve;
        }),
      uploadLogFromPath: async () => {
        uploadLogCalls += 1;
        return upload('report.zip', 'https://cdn/report.zip');
      },
      addPending: (message) => pending.push(message),
    });
    const submission = submitConversationErrorReport(context, { description: 'stale', screenshots: [] }, deps);

    active = false;
    resolvePack?.({
      zipPath: 'C:/logs/report.zip',
      fileName: 'report.zip',
      byteSize: 16,
      includedFiles: [],
      truncated: false,
    });

    const result = await submission;
    expect(result).toEqual({ status: 'preparation-failed' });
    expect(uploadLogCalls).toBe(0);
    expect(pending).toHaveLength(0);
  });

  test('marks only the unsent tail failed after a partial send', async () => {
    const pending: SupportPendingMessage[] = [];
    const failed: string[] = [];
    const sent: string[] = [];
    const deps = createDependencies({
      addPending: (message) => pending.push(message),
      markPendingFailed: (id) => failed.push(id),
      send: async (id, _content, options) => {
        sent.push(`${id}:${options.msgType}`);
        if (options.msgType === 'image') throw new Error('image send failed');
      },
    });

    const result = await submitConversationErrorReport(
      context,
      { description: 'partial', screenshots: [screenshot, { ...screenshot, fileName: 'second.png' }] },
      deps
    );

    expect(result).toEqual({ status: 'partial-failure' });
    expect(pending).toHaveLength(3);
    expect(sent).toEqual(['report-1:text', 'report-2:image']);
    expect(failed).toEqual(['report-3']);
  });
});
