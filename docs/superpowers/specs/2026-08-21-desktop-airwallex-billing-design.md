# Desktop Airwallex Billing Design

**Date:** 2026-08-21  
**Scope:** Native Tauri billing for personal plans and credit packs, paid with Airwallex (空中云汇) drop-in card fields. Hidden client route only.

## Goal

Let a logged-in desktop user buy a USD personal plan or credit pack inside Flowy, without opening the official website. Card data never touches the local backend. The billing UI lives on a dedicated SPA route that is **not** in the sider, settings nav, or any other chrome.

## Decisions

- Integration: native in-app checkout (not website WebView, not WeChat QR).
- Payment channel: `airwallex` only.
- Catalog currency: `USD` (`GET /plans` and packs with `currency=USD` and `externalChannelCode` from `server.channel`, default `flowy`).
- Card UI: `@airwallex/components-sdk` drop-in `cardNumber` / `expiry` / `cvc` (same as website `/pay-creditcard`), default env `prod`.
- Hosted Airwallex redirect (`redirectToCheckout`) is out of scope.
- JWT stays in Rust. Renderer calls `/api/cloud/*` only.

## Hidden route

Register one HashRouter path: **`/billing`**.

- Inside `ProtectedLayout` (local session required).
- If cloud session is missing, send the user to `/cloud-login` and return to `/billing` after success.
- **Do not** add this path to `settingsNavigation`, sider capability rail, titlebar, or footer.
- **Do not** treat `#/billing` as a public product URL (no marketing copy, no “Plans” menu).
- Direct navigation to `#/billing` still works; that is enough for the `+` button and in-app `navigate`.
- Wizard steps (catalog → confirm → pay) are **in-page state**, not extra routes, so the address bar stays `#/billing`.

Entry: the credits `+` button (`CreditsWebsiteButton`) `navigate('/billing')` instead of `openExternalUrl`. Keep `GET /api/cloud/website-entry` for other uses if needed; it is not the billing entry.

Layout: keep the normal app shell (sider visible) so the user can leave. Nothing in the sider is selected while on `/billing`.

## Cloud API (via local proxy)

All paths below are Flowy Cloud `/claw` paths. Local axum exposes them under `/api/cloud/...` and attaches the stored JWT.

| Step | Cloud | Notes |
| --- | --- | --- |
| List plans | `GET /plans?currency=USD&externalChannelCode=…` | Same catalog query as the international website |
| List packs | `GET /creditPacks/available?currency=USD` | Filter by current subscription |
| Coupons | `GET /promo/coupons?status=UNUSED` | Optional; skip if unused list is empty |
| Channels | `GET /paymentChannels?itemType=plan\|pack&itemId=…` | Expect `airwallex`; fail closed if missing |
| Create order | `POST /orders` | `payChannel: "airwallex"`, UUID `idempotencyKey`, optional `couponId` |
| Init pay | `POST /orders/:orderNo/pay/airwallex/init` | Only if create response has no `paymentIntentId` / `clientSecret` |
| Poll | `GET /orders/byOrderNo?orderNo=…` | Every 2.5s until `PAID`, `FAILED`, `CLOSED`, or expiry |

Create-order body:

```json
{
  "itemType": "plan",
  "itemId": 1,
  "payChannel": "airwallex",
  "idempotencyKey": "<uuid>"
}
```

Reuse the same idempotency key for retries of the same checkout attempt. Generate a new key only when the user starts a new attempt (different item or explicit retry after close).

After `PAID`, refresh `CreditsContext` (existing `/api/media/credits`).

## UI flow

1. Catalog: personal plans with month / half-year / year toggle; credit packs; mark `isCurrent` plans as non-purchasable.
2. Confirm: name, USD amount, optional coupon, estimated total.
3. Pay: mount Airwallex fields; `confirm({ intent_id, client_secret })`; show pending while polling.
4. Success: show paid state, then user can leave `/billing`. Failure: message + retry (new idempotency key).

Visual language: existing allo theme / Arco / UnoCSS. Do not copy the marketing website gradient.

## Architecture

```
Credits +  →  HashRouter /billing
                →  GET /api/cloud/plans | credit-packs | coupons
                →  POST /api/cloud/orders  (Rust + JWT → Cloud)
                →  Airwallex iframe confirm (renderer only)
                →  GET /api/cloud/orders/by-order-no  (poll)
                →  refresh credits
```

PCI: PAN/CVV never appear in `nomifun-cloud` or renderer fetch bodies.

## Out of scope

- WeChat Native QR
- Team plans / seat expand
- Wrangler plans
- Invoices, invite codes, redeem codes
- Airwallex hosted checkout in the system browser
- Exposing `/billing` in any nav surface

## Risks

- WebView2 may block Airwallex iframes. If so, allow `*.airwallex.com` (current Tauri `csp` is `null`).
- International catalog must actually return USD SKUs for `externalChannelCode=flowy`; empty list is a data/config issue, not a client bug.

## Tests (minimum)

- Hidden route: `/billing` is registered; `settingsNavigation` and sider sources do not mention it.
- `+` navigates to `/billing` when cloud-authenticated.
- Order create payload includes `payChannel: "airwallex"` and a stable idempotency key.
- Polling stops on `PAID` and triggers a credits refresh.
