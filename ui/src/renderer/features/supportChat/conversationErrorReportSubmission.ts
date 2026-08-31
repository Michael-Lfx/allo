/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type {
  ICloudImAttachmentPayload,
  ICloudImLogUploadResponse,
  IPackSupportLogsRequest,
  ISupportLogsPackResponse,
} from '@/common/adapter/ipcBridge';
import { isAuthExpiredHttpError } from '@/common/adapter/httpBridge';
import type { SupportLogDeviceInfo } from './collectSupportDeviceInfo';
import type { SupportLogUserInfo } from './collectSupportLogUserInfo';
import {
  buildConversationErrorReportMetadata,
  type ConversationErrorReportContext,
  type ConversationErrorReportDraft,
  type ConversationErrorReportSubmitResult,
} from './conversationErrorReport';
import { buildSupportImagePayload } from './supportImageAttachments';
import type { SupportPendingMessage } from './api/supportChatTypes';
import { createPendingMessage } from './state/supportMessageMerge';

type ReportSendOptions = {
  msgType: 'text' | 'image';
  payload?: ICloudImAttachmentPayload;
  logPayload?: ICloudImAttachmentPayload;
  shouldContinue: () => boolean;
};

export type ConversationErrorReportSubmissionDependencies = {
  isCurrent: () => boolean;
  packLogs: (params: IPackSupportLogsRequest) => Promise<ISupportLogsPackResponse>;
  collectDevice: () => Promise<SupportLogDeviceInfo>;
  uploadScreenshot: (params: { file: Blob; fileName: string }) => Promise<ICloudImLogUploadResponse>;
  uploadLogFromPath: (params: { zipPath: string; fileName: string }) => Promise<ICloudImLogUploadResponse>;
  account: SupportLogUserInfo;
  addPending: (message: SupportPendingMessage) => void;
  markPendingFailed: (clientMsgId: string) => void;
  send: (clientMsgId: string, content: string, options: ReportSendOptions) => Promise<void>;
  onAuthExpired: (error: unknown) => void;
  defaultContent: string;
  now?: () => number;
  createClientMsgId?: () => string;
};

function clientMsgId(deps: ConversationErrorReportSubmissionDependencies): string {
  return deps.createClientMsgId?.() ?? crypto.randomUUID();
}

function createReportPendingMessages(
  content: string,
  logPayload: ICloudImAttachmentPayload,
  screenshots: ConversationErrorReportDraft['screenshots'],
  createdAt: number,
  deps: ConversationErrorReportSubmissionDependencies
): Array<{
  clientMsgId: string;
  content: string;
  createdAt: string;
  msgType: 'text' | 'image';
  logPayload?: ICloudImAttachmentPayload;
  screenshot?: ConversationErrorReportDraft['screenshots'][number];
}> {
  const reportEntry = {
    clientMsgId: clientMsgId(deps),
    content,
    createdAt: new Date(createdAt).toISOString(),
    msgType: 'text' as const,
    logPayload,
  };
  const screenshotEntries = screenshots.map((screenshot, index) => ({
    clientMsgId: clientMsgId(deps),
    content: '',
    createdAt: new Date(createdAt + index + 1).toISOString(),
    msgType: 'image' as const,
    screenshot,
  }));
  return [reportEntry, ...screenshotEntries];
}

