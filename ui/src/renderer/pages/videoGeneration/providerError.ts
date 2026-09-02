/**
 * Pull a stable Seedance/Ark error code out of wrapped pipeline text.
 * Mirrors nomifun-cloud `extract_provider_error_code`.
 */

const STATUS_LABELS = new Set([
  'failed',
  'fail',
  'error',
  'cancelled',
  'canceled',
  'expired',
  'succeeded',
  'success',
]);

const PROVIDER_CODE_RE = /\b([A-Za-z][A-Za-z0-9]{7,}\.[A-Za-z][A-Za-z0-9]+)\b/;

export function extractProviderErrorCode(blob: string | null | undefined): string | null {
  const text = blob?.trim() ?? '';
  if (!text) return null;
  const match = text.match(PROVIDER_CODE_RE);
  const token = match?.[1];
  if (!token) return null;
  const [left, right] = token.split('.');
  if (!left || !right || /^\d+$/.test(left) || /^\d+$/.test(right)) return null;
  const lower = token.toLowerCase();
  if (
    /[A-Z]/.test(left) ||
    lower.includes('sensitive') ||
    lower.includes('policy') ||
    lower.includes('privacy') ||
    lower.includes('inspection')
  ) {
    return token;
  }
  return null;
}

export function extractProviderErrorMessage(blob: string | null | undefined): string | null {
  const text = blob?.trim() ?? '';
  if (!text) return null;
  const code = extractProviderErrorCode(text);
  if (code) {
    const needle = `${code}:`;
    const idx = text.indexOf(needle);
    if (idx >= 0) {
      const line = text
        .slice(idx + needle.length)
        .split('\n')[0]
        .trim();
      if (line && !STATUS_LABELS.has(line.toLowerCase())) {
        return line.slice(0, 256);
      }
    }
  }
  const lower = text.toLowerCase();
  for (const marker of [
    'the request failed because',
    'output video may be related to copyright',
    'input text may be related to copyright',
    'may contain real person',
  ]) {
    const idx = lower.indexOf(marker);
    if (idx >= 0) {
      return text.slice(idx).split('\n')[0].trim().slice(0, 256);
    }
  }
  return null;
}

export function isCopyrightRestriction(blob: string): boolean {
  const lower = blob.toLowerCase();
  return (
    lower.includes('copyright') ||
    blob.includes('版权') ||
    (lower.includes('outputvideosensitive') && lower.includes('policyviolation'))
  );
}

export function isReferenceImageModeration(blob: string): boolean {
  const lower = blob.toLowerCase();
  return (
    lower.includes('inputimagesensitive') ||
    lower.includes('privacyinformation') ||
    lower.includes('may contain real person') ||
    (lower.includes('real person') && lower.includes('sensitive')) ||
    blob.includes('含真人')
  );
}

export function isContentPolicyRejection(blob: string): boolean {
  const lower = blob.toLowerCase();
  return (
    isCopyrightRestriction(blob) ||
    isReferenceImageModeration(blob) ||
    lower.includes('sensitivecontent') ||
    lower.includes('inputtextsensitive') ||
    lower.includes('sensitive content') ||
    lower.includes('inappropriate content') ||
    lower.includes('datainspectionfailed') ||
    lower.includes('policyviolation') ||
    blob.includes('内容安全') ||
    blob.includes('敏感内容') ||
    blob.includes('不当内容')
  );
}

export function filmTelemetryError(
  blob: string | null | undefined
): { errorCode: string | null; errorMessage: string | null } {
  const text = blob?.trim() ?? '';
  if (!text) return { errorCode: null, errorMessage: null };
  const errorCode =
    extractProviderErrorCode(text) ??
    (text.toLowerCase().includes('insufficient_credit') || text.includes('积分不足')
      ? 'insufficient_credits'
      : isCopyrightRestriction(text)
        ? 'copyright_restriction'
        : isReferenceImageModeration(text)
          ? 'input_image_privacy'
          : isContentPolicyRejection(text)
            ? 'content_policy'
            : text.toLowerCase().includes('video generation failed') ||
                text.toLowerCase().includes('video task')
              ? 'video_generation_failed'
              : null);
  const errorMessage =
    extractProviderErrorMessage(text) ??
    (STATUS_LABELS.has(text.toLowerCase()) ? null : text.slice(0, 256));
  return { errorCode, errorMessage };
}
