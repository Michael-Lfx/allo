import { describe, expect, test } from 'bun:test';
import { BackendHttpError } from '@/common/adapter/httpBridge';
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
    const pending = new Map<string, SupportPendingMessage>();
    const sent: Array<{ id: string; content: string; type: string }> = [];
    const events: string[] = [];
    const deps = createDependencies({
      addPending: (message) => pending.set(message.clientMsgId, message),
      uploadLogFromPath: async () => {
        events.push('log-upload');
        return upload('report.zip', 'https://cdn/report.zip');
      },
      uploadScreenshot: async () => {
        events.push('screenshot-upload');
        return upload('screen.png', 'https://cdn/screen.png');
      },
      send: async (id, content, options) => {
        events.push(`send-${options.msgType}`);
        sent.push({ id, content, type: options.msgType });
      },
    });

    const result = await submitConversationErrorReport(
      context,
      { description: '  复现步骤  ', screenshots: [screenshot] },
      deps
    );

    expect(result).toEqual({ status: 'success' });
    expect(pending.size).toBe(2);
    expect(pending.get('report-1')?.content).toBe('复现步骤');
    expect(pending.get('report-2')?.previewUrl).toBe('blob:screen');
    expect(sent.map((item) => item.type)).toEqual(['text', 'image']);
    expect(sent.map((item) => item.id)).toEqual(['report-1', 'report-2']);
    expect(events).toEqual(['log-upload', 'send-text', 'screenshot-upload', 'send-image']);
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

  test('does not upload screenshots when log preparation fails', async () => {
    let uploadScreenshotCalls = 0;
    const deps = createDependencies({
      uploadLogFromPath: async () => {
        throw new Error('log upload failed');
      },
      uploadScreenshot: async () => {
        uploadScreenshotCalls += 1;
        return upload('screen.png', 'https://cdn/screen.png');
      },
    });

    const result = await submitConversationErrorReport(context, { description: 'report', screenshots: [screenshot] }, deps);

    expect(result).toEqual({ status: 'preparation-failed' });
    expect(uploadScreenshotCalls).toBe(0);
  });

  test('keeps a failed screenshot as a retryable pending item without a remote payload', async () => {
    const pending = new Map<string, SupportPendingMessage>();
    const failed: string[] = [];
    const sent: string[] = [];
    const deps = createDependencies({
      addPending: (message) => pending.set(message.clientMsgId, message),
      markPendingFailed: (id) => failed.push(id),
      uploadScreenshot: async () => {
        throw new Error('screenshot upload failed');
      },
      send: async (id, _content, options) => {
        sent.push(`${id}:${options.msgType}`);
      },
    });

    const result = await submitConversationErrorReport(context, { description: 'report', screenshots: [screenshot] }, deps);

    expect(result).toEqual({ status: 'partial-failure' });
    expect(sent).toEqual(['report-1:text']);
    expect(failed).toEqual(['report-2']);
    expect(pending.get('report-2')?.file).toBe(screenshot.file);
    expect(pending.get('report-2')?.payload).toBeUndefined();
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
    const pending = new Map<string, SupportPendingMessage>();
    const failed: string[] = [];
    const sent: string[] = [];
    const deps = createDependencies({
      addPending: (message) => pending.set(message.clientMsgId, message),
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
    expect(pending.size).toBe(3);
    expect(sent).toEqual(['report-1:text', 'report-2:image']);
    expect(failed).toEqual(['report-3']);
  });

  test('does not report auth expiry after the operation becomes stale', async () => {
    let active = true;
    let authExpiredCalls = 0;
    const deps = createDependencies({
      isCurrent: () => active,
      uploadLogFromPath: async () => {
        active = false;
        throw new BackendHttpError({
          method: 'POST',
          path: '/api/support/logs',
          status: 401,
          body: { code: 'UNAUTHORIZED', error: 'authentication required' },
        });
      },
      onAuthExpired: () => {
        authExpiredCalls += 1;
      },
    });

    const result = await submitConversationErrorReport(context, { description: 'stale auth', screenshots: [] }, deps);

    expect(result).toEqual({ status: 'preparation-failed' });
    expect(authExpiredCalls).toBe(0);
  });

  test('does not report success when the final send becomes stale', async () => {
    let active = true;
    const deps = createDependencies({
      isCurrent: () => active,
      send: async () => {
        active = false;
      },
    });

    const result = await submitConversationErrorReport(context, { description: 'stale send', screenshots: [] }, deps);

    expect(result).toEqual({ status: 'preparation-failed' });
  });
});
