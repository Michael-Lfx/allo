import defaultLogoUrl from '@/renderer/assets/logo.svg';

const configuredBrandName = import.meta.env.VITE_BRAND_NAME?.trim();
const configuredLogoUrl = import.meta.env.VITE_BRAND_LOGO_URL?.trim();
const configuredCopyrightName = import.meta.env.VITE_BRAND_COPYRIGHT_NAME?.trim();
const configuredWakeWord = import.meta.env.VITE_BRAND_WAKE_WORD?.trim();

export const BRAND_NAME = configuredBrandName || 'Flowy';
export const BRAND_LOGO_URL = configuredLogoUrl || defaultLogoUrl;
export const BRAND_COPYRIGHT_NAME = configuredCopyrightName || BRAND_NAME;
export const BRAND_WAKE_WORD = configuredWakeWord || `Hey ${BRAND_NAME}`;

export const BRAND_I18N_VARIABLES = {
  brandName: BRAND_NAME,
} as const;
