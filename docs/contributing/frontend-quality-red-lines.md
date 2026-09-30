# Frontend Quality Red Lines (Lessons Learned)

> **Scope**: Quality constraints, historical retrospectives, and automated guards for Flowy's frontend surfaces (`ui/` and `web/`).  
> **Golden Rule**: **"Zero mock contamination in production, full i18n symmetry without fallback leaks, dual-theme compatibility with complete CSS."**

---

## 1. Red Line 1: Zero Mock Contamination in Production Runtime

### 1.1 The Issue
Hardcoding mock balances, fake accounts, or test states into production contexts (e.g. `BillingContext.tsx`, `CreditsContext.tsx`, `AuthContext.tsx`) or hooks to preview UI states pollutes user accounts and leaks into release builds.

### 1.2 The Rule
- Never hardcode mock balances, fake account states, bypass tokens, or test defaults into production Contexts (`*Context.tsx`), hooks, or runtime state.
- UI previews must use isolated sandbox files, test pages (Test Harness), or temporary HTML artifacts, never in-tree production fallbacks.

### 1.3 The Boundary
Changes to billing, credits, and auth contexts must be kept in minimal, dedicated PRs rather than mixed into general UI styling or feature PRs.

---

## 2. Red Line 2: Full i18n Coverage & Zero Fallback Leakage

### 2.1 The Issue
Using `t('key', { defaultValue: '中文' })` while omitting the key from `en-US.json` (or `zh-CN.json`). In the omitted locale, i18next silently falls back to `defaultValue`, leaking untranslated Chinese to English users.

### 2.2 The Rule
- Every single user-visible string — buttons, pills, tooltips, popovers, badges, aria-labels, titles, and error toasts — must be declared symmetrically in both `zh-CN` and `en-US` locale dictionaries (`ui/src/renderer/services/i18n/locales/`).
- Never rely on default values as a substitute for symmetric dictionary entries.

### 2.3 Verification
1. Run `bun run gen:i18n` to update `i18n-keys.d.ts`;
2. Run `bun run check:i18n` to verify dictionary completeness;
3. Add dual-locale assertions in accompanying unit tests.

---

## 3. Red Line 3: Dual-Theme (Light & Dark) Compatibility & CSS Completeness

### 3.1 The Issue
Hardcoding absolute colors (e.g. `#fff`, `#000`) causes invisible text or poor contrast when switching themes. Incomplete UnoCSS border utilities (e.g. `border-t` without `border-t-solid`) fail to render borders and violate dead-css rules.

### 3.2 The Rule
- Every UI component must adapt cleanly to both Light and Dark modes. Always use semantic design tokens (`text-t-primary`, `text-t-secondary`, `bg-fill-1`, `var(--border-base)`, `var(--flowy-attention)`).
- Directional border width classes like `border-t` or `border-b` must be accompanied by explicit style classes (e.g. `border-t-solid`).
- Creative visuals and new colors must be expressed as theme-aware values (define a new theme token, or use a `var()` whose fallback stays legible in both modes), never as a hardcoded value tuned for one theme. A hardcoded color that keeps its contrast in both modes (e.g. white text on a saturated accent button) is fine.

### 3.3 Verification
1. Visually verify both modes by toggling the theme switcher;
2. Run `bun run check:theme`;
3. Ensure `bun run check` and `bun run check:dead-css` pass without warnings.
