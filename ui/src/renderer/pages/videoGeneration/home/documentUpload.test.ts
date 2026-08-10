import { describe, expect, test } from 'bun:test';
import { strToU8, zipSync } from 'fflate';
import {
  displayFileStem,
  isSupportedImageFile,
  isSupportedTextFile,
  readUploadedTextFile,
} from './documentUpload';

describe('video home document uploads', () => {
  test('accepts supported image and text formats', () => {
    expect(
      isSupportedImageFile(new File(['x'], 'hero.webp', { type: 'image/webp' }))
    ).toBe(true);
    expect(
      isSupportedTextFile(new File(['story'], 'story.md', { type: '' }))
    ).toBe(true);
    expect(
      isSupportedTextFile(new File(['story'], 'story.docx', { type: '' }))
    ).toBe(true);
  });

  test('reads text and strips BOM/outer whitespace', async () => {
    const file = new File(['\uFEFF  第一场\n\n画面淡入  '], 'script.txt', {
      type: 'text/plain',
    });
    expect(await readUploadedTextFile(file)).toBe('第一场\n\n画面淡入');
  });

  test('extracts paragraphs from DOCX documents', async () => {
    const archive = zipSync({
      'word/document.xml': strToU8(
        '<w:document><w:body><w:p><w:r><w:t>第一场</w:t></w:r></w:p><w:p><w:r><w:t>雨夜车站</w:t></w:r></w:p></w:body></w:document>'
      ),
    });
    const file = new File([archive], 'script.docx', {
      type: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    });
    expect(await readUploadedTextFile(file)).toBe('第一场\n雨夜车站');
  });

  test('rejects unsupported and empty documents', async () => {
    let unsupported = '';
    let empty = '';
    try {
      await readUploadedTextFile(
        new File(['binary'], 'script.pdf', { type: 'application/pdf' })
      );
    } catch (error) {
      unsupported = error instanceof Error ? error.message : String(error);
    }
    try {
      await readUploadedTextFile(
        new File(['  '], 'empty.txt', { type: 'text/plain' })
      );
    } catch (error) {
      empty = error instanceof Error ? error.message : String(error);
    }
    expect(unsupported).toContain('暂不支持');
    expect(empty).toContain('内容为空');
  });

  test('creates a concise display name', () => {
    expect(displayFileStem('chapter-one.script.md')).toBe('chapter-one.script');
  });
});
