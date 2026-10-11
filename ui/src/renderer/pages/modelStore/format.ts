/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { LocalModelDownloadError, LocalModelDownloadPhase } from '@common/types/localModel';
import type { LocalizedText, ModelStoreCatalogEntry } from './types';

/** Pick the locale-appropriate half of a `{ zh, en }` pair. */
export function pickLocalized(text: LocalizedText, language: string): string {
  const isChinese = language.toLowerCase().startsWith('zh');
  const preferred = isChinese ? text.zh : text.en;
  return (preferred || text.en || text.zh || '').trim();
}

/**
 * Collapse manifest prose into a single readable line. Variant manifests often
 * prefix the description with a machine id (`DeepSeek-R1-...-lmhead — ONNX AWQ
 * INT4 量化…`), repeat the model name as an ASCII clause before the first comma,
 * and separate bullets with newlines; all three read as noise on a card.
 */
const LEADING_MACHINE_ID = /^([\w.]+(?:[-_][\w.]+)*)\s*(?:[—–]+\s*|-{1,2}\s+)/;

const hasCjk = (value: string): boolean => /[\u4e00-\u9fff]/.test(value);

function isMachineId(id: string): boolean {
  return id.length > 10 && /\d/.test(id) && /[-_]/.test(id);
}

/**
 * Chinese variants frequently open with an ASCII-only clause that just restates
 * the model id and quantization (`DeepSeek R1 Distill Qwen 1.5B INT4 CW
 * OpenVINO NPU，OpenVINO IR 格式…`). Drop that clause so the visible text starts
 * at the first meaningful sentence. English descriptions keep their clause.
 */
function stripLeadingAsciiClause(value: string): string {
  if (!hasCjk(value)) return value;
  const match = /^([^，,]+)[，,]\s*/.exec(value);
  if (!match) return value;
  const head = match[1];
  if (!hasCjk(head) && /\d/.test(head) && head.length >= 12) {
    return value.slice(match[0].length).trim();
  }
  return value;
}

export function cleanDescription(text: string): string {
  let value = (text || '').replace(/\s+/g, ' ').trim();
  if (!value) return '';
  const prefix = LEADING_MACHINE_ID.exec(value);
  if (prefix && isMachineId(prefix[1])) {
    value = value.slice(prefix[0].length).trim();
  }
  return stripLeadingAsciiClause(value);
}

