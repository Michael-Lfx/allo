

import { speechToText } from '@/common/adapter/ipcBridge';
import { isBackendHttpError, isBackendRequestError } from '@/common/adapter/httpBridge';
import type { SpeechToTextResult } from '@/common/types/provider/speech';

const MAX_AUDIO_FILE_SIZE_MB = 30;
const MAX_AUDIO_FILE_SIZE_BYTES = MAX_AUDIO_FILE_SIZE_MB * 1024 * 1024;

const STT_CONFIG_ERROR_CODES = new Set([
  'STT_DISABLED',
  'STT_OPENAI_NOT_CONFIGURED',
  'STT_DEEPGRAM_NOT_CONFIGURED',
  'STT_CLAW_NOT_CONFIGURED',
]);

const ensureAudioSize = (blob: Blob) => {
  if (blob.size > MAX_AUDIO_FILE_SIZE_BYTES) {
    throw new Error('STT_FILE_TOO_LARGE');
  }
};

const mapSttTransportError = (error: unknown): never => {
  if (isBackendRequestError(error)) {
    throw new Error('STT_NETWORK_ERROR');
  }

  if (isBackendHttpError(error)) {
    if (error.status === 413) {
      throw new Error('STT_FILE_TOO_LARGE');
    }
    if (error.code && STT_CONFIG_ERROR_CODES.has(error.code)) {
      throw new Error(error.code);
    }
    const detail = error.backendMessage || `${error.status}`;
    throw new Error(`STT_REQUEST_FAILED:${detail}`);
  }

  throw error instanceof Error ? error : new Error(String(error));
};

export async function transcribeAudioBlob(blob: Blob, languageHint?: string): Promise<SpeechToTextResult> {
  ensureAudioSize(blob);

  try {
    return await speechToText.transcribe.invoke({ blob, languageHint });
  } catch (error) {
    return mapSttTransportError(error);
  }
}
