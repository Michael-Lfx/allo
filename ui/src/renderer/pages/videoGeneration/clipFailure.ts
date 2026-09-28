/**
 * User-facing copy for 「即刻出片」 clip-task failures.
 *
 * Persistence keeps the backend/provider string (often Chinese canonical or an
 * English API dump). Call this at render so the card follows the current UI
 * language, matching canvas `formatCanvasUserError` and the agent-workshop
 * `classifyFailure` path.
 */

import type { TFunction } from 'i18next';
import { formatCanvasUserError } from '@oc/lib/canvas/canvas-user-error';
import { isInsufficientCreditsError } from './creditsError';
import {
  resolveVideoFailureRecoveryAction,
  type VideoFailureRecoveryAction,
} from './videoFailureRecovery';

export type ClipFailureView = {
  title: string;
  message: string;
  credits: boolean;
  recovery: VideoFailureRecoveryAction | null;
};

export function describeClipFailure(
  raw: string | null | undefined,
  t: TFunction
): ClipFailureView {
  const text = raw?.trim() ?? '';
  if (isInsufficientCreditsError(text)) {
    return {
      credits: true,
      title: t('videoGeneration.clip.failure.creditsTitle', {
        defaultValue: '积分不足',
      }),
      message: t('videoGeneration.clip.failure.creditsHint', {
        defaultValue:
          '当前积分不足以完成本次生成。点击「购买积分」充值后，请返回首页重新发起生成。',
      }),
      recovery: resolveVideoFailureRecoveryAction('credits'),
    };
  }

  const fallback = t('videoGeneration.clip.generationFailed', {
    defaultValue: '视频生成失败',
  });
  return {
    credits: false,
    title: t('videoGeneration.clip.errorTitle', { defaultValue: '生成出错' }),
    message: text ? formatCanvasUserError(text, fallback) : fallback,
    recovery: null,
  };
}

export function formatClipOperationError(
  error: unknown,
  t: TFunction,
  prefix: { key: string; defaultValue: string }
): { message: string; credits: boolean } {
  const raw = error instanceof Error ? error.message : String(error ?? '');
  if (isInsufficientCreditsError(raw)) {
    return {
      credits: true,
      message: t('videoGeneration.clip.failure.creditsToast', {
        defaultValue: '积分不足，请充值后再试。',
      }),
    };
  }
  const detail = formatCanvasUserError(raw);
  const head = t(prefix.key, { defaultValue: prefix.defaultValue });
  return { credits: false, message: `${head}: ${detail}` };
}
