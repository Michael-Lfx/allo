/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { resolveBackendAssetUrl } from '@/renderer/utils/platform';

/** Display name for Flowy-managed canvas model channels. */
export const FLOWY_CLOUD_CHANNEL_NAME = 'Flowy Cloud';

function logoAsset(path: string): string {
  return resolveBackendAssetUrl(`/api/assets/logos/${path}`) ?? `/api/assets/logos/${path}`;
}

/**
 * Turn a Flowy catalog `icon` into a URL the canvas `<img>` can load.
 * Absolute http(s)/data/blob URLs stay as-is; protocol-relative and
 * server-relative paths are joined to the Flowy API origin.
 */
export function rewriteCatalogIconUrl(
  icon: string | undefined | null,
  serverBaseUrl?: string
): string {
  const trimmed = icon?.trim() ?? '';
  if (!trimmed) return '';
  if (/^(https?:|data:|blob:)/i.test(trimmed)) return trimmed;
  if (trimmed.startsWith('//')) return `https:${trimmed}`;
  const base = serverBaseUrl?.trim().replace(/\/+$/, '') ?? '';
  if (!base) return trimmed;
  return trimmed.startsWith('/') ? `${base}${trimmed}` : `${base}/${trimmed}`;
}

/** Local vendor logos used when the catalog `icon` is missing or fails to load. */
export function resolveModelFallbackIcon(model: string): string {
  const name = model.toLowerCase();
  if (name.includes('claude') || name.includes('anthropic')) return logoAsset('ai-major/claude.svg');
  if (
    name.includes('gemini') ||
    name.includes('google') ||
    name.includes('nano banana') ||
    name.includes('nanobanana') ||
    name.includes('imagen') ||
    name.includes('veo') ||
    name.includes('omni flash') ||
    name.includes('omni-flash')
  ) {
    return logoAsset('ai-major/gemini.svg');
  }
  if (name.includes('gpt') || name.includes('openai') || name.includes('dall-e') || name.includes('dalle')) {
    return logoAsset('ai-major/openai.svg');
  }
  if (name.includes('grok') || name.includes('xai')) return logoAsset('ai-major/xai.svg');
  if (name.includes('deepseek')) return logoAsset('ai-major/deepseek.svg');
  if (name.includes('mistral')) return logoAsset('ai-major/mistral.svg');
  if (name.includes('glm') || name.includes('chatglm') || name.includes('zhipu')) {
    return logoAsset('ai-china/zhipu.svg');
  }
  if (name.includes('qwen') || name.includes('dashscope') || /(^|[^a-z])wan([^a-z]|$)/.test(name)) {
    return logoAsset('ai-china/qwen.svg');
  }
  if (
    name.includes('seedream') ||
    name.includes('seedance') ||
    name.includes('doubao') ||
    name.includes('volc') ||
    name.includes('ark')
  ) {
    return logoAsset('ai-china/volcengine.svg');
  }
  if (name.includes('kimi') || name.includes('moonshot')) return logoAsset('ai-china/kimi.svg');
  if (name.includes('hunyuan') || name.includes('tencent')) return logoAsset('ai-china/tencent.svg');
  return '';
}

export function isMonochromeLogo(src: string): boolean {
  return src.includes('/ai-major/openai.svg') || src.includes('/ai-major/xai.svg');
}
