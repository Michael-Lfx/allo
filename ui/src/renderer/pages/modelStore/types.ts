/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type {
  LocalModelDownloadError,
  LocalModelDownloadPhase,
  LocalModelLaunchOptions,
  LocalModelRunStatus,
} from '@common/types/localModel';

/** Localized text pair used across the model catalog. */
export interface LocalizedText {
  zh: string;
  en: string;
}

/**
 * A single catalog entry. This is the static directory metadata bundled with
 * the store; live install/run state is merged in from `GET /api/local-models`.
 */
export interface ModelStoreCatalogEntry {
  /** Stable catalog id. */
  id: string;
  displayName: LocalizedText;
  description: LocalizedText;
  /** Manifest `type`, e.g. `multimodal`. */
  type: string;
  capabilities: string[];
  tags: string[];
  scenarios: string[];
  /** Declared parameter count in billions. */
  parameterCount: number;
  /** Total download footprint in bytes. */
  sizeBytes: number;
  /** Public path to the vendor logo, e.g. `/model_logo/qwen.png`. */
  icon?: string;
  engines: string[];
  npu: boolean;
  realtime: boolean;
  author?: string;
  license?: string;
}

/** A catalog entry merged with its live runtime state. */
export interface ModelStoreModel extends ModelStoreCatalogEntry {
  /** `houmo` entries come from the remote Houmo catalog. */
  source: 'builtin' | 'houmo';
  status: LocalModelRunStatus;
  downloadProgress: number;
  downloadSpeed: number;
  downloadPhase?: LocalModelDownloadPhase;
  downloadError?: LocalModelDownloadError;
  port?: number;
  baseUrl?: string;
  installPath?: string;
  mainGguf: string;
  mmproj?: string;
  dflash?: string;
  supportsMtp: boolean;
  supportsDflash: boolean;
  contextSize: number;
  maxTokens: number;
  lastLaunchOptions?: LocalModelLaunchOptions;
}

/** Install/runtime status filter on the store toolbar. */
export type ModelStoreStatus = 'all' | 'installed' | 'running';
