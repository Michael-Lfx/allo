/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { LocalModelLaunchOptions } from '@common/types/localModel';
import type { ModelStoreModel } from './types';

/** KV cache quantization options for the standard llama.cpp runtime. */
export const CACHE_TYPE_OPTIONS = [
  'f32',
  'f16',
  'bf16',
  'q8_0',
  'q5_1',
  'q5_0',
  'q4_1',
  'q4_0',
  'iq4_nl',
  'turbo3',
  'turbo4',
] as const;

const SAMPLING_DEFAULTS: Record<string, Partial<LocalModelLaunchOptions>> = {
  'Qwen3.8-27B': {
    temperature: 1.0,
    top_p: 0.95,
    top_k: 20,
    min_p: 0,
    repeat_penalty: 1.0,
  },
  'Qwen3.6-35B-A3B': {
    temperature: 0.7,
    top_p: 0.95,
    top_k: 40,
    repeat_penalty: 1.1,
  },
  'Qwen3.6-35B-A3B-MTP': {
    temperature: 0.7,
    top_p: 0.95,
    top_k: 40,
    repeat_penalty: 1.1,
    gpu_layers: 99,
    gpu_layers_auto: true,
  },
};

/** Manifest/herdsman defaults, mirroring the Rust `default_launch_options`. */
export function defaultLaunchOptions(model: ModelStoreModel): LocalModelLaunchOptions {
  return {
    context_size: model.contextSize || undefined,
    cache_type_k: 'f16',
    cache_type_v: 'f16',
    cache_ram: 8192,
    parallel: 1,
    no_mmap: true,
    cont_batching: false,
    no_kv_offload: false,
    enable_dflash: false,
    dflash_gpu_layers: 99,
    dflash_draft_min: 0,
    dflash_draft_max: 2,
    mtp_speculative_decoding: model.supportsMtp,
    mtp_draft_max: 2,
    ngram_speculative_decoding: false,
    spec_type: 'ngram-mod',
    spec_ngram_size_n: 24,
    draft_min: 48,
    draft_max: 64,
    ...SAMPLING_DEFAULTS[model.id],
  };
}

/** Initial form values: last successful launch, else defaults. */
export function initialLaunchOptions(model: ModelStoreModel): LocalModelLaunchOptions {
  return { ...defaultLaunchOptions(model), ...(model.lastLaunchOptions ?? {}) };
}
