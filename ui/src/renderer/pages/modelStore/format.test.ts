import { describe, expect, test } from 'bun:test';
import type { LocalModelDownloadError } from '@common/types/localModel';
import {
  cleanDescription,
  diffDownloadErrors,
  formatDownloadError,
  resolveDownloadPhase,
} from './format';

describe('model store description cleaner', () => {
  test('drops a leading machine model id before the variant blurb', () => {
    expect(
      cleanDescription('DeepSeek-R1-Distill-Llama-8B-awq-asym-uint4-g128-lmhead — ONNX AWQ INT4 量化，AMD NPU 优化。')
    ).toBe('ONNX AWQ INT4 量化，AMD NPU 优化。');
  });

  test('drops a leading ASCII-only clause that just restates the model id', () => {
    expect(
      cleanDescription('DeepSeek R1 Distill Qwen 1.5B INT4 CW OpenVINO NPU，OpenVINO IR 格式，面向 Intel NPU 推理优化。')
    ).toBe('OpenVINO IR 格式，面向 Intel NPU 推理优化。');
  });

  test('flattens newlines and repeated whitespace', () => {
    expect(cleanDescription('第一条。\n\n第二条。\n第三条。')).toBe('第一条。 第二条。 第三条。');
  });

  test('keeps a plain human description untouched', () => {
    expect(cleanDescription('DeepSeek-V4-Flash-0731 是一款 284B MoE 模型。')).toBe(
      'DeepSeek-V4-Flash-0731 是一款 284B MoE 模型。'
    );
  });

  test('returns an empty string for empty input', () => {
    expect(cleanDescription('')).toBe('');
  });
});

describe('model store download error formatting', () => {
  const t = (key: string, options?: Record<string, unknown>) =>
    options ? `${key}|${JSON.stringify(options)}` : key;

  test('maps each code onto its localized sentence', () => {
    expect(formatDownloadError(t, { code: 'timeout' })).toBe('modelStore.downloadError.timeout');
    expect(formatDownloadError(t, { code: 'network' })).toBe('modelStore.downloadError.network');
    expect(formatDownloadError(t, { code: 'disk' })).toBe('modelStore.downloadError.disk');
    expect(formatDownloadError(t, { code: 'unknown' })).toBe('modelStore.downloadError.unknown');
  });

  test('passes the HTTP status through for http failures', () => {
    expect(formatDownloadError(t, { code: 'http', detail: '403' })).toBe(
      'modelStore.downloadError.http|{"status":"403"}'
    );
  });

  test('passes the file name through for checksum failures', () => {
    expect(formatDownloadError(t, { code: 'checksum', detail: 'model.gguf' })).toBe(
      'modelStore.downloadError.checksum|{"file":"model.gguf"}'
    );
  });
});

describe('model store download phase', () => {
  test('names each action on the downloading card', () => {
    const connecting = resolveDownloadPhase({
      downloadPhase: 'connecting',
      downloadProgress: 0,
      downloadSpeed: 0,
    });
    expect(connecting.labelKey).toBe('modelStore.card.phase.connecting');
    expect(connecting.showPercent).toBe(false);
    expect(connecting.showSpeed).toBe(false);

    const downloading = resolveDownloadPhase({
      downloadPhase: 'downloading',
      downloadProgress: 12,
      downloadSpeed: 2_800_000,
    });
    expect(downloading.labelKey).toBe('modelStore.card.phase.downloading');
    expect(downloading.showPercent).toBe(true);
    expect(downloading.showSpeed).toBe(true);

    const verifying = resolveDownloadPhase({
      downloadPhase: 'verifying',
      downloadProgress: 95,
      downloadSpeed: 0,
    });
    expect(verifying.labelKey).toBe('modelStore.card.phase.verifying');
    expect(verifying.showPercent).toBe(true);
    expect(verifying.showSpeed).toBe(false);
  });

  test('names both actions and keeps speed when verify overlaps the download', () => {
    const overlap = resolveDownloadPhase({
      downloadPhase: 'verifying_downloading',
      downloadProgress: 96,
      downloadSpeed: 2_800_000,
    });
    expect(overlap.labelKey).toBe('modelStore.card.phase.verifying_downloading');
    expect(overlap.showPercent).toBe(true);
    expect(overlap.showSpeed).toBe(true);
  });

  test('falls back to connecting before the first byte when the phase is absent', () => {
    expect(resolveDownloadPhase({ downloadProgress: 0, downloadSpeed: 0 }).phase).toBe('connecting');
    expect(resolveDownloadPhase({ downloadProgress: 5, downloadSpeed: 0 }).phase).toBe('downloading');
  });
});

describe('model store download error diffing', () => {
  test('notifies a new error once and not again on the next poll', () => {
    const seen = new Map<string, string>();
    const error: LocalModelDownloadError = { code: 'timeout' };

    const first = diffDownloadErrors(seen, [{ id: 'a', downloadError: error }]);
    expect(first.map((notice) => notice.id)).toEqual(['a']);

    const second = diffDownloadErrors(seen, [{ id: 'a', downloadError: error }]);
    expect(second).toHaveLength(0);
  });

  test('a changed error is notified again', () => {
    const seen = new Map<string, string>();
    diffDownloadErrors(seen, [{ id: 'a', downloadError: { code: 'timeout' } }]);
    const changed = diffDownloadErrors(seen, [{ id: 'a', downloadError: { code: 'http', detail: '403' } }]);
    expect(changed).toHaveLength(1);
    expect(changed[0].error.code).toBe('http');
  });

  test('clearing the error forgets it so a later retry is shown', () => {
    const seen = new Map<string, string>();
    const error: LocalModelDownloadError = { code: 'network' };
    diffDownloadErrors(seen, [{ id: 'a', downloadError: error }]);
    diffDownloadErrors(seen, [{ id: 'a' }]);
    const again = diffDownloadErrors(seen, [{ id: 'a', downloadError: error }]);
    expect(again).toHaveLength(1);
  });
});
