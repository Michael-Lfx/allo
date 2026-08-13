# Renderer i18n

The renderer uses `i18next` + `react-i18next`. The source of truth lives in
`ui/src/renderer/services/i18n/`.

## Supported Languages

- `zh-CN`
- `en-US`

`DEFAULT_LANGUAGE`, normalization, fallback merging, and the supported language
type are shared from `@/common/config/i18n`.

## File Layout

```text
services/i18n/
├── index.ts
├── i18n-keys.d.ts
└── locales/
    ├── zh-CN/
    │   ├── index.ts
    │   ├── common.json
    │   ├── conversation.json
    │   └── ...
    └── en-US/
        ├── index.ts
        ├── common.json
        ├── conversation.json
        └── ...
```

Locale JSON is split by module. Each locale folder exports its modules through
`locales/<lang>/index.ts`, and `services/i18n/index.ts` statically imports both
locale bundles so the packaged desktop app can switch languages without runtime
file discovery.

`i18n-keys.d.ts` is generated from locale files and exports `I18nKey` /
`I18nModule` for typed call sites.

## Runtime Flow

1. `i18n` initializes synchronously. First paint uses `localStorage.i18nextLng`
   (returning users) or the desktop-injected host OS locale
   (`window.__osLocale` from `sys_locale`), then English. It never reads
   `navigator.language`.
2. On first backend start, if the installation has never stored `language`,
   the host OS locale is normalized (`zh*` → `zh-CN`, everything else →
   `en-US`) and written to `installation-preferences.json`.
3. `configService.whenReady()` loads that authoritative language from the
   backend. A successful snapshot after login/reload re-applies it.
4. `ensureAndSwitch()` loads/merges the locale and calls i18next.
5. `changeLanguage()` writes the normalized language through `configService`,
   syncs `localStorage`, and notifies the host through
   `ipcBridge.systemSettings.changeLanguage`. After that, the saved preference
   wins over the OS locale.
6. Other renderer surfaces receive language changes through
   `ipcBridge.systemSettings.languageChanged`.

Do not use `i18next-browser-languagedetector`: desktop WebView and WebUI run on
different origins, so browser-origin storage is not the source of truth. Default
UI language is the **host OS locale**, not the browser language.

## Usage

```tsx
import { useTranslation } from 'react-i18next';

export function SaveButton() {
  const { t } = useTranslation();
  return <button>{t('common.save')}</button>;
}
```

For language switching, use the shared helper:

```ts
import { changeLanguage, supportedLanguages } from '@/renderer/services/i18n';

await changeLanguage('en-US');
```

## Adding Or Changing Text

1. Add the key to the matching module JSON in both `locales/zh-CN/` and
   `locales/en-US/`.
2. Keep module names aligned across languages.
3. Regenerate/check key types:

   ```bash
   bun run gen:i18n
   bun run check:i18n
   ```

4. Use the generated key at call sites.

## Rules

- Do not hardcode user-visible product text in components.
- Prefer stable semantic keys such as `cron.detail.runNow`.
- Keep Chinese and English keys symmetric.
- Add a new module only when the feature boundary is real; otherwise extend the
  nearest existing module.
- Run `bun run check:i18n` before submitting locale changes.
