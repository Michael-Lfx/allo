/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** Install/run lifecycle state of a local model. Mirrors the Rust enum. */
export type LocalModelRunStatus = 'uninstalled' | 'downloading' | 'installed' | 'starting' | 'running';

/**
 * What the install pipeline is doing while a model downloads. Mirrors the Rust
 * enum. `verifying_downloading` means a finished file is hashing in the
 * background while the next file downloads.
 */
export type LocalModelDownloadPhase =
  | 'connecting'
  | 'downloading'
  | 'verifying'
  | 'verifying_downloading';

/** Stable machine-readable code for a failed download. Mirrors the Rust enum. */
export type LocalModelDownloadErrorCode =
  | 'timeout'
  | 'http'
  | 'network'
  | 'checksum'
  | 'disk'
  | 'unknown';

/** Why the last download attempt failed. */
export interface LocalModelDownloadError {
  code: LocalModelDownloadErrorCode;
  /** Optional detail: HTTP status, file name, transport message. */
  detail?: string;
}

export interface LocalModelText {
  zh: string;
  en: string;
}

/**
 * Full launch options accepted by the start endpoint. Mirrors
 * `crates/backend/nomifun-api-types/src/local_models.rs`.
 */
export interface LocalModelLaunchOptions {
  context_size?: number;
  cache_type_k?: string;
  cache_type_v?: string;
  temperature?: number;
  top_p?: number;
  top_k?: number;
  repeat_penalty?: number;
  min_p?: number;
  parallel?: number;
  batch_size?: number;
  ubatch_size?: number;
  gpu_layers?: number;
  gpu_layers_auto?: boolean;
  threads?: number;
  threads_batch?: number;
  cache_ram?: number;
  no_mmap?: boolean;
  cont_batching?: boolean;
  no_kv_offload?: boolean;
  enable_dflash?: boolean;
  dflash_gpu_layers?: number;
  dflash_draft_min?: number;
  dflash_draft_max?: number;
  mtp_speculative_decoding?: boolean;
  mtp_draft_max?: number;
  ngram_speculative_decoding?: boolean;
  spec_type?: string;
  spec_ngram_size_n?: number;
  draft_min?: number;
  draft_max?: number;
}

/** Which model-store partition a catalog entry belongs to. */
export type LocalModelSource = 'builtin' | 'houmo';

/** One catalog entry plus its live install/run state. */
export interface LocalModelStatus {
  id: string;
  /** Absent on older payloads, which are the builtin llama.cpp models. */
  source?: LocalModelSource;
  display_name: LocalModelText;
  description: LocalModelText;
  type: string;
  capabilities: string[];
  tags: string[];
  scenarios: string[];
  parameter_count: number;
  size_bytes: number;
  engines: string[];
  npu: boolean;
  realtime: boolean;
  author: string;
  license: string;
  icon?: string;
  status: LocalModelRunStatus;
  download_progress: number;
  download_speed: number;
  download_phase?: LocalModelDownloadPhase;
  download_error?: LocalModelDownloadError;
  port?: number;
  base_url?: string;
  install_path?: string;
  main_gguf: string;
  mmproj?: string;
  dflash?: string;
  supports_mtp: boolean;
  supports_dflash: boolean;
  context_size: number;
  max_tokens: number;
  last_launch_options?: LocalModelLaunchOptions;
}

export interface LocalModelsResponse {
  models: LocalModelStatus[];
  /** Set when the Houmo catalog could not be loaded. */
  houmo_error?: string;
}

export interface LocalModelResponse {
  model: LocalModelStatus;
}
