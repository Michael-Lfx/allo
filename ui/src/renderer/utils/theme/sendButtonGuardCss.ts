/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** Disabled send-button fill — shared by SendBox, Guid, OrchestratorComposer.
 * Must stay in sync with sendbox.css; injected last in Layout so ambiance presets
 * (all properties !important) cannot wash it back to --aou-2 / primary-light-3. */
export const SEND_BUTTON_DISABLED_BG = '#d3d4d9';
export const SEND_BUTTON_SIZE = '30px';

export const SEND_BUTTON_GUARD_STYLE_ID = 'send-button-style-guard';

export const SEND_BUTTON_GUARD_CSS = `
button.send-button-custom.arco-btn.arco-btn-shape-circle,
button[data-testid='guid-send-btn'].arco-btn.arco-btn-shape-circle,
button[data-testid='sendbox-send-btn'].arco-btn.arco-btn-shape-circle,
button[data-testid='orchestrator-send-btn'].arco-btn.arco-btn-shape-circle {
  width: ${SEND_BUTTON_SIZE} !important;
  height: ${SEND_BUTTON_SIZE} !important;
  min-width: ${SEND_BUTTON_SIZE} !important;
  padding: 0 !important;
  flex-shrink: 0 !important;
  line-height: 1 !important;
}

button.send-button-custom.arco-btn.arco-btn-primary.arco-btn-disabled,
button.send-button-custom.arco-btn.arco-btn-primary:disabled,
button.send-button-custom.arco-btn.arco-btn-primary.arco-btn-icon-only.arco-btn-disabled,
button.send-button-custom.arco-btn.arco-btn-primary.arco-btn-icon-only:disabled,
button[data-testid='guid-send-btn'].arco-btn.arco-btn-primary.arco-btn-disabled,
button[data-testid='guid-send-btn'].arco-btn.arco-btn-primary:disabled,
button[data-testid='sendbox-send-btn'].arco-btn.arco-btn-primary.arco-btn-disabled,
button[data-testid='sendbox-send-btn'].arco-btn.arco-btn-primary:disabled {
  background: ${SEND_BUTTON_DISABLED_BG} !important;
  background-color: ${SEND_BUTTON_DISABLED_BG} !important;
  background-image: none !important;
  border-color: ${SEND_BUTTON_DISABLED_BG} !important;
  color: #fff !important;
  opacity: 1 !important;
}

[data-theme='dark'] button.send-button-custom.arco-btn.arco-btn-primary.arco-btn-disabled,
[data-theme='dark'] button.send-button-custom.arco-btn.arco-btn-primary:disabled,
[data-theme='dark'] button[data-testid='guid-send-btn'].arco-btn.arco-btn-primary.arco-btn-disabled,
[data-theme='dark'] button[data-testid='guid-send-btn'].arco-btn.arco-btn-primary:disabled,
[data-theme='dark'] button[data-testid='sendbox-send-btn'].arco-btn.arco-btn-primary.arco-btn-disabled,
[data-theme='dark'] button[data-testid='sendbox-send-btn'].arco-btn.arco-btn-primary:disabled {
  background: color-mix(in srgb, var(--color-fill-4) 94%, var(--color-border-2)) !important;
  background-color: color-mix(in srgb, var(--color-fill-4) 94%, var(--color-border-2)) !important;
  background-image: none !important;
  border-color: color-mix(in srgb, var(--color-fill-4) 96%, var(--color-border-2)) !important;
  color: #fff !important;
  opacity: 1 !important;
}
`.trim();
