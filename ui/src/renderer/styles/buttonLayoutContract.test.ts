/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const read = (url: URL) => readFileSync(url, 'utf8');
const contractCss = read(new URL('./theme-control-contract.css', import.meta.url));
const contractRuntime = read(new URL('../utils/theme/themeControlContract.ts', import.meta.url));
const probeSource = read(new URL('../pages/test/ButtonLayoutProbe.tsx', import.meta.url));
const probeEntrySource = read(new URL('../main.tsx', import.meta.url));
const contractAuditSource = read(new URL('../../../../scripts/check-button-layout-contract.mjs', import.meta.url));
const packageSource = read(new URL('../../../../package.json', import.meta.url));

const applicationSources = [
  read(new URL('../pages/knowledge/KnowledgeDetailPage/index.tsx', import.meta.url)),
  read(new URL('../pages/conversation/platforms/nomi/NomiModelSelector.tsx', import.meta.url)),
  read(new URL('../components/agent/AgentModeSelector.tsx', import.meta.url)),
  read(new URL('../components/agent/AcpModelSelector.tsx', import.meta.url)),
  read(new URL('../pages/guid/components/GuidModelSelector.tsx', import.meta.url)),
  read(new URL('../pages/guid/components/GuidActionRow.tsx', import.meta.url)),
  read(new URL('../components/base/CopyFullIdButton.tsx', import.meta.url)),
  read(new URL('../pages/knowledge/KnowledgeDetailPage/KnowledgeDetailActionBar.tsx', import.meta.url)),
];

const staleSelectorSources = [
  new URL('../components/chat/SendBox/sendbox.css', import.meta.url),
  new URL('../components/chat/GoalModeChip/index.module.css', import.meta.url),
  new URL('../pages/guid/index.module.css', import.meta.url),
  new URL('../pages/conversation/components/ChatLayout/chat-layout.css', import.meta.url),
  new URL('./arco-override.css', import.meta.url),
].map(read);

describe('global icon-text button layout contract', () => {
  test('pins the opt-in Arco and explicit content contracts to one horizontal row', () => {
    for (const selector of ['.arco-btn.flowy-icon-text-btn', '.arco-btn.flowy-button-icon', '.flowy-button-inline-content']) {
      expect(contractCss.includes(selector)).toBe(true);
    }
    for (const [selector, gap] of [
      ['.arco-btn.flowy-icon-text-btn.arco-btn-size-mini', 'gap: 4px !important;'],
      ['.arco-btn.flowy-icon-text-btn.arco-btn-size-small', 'gap: 6px !important;'],
      ['.arco-btn.flowy-icon-text-btn.arco-btn-size-default', 'gap: 8px !important;'],
    ]) {
      expect(contractCss).toContain(selector);
      expect(contractCss).toContain(gap);
    }
    for (const declaration of [
      'display: inline-flex !important;',
      'flex-direction: row !important;',
      'align-items: center !important;',
      'justify-content: center !important;',
      'white-space: nowrap !important;',
      'flex: 0 0 auto !important;',
      'line-height: 0 !important;',
    ]) {
      expect(contractCss.includes(declaration)).toBe(true);
    }
  });

  test('keeps stale Arco 2.66 content selectors out of application CSS', () => {
    for (const source of staleSelectorSources) {
      expect(source).not.toContain('.arco-btn-content');
      expect(source).not.toMatch(/\.arco-btn-icon(?:[\s,{):]|$)/);
    }
  });

  test('opts the screenshot/action-bar and selector surfaces into the contract', () => {
    for (const source of applicationSources) {
      expect(source).toContain('flowy-icon-text-btn');
    }
    expect(probeSource).toContain("data-button-probe='arco-icon-prop-search'");
    expect(probeSource).toContain("id: 'production-knowledge-search'");
    expect(probeSource).toContain("id: 'production-nomi-model-selector'");
    expect(probeSource).toContain("id: 'production-agent-mode-selector'");
    expect(probeSource).toContain("id: 'production-reasoning-effort'");
    expect(probeSource).toContain('<NomiModelSelector');
    expect(probeSource).toContain('<AgentModeSelector');
    expect(probeSource).toContain('<GuidModelSelectorButton');
    expect(probeSource).toContain('<KnowledgeDetailActionBar');
    expect(probeSource).toContain('CHAT_PROBE_SCENARIOS');
    expect(probeSource).toContain('measureChatCoordinateStability');
    expect(probeSource).toContain("data-layout-part='leading-icon'");
    expect(probeSource).toContain("data-layout-part='chevron'");
    expect(probeSource).toContain("data-layout-part='context-ring'");
    expect(probeSource).toContain("data-layout-part='microphone'");
    expect(probeSource).toContain("data-layout-part='send'");
    expect(probeSource).toContain("scenario === 'adversarial'");
    expect(probeEntrySource).toContain("import.meta.env.DEV && window.location.hash.split('?')[0] === '#/test/button-layout'");
  });

  test('keeps the global source audit registered in the repository check', () => {
    expect(contractAuditSource).toContain('createSourceFile');
    expect(contractAuditSource).toContain('scanButtonLayoutContracts');
    expect(packageSource).toContain('"check:button-layout-contract"');
    expect(packageSource).toContain('bun run check:button-layout-contract');
  });

  test('preserves separate SendBox and Guid responsive thresholds', () => {
    const sendboxCss = read(new URL('../components/chat/SendBox/sendbox.css', import.meta.url));
    const guidCss = read(new URL('../pages/guid/index.module.css', import.meta.url));
    expect(sendboxCss).toContain('@container sendbox-config (max-width: 560px)');
    expect(guidCss).toContain('@container guid-action-config (max-width: 440px)');
    expect(sendboxCss).toContain('.flowy-button-inline-content');
    expect(guidCss).toContain('.flowy-button-inline-content');
  });

  test('injects the structural guard after user/preset CSS', () => {
    expect(contractRuntime).toContain('existing === document.head.lastElementChild');
    expect(contractRuntime).toContain('document.head.appendChild(styleEl)');
    expect(probeSource).toContain('processCustomCss');
    expect(probeSource).toContain('ensureThemeControlContract()');
  });
});