/** Human-readable download footprint (GB with one decimal, else MB). */
export function formatSize(bytes: number): string {
  if (!bytes || bytes <= 0) return '—';
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(gb >= 10 ? 0 : 1)} GB`;
  const mb = bytes / 1024 ** 2;
  if (mb >= 1) return `${mb.toFixed(0)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

/** Human-readable download speed (bytes/second). */
export function formatSpeed(bytesPerSecond: number): string {
  if (!bytesPerSecond || bytesPerSecond <= 0) return '—';
  const mb = bytesPerSecond / 1024 ** 2;
  if (mb >= 1) return `${mb.toFixed(1)} MB/s`;
  const kb = bytesPerSecond / 1024;
  return `${Math.max(1, Math.round(kb))} KB/s`;
}

/** Everything a downloading card needs to state its current action. */
export interface DownloadPhaseView {
  phase: LocalModelDownloadPhase;
  /** i18n key for the one-line "what is happening" label. */
  labelKey: string;
  /** Percent is meaningful once we are past connecting. */
  showPercent: boolean;
  /** Speed only shows while bytes are moving. */
  showSpeed: boolean;
}

const PHASE_LABEL_KEYS: Record<LocalModelDownloadPhase, string> = {
  connecting: 'modelStore.card.phase.connecting',
  downloading: 'modelStore.card.phase.downloading',
  verifying: 'modelStore.card.phase.verifying',
  verifying_downloading: 'modelStore.card.phase.verifying_downloading',
};

/**
 * Resolve the sentence a downloading card shows. The backend's explicit
 * `downloadPhase` wins; without it we fall back to "connecting" until the first
 * byte lands. Speed only shows while downloading, so a pure verifying phase
 * keeps the percent but drops the (zero) speed. An overlap phase names both
 * actions (`verifying_downloading`) and still shows the speed.
 */
export function resolveDownloadPhase(model: {
  downloadPhase?: LocalModelDownloadPhase;
  downloadProgress: number;
  downloadSpeed: number;
}): DownloadPhaseView {
  const phase = model.downloadPhase ?? (model.downloadProgress <= 0 ? 'connecting' : 'downloading');
  return {
    phase,
    labelKey: PHASE_LABEL_KEYS[phase],
    showPercent: phase !== 'connecting',
    showSpeed:
      (phase === 'downloading' || phase === 'verifying_downloading') && model.downloadSpeed > 0,
  };
}

/** Two-character avatar text derived from the model name. */
export function getAvatarText(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return '?';
  const cjk = trimmed.match(/[\u4e00-\u9fff]/g);
  if (cjk && cjk.length) return cjk.slice(0, 2).join('');
  const words = trimmed.split(/[\s._-]+/).filter(Boolean);
  if (words.length >= 2) return `${words[0][0] ?? ''}${words[1][0] ?? ''}`.toUpperCase();
  return trimmed.slice(0, 2).toUpperCase();
}

/** Stable hue so the same model always gets the same avatar tint. */
export function getAvatarHue(id: string): number {
  let hash = 0;
  for (let index = 0; index < id.length; index += 1) {
    hash = (hash * 31 + id.charCodeAt(index)) % 360;
  }
  return hash;
}

/** Capability/scenario chips shown on a card (capabilities win over tags). */
export function getCapabilityTags(model: ModelStoreCatalogEntry, limit = 3): string[] {
  const source = model.capabilities.length ? model.capabilities : model.scenarios.length ? model.scenarios : model.tags;
  const seen = new Set<string>();
  const result: string[] = [];
  for (const raw of source) {
    const value = raw.trim();
    if (!value || seen.has(value.toLowerCase())) continue;
    seen.add(value.toLowerCase());
    result.push(value);
    if (result.length >= limit) break;
  }
  return result;
}

/** Case-insensitive match over localized name, description and tags. */
export function matchesSearch(model: ModelStoreCatalogEntry, query: string, language: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  const haystack = [
    model.displayName.zh,
    model.displayName.en,
    model.description.zh,
    model.description.en,
    ...model.tags,
    ...model.scenarios,
    ...model.capabilities,
  ]
    .join(' ')
    .toLowerCase();
  return haystack.includes(needle);
}

/** Minimal translator shape the error formatter needs. */
type TranslateFn = (key: string, options?: Record<string, unknown>) => string;

/** Localized sentence for a structured download failure. */
export function formatDownloadError(t: TranslateFn, error: LocalModelDownloadError): string {
  switch (error.code) {
    case 'timeout':
      return t('modelStore.downloadError.timeout');
    case 'http':
      return t('modelStore.downloadError.http', { status: error.detail ?? '' });
    case 'network':
      return t('modelStore.downloadError.network');
    case 'checksum':
      return t('modelStore.downloadError.checksum', { file: error.detail ?? '' });
    case 'disk':
      return t('modelStore.downloadError.disk');
    default:
      return t('modelStore.downloadError.unknown');
  }
}

/** Identity of an error so the same failure is not surfaced twice. */
export function downloadErrorKey(error: LocalModelDownloadError): string {
  return `${error.code}:${error.detail ?? ''}`;
}

/** A model whose download error should be surfaced to the user once. */
export interface DownloadErrorNotice {
  id: string;
  error: LocalModelDownloadError;
}

/**
 * Diff the current models against the already-notified errors, mutating `seen`
 * in place. A model with no error forgets its previous notice so a later retry
 * that fails again is shown anew.
 */
export function diffDownloadErrors(
  seen: Map<string, string>,
  models: { id: string; downloadError?: LocalModelDownloadError }[]
): DownloadErrorNotice[] {
  const notices: DownloadErrorNotice[] = [];
  for (const model of models) {
    if (!model.downloadError) {
      seen.delete(model.id);
      continue;
    }
    const key = downloadErrorKey(model.downloadError);
    if (seen.get(model.id) === key) continue;
    seen.set(model.id, key);
    notices.push({ id: model.id, error: model.downloadError });
  }
  return notices;
}
