import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const read = (url: URL) => readFileSync(url, 'utf8');
const probeSource = read(new URL('../pages/test/ErrorSurfaceProbe.tsx', import.meta.url));
const probeCss = read(new URL('../pages/test/errorSurfaceProbe.css', import.meta.url));
const auditSource = read(new URL('../../../../scripts/check-error-surface.mjs', import.meta.url));

describe('error surface browser probe contract', () => {
  test('mounts the real simple Modal and measures its disclosure and overflow states', () => {
    expect(probeSource).toContain("import { Modal } from '@arco-design/web-react';");
    expect(probeSource).toContain("className='error-surface-probe__modal'");
    expect(probeSource).toContain('modalHorizontalOverflow');
    expect(probeSource).toContain('modal-details-not-visible');
    expect(probeSource).toContain('modal?.querySelector<HTMLElement>');
    expect(probeCss).toContain('.error-surface-probe__modal');
  });

  test('fails the report when action order or enabled state drifts', () => {
    expect(probeSource).toContain('expectedActionLabels');
    expect(probeSource).toContain('action-order=');
    expect(probeSource).toContain('disabledActionCount');
    expect(probeSource).toContain('disabled-actions=');
    expect(auditSource).toContain('report.ok');
  });

  test('keeps a configuration recovery fixture in the browser matrix', () => {
    expect(probeSource).toContain("id: 'config-recovery'");
    expect(auditSource).toContain("'config-recovery'");
  });

  test('bounds browser process work and rejects unsafe launch conditions', () => {
    expect(auditSource).toContain('const activeChildren = new Set();');
    expect(auditSource).toContain('const maxCasesPerRun = 256;');
    expect(auditSource).toContain('const maxAttemptsPerCase = 2;');
    expect(auditSource).toContain('const selectCases = (cases, limit) => {');
    expect(auditSource).toContain("const allowedHosts = new Set(['localhost', '127.0.0.1', '[::1]']);");
    expect(auditSource).toContain('await cleanupActiveChildren();');
    expect(auditSource).not.toContain("'--no-sandbox'");
    expect(auditSource).not.toContain('deferred cleanup');
    expect(auditSource).not.toContain('--max-runs 0');
  });
});
