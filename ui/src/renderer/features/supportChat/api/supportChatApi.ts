/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { cloudIm } from '@/common/adapter/ipcBridge';
import type {
  ICloudImAttachmentPayload,
  ICloudImConversation,
  ICloudImLogUploadResponse,
  ICloudImMessage,
  ICloudImMessageList,
  ISupportLogsPackResponse,
} from '@/common/adapter/ipcBridge';
import { SUPPORT_CHAT_APP } from './supportChatTypes';

export const supportChatApi = {
  getConversation(app: typeof SUPPORT_CHAT_APP = SUPPORT_CHAT_APP): Promise<ICloudImConversation> {
    return cloudIm.getConversation.invoke({ app });
  },

  listMessages(params: {
    afterSeq?: number;
    beforeSeq?: number;
    limit?: number;
  } = {}): Promise<ICloudImMessageList> {
    return cloudIm.listMessages.invoke(params);
  },

  sendMessage(params: {
    clientMsgId: string;
    content: string;
    msgType?: 'text' | 'image';
    payload?: ICloudImAttachmentPayload;
    logPayload?: ICloudImAttachmentPayload;
  }): Promise<ICloudImMessage> {
    return cloudIm.sendMessage.invoke({
      clientMsgId: params.clientMsgId,
      content: params.content,
      msgType: params.msgType ?? 'text',
      app: SUPPORT_CHAT_APP,
      ...(params.payload ? { payload: params.payload } : {}),
      ...(params.logPayload ? { logPayload: params.logPayload } : {}),
    });
  },

  uploadScreenshot(params: {
    file: Blob;
    fileName: string;
  }): Promise<ICloudImLogUploadResponse> {
    return cloudIm.uploadScreenshot.invoke(params);
  },

  packLogs(): Promise<ISupportLogsPackResponse> {
    return cloudIm.packSupportLogs.invoke();
  },

  uploadLogFromPath(params: {
    zipPath: string;
    fileName?: string;
  }): Promise<ICloudImLogUploadResponse> {
    return cloudIm.uploadLogFromPath.invoke(params);
  },

  markRead(lastReadSeq: number): Promise<ICloudImConversation> {
    return cloudIm.markRead.invoke({ lastReadSeq });
  },
};
