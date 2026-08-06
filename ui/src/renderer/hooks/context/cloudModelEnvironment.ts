export type CloudModelEnvironmentStatus = 'ready' | 'degraded' | 'failed';

export interface CloudModelEnvironmentEvidence {
  /** Models returned by the authoritative task catalog resolver. */
  resolvedModelCount: number;
  /** Provider metadata only; this is diagnostic and never grants readiness. */
  cachedProviderModelCount: number;
  syncError?: Error;
  resolveError?: Error;
}

export interface CloudModelEnvironmentClassification {
  status: CloudModelEnvironmentStatus;
  error?: Error;
}

/**
 * Decide whether the authenticated app may enter the main UI.
 *
 * The task resolver is the only readiness authority. Provider rows can carry
 * stale metadata after a failed refresh, but that metadata is not sufficient
 * to make a model usable for chat.
 */
export function classifyCloudModelEnvironment(
  evidence: CloudModelEnvironmentEvidence
): CloudModelEnvironmentClassification {
  const error = evidence.syncError ?? evidence.resolveError;
  const hasResolvedModels = evidence.resolvedModelCount > 0;
  if (!hasResolvedModels) {
    return {
      status: 'failed',
      error: error ?? new Error('No usable cloud models are available'),
    };
  }

  if (error) {
    return {
      status: 'degraded',
      error: error ?? new Error('The cloud model catalog did not resolve any models'),
    };
  }

  return { status: 'ready' };
}
