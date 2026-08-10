import i18n from 'i18next';
import { strFromU8, unzipSync } from 'fflate';

const TEXT_EXTENSIONS = new Set(['txt', 'md', 'markdown', 'json', 'csv', 'srt', 'vtt']);
const MAX_TEXT_FILE_BYTES = 5 * 1024 * 1024;

function uploadMessage(key: string, defaultValue: string): string {
  const translated = i18n.t(key, { defaultValue });
  return typeof translated === 'string' && translated.length > 0
    ? translated
    : defaultValue;
}

export const VIDEO_HOME_UPLOAD_ACCEPT = [
  'image/png',
  'image/jpeg',
  'image/webp',
  '.txt',
  '.md',
  '.markdown',
  '.json',
  '.csv',
  '.srt',
  '.vtt',
  '.docx',
].join(',');

export function isSupportedImageFile(file: File): boolean {
  return /^image\/(png|jpeg|webp)$/i.test(file.type);
}

export function isSupportedTextFile(file: File): boolean {
  const extension = file.name.split('.').pop()?.toLowerCase() ?? '';
  return (
    file.type.startsWith('text/') ||
    TEXT_EXTENSIONS.has(extension) ||
    extension === 'docx' ||
    file.type ===
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document'
  );
}

export async function readUploadedTextFile(file: File): Promise<string> {
  if (!isSupportedTextFile(file)) {
    throw new Error(
      uploadMessage(
        'videoGeneration.create.upload.docUnsupported',
        '暂不支持该文档格式，请上传 DOCX、TXT、Markdown、JSON、CSV、SRT 或 VTT。'
      )
    );
  }
  if (file.size > MAX_TEXT_FILE_BYTES) {
    throw new Error(
      uploadMessage(
        'videoGeneration.create.upload.docTooLarge',
        '文档不能超过 5 MB。'
      )
    );
  }
  const extension = file.name.split('.').pop()?.toLowerCase() ?? '';
  const text =
    extension === 'docx' ||
    file.type ===
      'application/vnd.openxmlformats-officedocument.wordprocessingml.document'
      ? readDocxText(new Uint8Array(await file.arrayBuffer()))
      : (await file.text()).replace(/^\uFEFF/, '').trim();
  if (!text) {
    throw new Error(
      uploadMessage('videoGeneration.create.upload.docEmpty', '文档内容为空。')
    );
  }
  return text;
}

function decodeXmlText(value: string): string {
  return value
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, '&');
}

function readDocxText(bytes: Uint8Array): string {
  let documentXml: Uint8Array | undefined;
  try {
    documentXml = unzipSync(bytes)['word/document.xml'];
  } catch {
    throw new Error(
      uploadMessage(
        'videoGeneration.create.upload.docxCorrupt',
        'DOCX 文件已损坏或无法读取。'
      )
    );
  }
  if (!documentXml) {
    throw new Error(
      uploadMessage(
        'videoGeneration.create.upload.docxNoBody',
        'DOCX 中未找到正文内容。'
      )
    );
  }
  const xml = strFromU8(documentXml);
  const paragraphs = xml.match(/<w:p(?:\s[^>]*)?>[\s\S]*?<\/w:p>/g) ?? [xml];
  return paragraphs
    .map((paragraph) =>
      decodeXmlText(
        paragraph
          .replace(/<w:tab\b[^>]*\/>/g, '\t')
          .replace(/<w:br\b[^>]*\/>/g, '\n')
          .replace(/<[^>]+>/g, '')
      )
    )
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

export function displayFileStem(fileName: string): string {
  return (
    fileName.replace(/\.[^.]+$/, '').trim().slice(0, 48) ||
    uploadMessage('videoGeneration.create.upload.untitledAsset', '未命名素材')
  );
}
