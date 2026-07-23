

export const COMMERCIAL_SLICE_FLAG = 'flowy.commercialSlice.v1';

export type CommercialSliceFlagSource = 'env' | 'localStorage' | 'default';

export function readCommercialSliceEnabled(): { enabled: boolean; source: CommercialSliceFlagSource } {
  if (typeof window !== 'undefined') {
    try {
      const raw = window.localStorage.getItem(COMMERCIAL_SLICE_FLAG);
      if (raw === '1' || raw === 'true') return { enabled: true, source: 'localStorage' };
      if (raw === '0' || raw === 'false') return { enabled: false, source: 'localStorage' };
    } catch {
      // ignore
    }
  }

  const env = (import.meta as ImportMeta & { env?: Record<string, string | boolean | undefined> }).env;
  if (env?.VITE_COMMERCIAL_SLICE === '1' || env?.VITE_COMMERCIAL_SLICE === true) {
    return { enabled: true, source: 'env' };
  }

  // Default ON for the commercial vertical slice during the P4/P5 window.
  return { enabled: true, source: 'default' };
}

export function isCommercialSliceEnabled(): boolean {
  return readCommercialSliceEnabled().enabled;
}

export function setCommercialSliceEnabled(enabled: boolean): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(COMMERCIAL_SLICE_FLAG, enabled ? '1' : '0');
  } catch {
    // ignore
  }
}
