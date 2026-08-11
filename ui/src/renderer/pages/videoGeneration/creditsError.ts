/** Detect Flowy / media "insufficient credits" failures (HTTP/business 402). */
export function isInsufficientCreditsError(error: string | null | undefined): boolean {
  if (!error) return false;
  const lower = error.toLowerCase();
  if (
    lower.includes('insufficient_credits') ||
    lower.includes('insufficient credits') ||
    lower.includes('insufficient_credit') ||
    lower.includes('insufficient credit') ||
    lower.includes('credit balance is too low') ||
    error.includes('积分不足') ||
    error.includes('余额不足') ||
    /\bapi error 402\b/.test(lower)
  ) {
    return true;
  }
  // Bare 402 only when paired with credit/balance wording (avoid false positives).
  return (
    /\b402\b/.test(lower) &&
    (lower.includes('credit') || error.includes('积分') || error.includes('余额'))
  );
}
