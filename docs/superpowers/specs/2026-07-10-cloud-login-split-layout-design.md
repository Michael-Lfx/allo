# Cloud Login Split Layout Design

**Date:** 2026-07-10  
**Scope:** Client `/cloud-login` page only (not local `/login`)

## Goal
Redesign the Flowy cloud email-OTP login page into a split-panel layout with a refined, non-AI-purple visual language.

## Decisions
- Layout: left brand/map panel + right form (Travel Connect inspired)
- Primary button: ink black `#0f172a` (no purple gradients)
- Motion: `framer-motion` for card/brand entrance; canvas dot-map routes on the left
- Keep existing OTP auth flow, i18n, signed-in state
- No Tailwind/shadcn; stay on existing CSS + UnoCSS stack
- Narrow screens: hide left panel, form-only card

## Files
- `ui/src/renderer/pages/cloudLogin/index.tsx`
- `ui/src/renderer/pages/cloudLogin/CloudLoginPage.css`
- `ui/src/renderer/services/i18n/locales/{zh-CN,en-US}/cloudLogin.json`
- `ui/package.json` (`framer-motion`)
