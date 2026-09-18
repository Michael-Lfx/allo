declare const __NOMI_BUILD_ID__: string;
declare const __NOMI_CODEMIRROR_VERSIONS__: Record<string, string>;

interface ImportMetaEnv {
  readonly DEV: boolean;
  readonly PROD: boolean;
  readonly MODE: string;
  readonly BASE_URL: string;
  readonly VITE_AIRWALLEX_ENV?: string;
  readonly VITE_CANVAS_BACKEND_URL?: string;
  readonly VITE_COMMERCIAL_SLICE?: string | boolean;
  readonly VITE_POSTHOG_HOST?: string;
  readonly VITE_POSTHOG_KEY?: string;
  readonly VITE_SENTRY_DSN?: string;
  readonly VITE_TLDRAW_LICENSE_KEY?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
