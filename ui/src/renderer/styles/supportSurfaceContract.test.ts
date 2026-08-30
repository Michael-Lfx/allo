import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const read = (url: URL) => readFileSync(url, 'utf8');
const probe = read(new URL('../pages/test/SupportSurfaceProbe.tsx', import.meta.url));
const probeCss = read(new URL('../pages/test/supportSurfaceProbe.css', import.meta.url));
const modal = read(new URL('../features/supportChat/components/SupportChatModal.tsx', import.meta.url));
const audit = read(new URL('../../../../scripts/check-support-surface.mjs', import.meta.url));
const auditContract = read(new URL('../../../../scripts/check-support-surface-contract.mjs', import.meta.url));
const main = read(new URL('../main.tsx', import.meta.url));
const packageJson = read(new URL('../../../../package.json', import.meta.url));

describe('support surface browser probe contract', () => {
  test('renders the production modal view and both controlled surface fixtures', () => {
    expect(probe).toContain('<SupportChatModalView');
    expect(probe).toContain('<ConversationErrorReportModal');
    expect(probe).toContain('scrollOwnerCount');
    expect(probe).toContain('iconCenterDeltaY');
    expect(probe).toContain('focusVisibleControlCount');
    expect(probe).toContain('contrastChecks');
    expect(probe).toContain('screenshotPreviewSize');
    expect(probe).toContain("root?.querySelectorAll<HTMLElement>('.support-image-preview-item')");
    expect(probe).toContain("scenario === 'log-confirm'");
    expect(probe).toContain("data-delivery=\"sending\"");
    expect(probe).toContain('DataTransfer');
    expect(probeCss).toContain('.support-surface-probe');
  });

  test('registers the dev-only hash entry without affecting the normal provider path', () => {
    expect(main).toContain("window.location.hash.split('?')[0] === '#/test/support-surface'");
    expect(main).toContain('<SupportSurfaceProbe />');
    expect(modal).toContain('export const SupportChatModalView');
    expect(modal).toContain('composerDisabled?: boolean');
  });

  test('bounds Edge work, restricts URLs to loopback and confirms cleanup', () => {
    for (const fragment of [
      'const activeChildren = new Set();',
      'const maxCasesPerRun = 128;',
      'const maxAttemptsPerCase = 3;',
      "const allowedHosts = new Set(['localhost', '127.0.0.1', '[::1]']);",
      'await cleanupActiveChildren();',
      'Edge profile cleanup was not confirmed',
    ]) {
      expect(audit).toContain(fragment);
    }
    expect(audit).not.toContain('--no-sandbox');
    expect(auditContract).toContain('support-surface-probe-result');
    expect(packageJson).toContain('"check:support-surface-contract"');
    expect(packageJson).toContain('bun run check:support-surface-contract');
  });
});
