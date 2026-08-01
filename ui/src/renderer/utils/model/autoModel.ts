/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Sentinel model id used as a fallback default. When the client sends
 * `model = 'auto'`, the LLM backend routes to a concrete model itself, so the
 * client does not need to pick one. The id is a plain string everywhere a model
 * id flows (nomi/cloud provider selection and ACP model switching), so the
 * sentinel is type-compatible with no schema change.
 */
export const AUTO_MODEL_ID = 'auto';

/**
 * Display label for the auto option in model selectors. Kept as a fixed string
 * (the label utilities are pure, without an i18n hook); localize later if needed.
 */
export const AUTO_MODEL_LABEL = 'Auto';

export const isAutoModel = (id?: string | null): boolean => Boolean(id) && id === AUTO_MODEL_ID;
