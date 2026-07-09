/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { useCloudAuth } from '@/renderer/hooks/context/CloudAuthContext';

/**
 * Whether claw cloud ASR (category=7) should be offered in the composer.
 * Requires an authenticated cloud session; actual transcription is handled by `/api/stt`.
 */
export function useClawAsrAvailable(): { ready: boolean; available: boolean } {
  const { ready, status } = useCloudAuth();

  return {
    ready,
    available: ready && status === 'authenticated',
  };
}