export async function submitConversationErrorReport(
  context: ConversationErrorReportContext,
  draft: ConversationErrorReportDraft,
  deps: ConversationErrorReportSubmissionDependencies
): Promise<ConversationErrorReportSubmitResult> {
  if (!deps.isCurrent()) return { status: 'preparation-failed' };

  const content = draft.description.trim() || deps.defaultContent;

  try {
    const [packed, device] = await Promise.all([
      deps.packLogs({ turnId: context.turnId ?? context.messageId }),
      deps.collectDevice(),
    ]);
    if (!deps.isCurrent()) return { status: 'preparation-failed' };

    const uploadedLog = await deps.uploadLogFromPath({
      zipPath: packed.zipPath,
      fileName: packed.fileName,
    });
    if (!deps.isCurrent()) return { status: 'preparation-failed' };

    const logPayload: ICloudImAttachmentPayload = {
      ...(uploadedLog.url ? { url: uploadedLog.url } : {}),
      ...(uploadedLog.objectKey ? { objectKey: uploadedLog.objectKey } : {}),
      name: uploadedLog.name || packed.fileName,
      contentType: uploadedLog.contentType || 'application/zip',
      byteSize: uploadedLog.byteSize || packed.byteSize,
      account: deps.account,
      device,
      report: buildConversationErrorReportMetadata(context),
    };
    const entries = createReportPendingMessages(
      content,
      logPayload,
      draft.screenshots,
      deps.now?.() ?? Date.now(),
      deps
    );

    for (const entry of entries) {
      if (!deps.isCurrent()) return { status: 'preparation-failed' };
      deps.addPending(
        entry.msgType === 'image'
          ? createPendingMessage(entry.clientMsgId, entry.content, entry.createdAt, 'sending', undefined, {
              previewUrl: entry.screenshot?.previewUrl,
              file: entry.screenshot?.file,
              fileName: entry.screenshot?.fileName,
            })
          : createPendingMessage(entry.clientMsgId, entry.content, entry.createdAt, 'sending', entry.logPayload)
      );
    }

    const [reportEntry, ...screenshotEntries] = entries;
    if (!reportEntry || !deps.isCurrent()) return { status: 'preparation-failed' };

    try {
      await deps.send(reportEntry.clientMsgId, reportEntry.content, {
        msgType: 'text',
        logPayload: reportEntry.logPayload,
        shouldContinue: deps.isCurrent,
      });
      if (!deps.isCurrent()) return { status: 'preparation-failed' };
    } catch {
      if (!deps.isCurrent()) return { status: 'preparation-failed' };
      for (const remaining of screenshotEntries) {
        deps.markPendingFailed(remaining.clientMsgId);
      }
      return { status: 'partial-failure' };
    }

    for (let index = 0; index < screenshotEntries.length; index += 1) {
      const entry = screenshotEntries[index];
      const screenshot = entry.screenshot;
      if (!screenshot || !deps.isCurrent()) return { status: 'preparation-failed' };

      let payload: ICloudImAttachmentPayload;
      try {
        const uploaded = await deps.uploadScreenshot({
          file: screenshot.file,
          fileName: screenshot.fileName,
        });
        if (!deps.isCurrent()) return { status: 'preparation-failed' };
        payload = buildSupportImagePayload(uploaded, {
          fileName: screenshot.fileName,
          contentType: screenshot.file.type,
          byteSize: screenshot.file.size,
        });
      } catch (error) {
        if (!deps.isCurrent()) return { status: 'preparation-failed' };
        if (isAuthExpiredHttpError(error)) deps.onAuthExpired(error);
        deps.markPendingFailed(entry.clientMsgId);
        for (const remaining of screenshotEntries.slice(index + 1)) {
          deps.markPendingFailed(remaining.clientMsgId);
        }
        return { status: 'partial-failure' };
      }

      // Replace the placeholder pending item with its upload payload so a
      // send failure can retry without uploading the same file again.
      deps.addPending(
        createPendingMessage(entry.clientMsgId, entry.content, entry.createdAt, 'sending', undefined, {
          payload,
          previewUrl: screenshot.previewUrl,
          file: screenshot.file,
          fileName: screenshot.fileName,
        })
      );
      try {
        await deps.send(entry.clientMsgId, entry.content, {
          msgType: 'image',
          payload,
          shouldContinue: deps.isCurrent,
        });
        if (!deps.isCurrent()) return { status: 'preparation-failed' };
      } catch {
        if (!deps.isCurrent()) return { status: 'preparation-failed' };
        // sendWithClientMsgId owns the current item's failed state; only the
        // messages that were not attempted yet need to be marked here.
        for (const remaining of screenshotEntries.slice(index + 1)) {
          deps.markPendingFailed(remaining.clientMsgId);
        }
        return { status: 'partial-failure' };
      }
    }
    if (!deps.isCurrent()) return { status: 'preparation-failed' };
    return { status: 'success' };
  } catch (error) {
    if (isAuthExpiredHttpError(error) && deps.isCurrent()) deps.onAuthExpired(error);
    return { status: 'preparation-failed' };
  }
}
