import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./ErrorDiagnosticContent.tsx', import.meta.url), 'utf8');
const modalCss = readFileSync(new URL('../../styles/arco-override.css', import.meta.url), 'utf8');

describe('ErrorDiagnosticContent', () => {
  test('keeps the safe summary visible and technical details progressively disclosed', () => {
    expect(source.includes('conversation-error-diagnostic__summary')).toBe(true);
    expect(source.includes('conversation-error-diagnostic__detail')).toBe(true);
    expect(source.includes('common.technical_details')).toBe(true);
    expect(source.includes('defaultActiveKey')).toBe(false);
  });

  test('uses the shared copy primitive and exposes locating metadata', () => {
    expect(source.includes('CopyIconButton')).toBe(true);
    expect(source.includes('conversation.agentError.copyDiagnostic')).toBe(true);
    expect(source.includes('conversation.agentError.errorCode')).toBe(true);
    expect(source.includes('conversation.agentError.incidentId')).toBe(false);
    expect(source.includes('conversation.agentError.httpStatus')).toBe(true);
    expect(source.includes('conversation-error-diagnostic__meta-item')).toBe(true);
  });

  test('bounds and wraps long modal diagnostics', () => {
    const detailSection = modalCss.slice(modalCss.indexOf('.conversation-error-diagnostic__detail'));
    expect(modalCss.includes('.conversation-error-diagnostic__detail')).toBe(true);
    expect(modalCss.includes('max-height: 240px')).toBe(true);
    expect(modalCss.includes('overflow: auto')).toBe(true);
    expect(modalCss.includes('overflow-wrap: anywhere')).toBe(true);
    expect(detailSection.includes('.arco-modal-confirm-content')).toBe(false);
  });
});
