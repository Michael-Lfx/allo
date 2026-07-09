/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type SpeechToTextProvider = 'openai' | 'deepgram' | 'claw';

export type OpenAISpeechToTextConfig = {
  api_key: string;
  base_url?: string;
  language?: string;
  model: string;
  prompt?: string;
  temperature?: number;
};

export type DeepgramSpeechToTextConfig = {
  api_key: string;
  base_url?: string;
  detectLanguage?: boolean;
  language?: string;
  model: string;
  punctuate?: boolean;
  smartFormat?: boolean;
};

export type SpeechToTextConfig = {
  autoSend?: boolean;
  enabled: boolean;
  provider: SpeechToTextProvider;
  deepgram?: DeepgramSpeechToTextConfig;
  openai?: OpenAISpeechToTextConfig;
};

export type SpeechToTextRequest = {
  blob: Blob;
  languageHint?: string;
};

export type SpeechToTextResult = {
  language?: string;
  model: string;
  provider: SpeechToTextProvider;
  text: string;
};

const getSpeechToTextAudioExtension = (mimeType: string) => {
  switch (mimeType) {
    case 'audio/mp4':
    case 'audio/x-m4a':
      return 'm4a';
    case 'audio/mpeg':
      return 'mp3';
    case 'audio/ogg':
    case 'audio/ogg;codecs=opus':
      return 'ogg';
    case 'audio/wav':
    case 'audio/wave':
      return 'wav';
    default:
      return 'webm';
  }
};

export const createSpeechToTextFileName = (mimeType: string) => {
  return `speech-input.${getSpeechToTextAudioExtension(mimeType)}`;
};

/** Build the multipart body expected by `POST /api/stt`. */
export const buildSpeechToTextFormData = (blob: Blob, languageHint?: string): FormData => {
  const mimeType = blob.type || 'audio/webm';
  const fileName = createSpeechToTextFileName(mimeType);
  const formData = new FormData();
  formData.append('file', blob, fileName);
  formData.append('fileName', fileName);
  formData.append('mimeType', mimeType);
  if (languageHint) {
    formData.append('languageHint', languageHint);
  }
  return formData;
};
