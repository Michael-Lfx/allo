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

type PreparedReportImage = {
  screenshot: ConversationErrorReportDraft['screenshots'][number];
  payload: ICloudImAttachmentPayload;
};

function clientMsgId(deps: ConversationErrorReportSubmissionDependencies): string {
  return deps.createClientMsgId?.() ?? crypto.randomUUID();
}

function createReportPendingMessages(
  content: string,
  logPayload: ICloudImAttachmentPayload,
  screenshots: PreparedReportImage[],
  createdAt: number,
  deps: ConversationErrorReportSubmissionDependencies
): Array<{ clientMsgId: string; content: string; createdAt: string; msgType: 'text' | 'image'; payload?: ICloudImAttachmentPayload; logPayload?: ICloudImAttachmentPayload; screenshot?: PreparedReportImage['screenshot'] }> {
  const reportEntry = {
    clientMsgId: clientMsgId(deps),
    content,
    createdAt: new Date(createdAt).toISOString(),
    msgType: 'text' as const,
    logPayload,
  };
  const screenshotEntries = screenshots.map(({ screenshot, payload }, index) => ({
    clientMsgId: clientMsgId(deps),
    content: '',
    createdAt: new Date(createdAt + index + 1).toISOString(),
    msgType: 'image' as const,
    payload,
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
    const [packed, device, screenshotUploads] = await Promise.all([
      deps.packLogs({ turnId: context.turnId ?? context.messageId }),
      deps.collectDevice(),
      Promise.all(
        draft.screenshots.map(async (screenshot): Promise<PreparedReportImage> => {
          const uploaded = await deps.uploadScreenshot({
            file: screenshot.file,
            fileName: screenshot.fileName,
          });
          return {
            screenshot,
            payload: buildSupportImagePayload(uploaded, {
              fileName: screenshot.fileName,
              contentType: screenshot.file.type,
              byteSize: screenshot.file.size,
            }),
          };
        })
      ),
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
      screenshotUploads,
      deps.now?.() ?? Date.now(),
      deps
    );

    for (const entry of entries) {
      if (!deps.isCurrent()) return { status: 'preparation-failed' };
      deps.addPending(
        entry.msgType === 'image'
          ? createPendingMessage(entry.clientMsgId, entry.content, entry.createdAt, 'sending', undefined, {
              payload: entry.payload,
              previewUrl: entry.screenshot?.previewUrl,
              file: entry.screenshot?.file,
              fileName: entry.screenshot?.fileName,
            })
          : createPendingMessage(entry.clientMsgId, entry.content, entry.createdAt, 'sending', entry.logPayload)
      );
    }

    for (let index = 0; index < entries.length; index += 1) {
      if (!deps.isCurrent()) return { status: 'preparation-failed' };
      const entry = entries[index];
      try {
        await deps.send(entry.clientMsgId, entry.content, {
          msgType: entry.msgType,
          payload: entry.msgType === 'image' ? entry.payload : undefined,
          logPayload: entry.msgType === 'text' ? entry.logPayload : undefined,
          shouldContinue: deps.isCurrent,
        });
        if (!deps.isCurrent()) return { status: 'preparation-failed' };
      } catch {
        if (!deps.isCurrent()) return { status: 'preparation-failed' };
        for (const remaining of entries.slice(index + 1)) {
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
