import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = (rel: string) => readFileSync(new URL(rel, import.meta.url), 'utf8');

describe('studio session failure card billing CTA', () => {
  test('credits failures open the in-app billing route', () => {
    const message = source('./StudioSessionMessage.tsx');
    const css = source('./index.module.css');

    expect(message.includes("from '../videoFailureRecovery'")).toBe(true);
    expect(message.includes('resolveVideoFailureRecoveryAction(issueKind)')).toBe(true);
    expect(message.includes("data-testid='video-failure-open-billing'")).toBe(true);
    expect(message.includes('navigate(recovery.href)')).toBe(true);
    expect(message.includes("kind: recovery.source")).toBe(true);
    expect(message.includes("feature: 'video_generation'")).toBe(true);
    expect(message.includes("defaultValue: '购买积分'")).toBe(true);

    expect(css.includes('.issueActions {')).toBe(true);
    expect(css.includes('.issueAction {')).toBe(true);
  });

  test('cancelled issue cards do not reuse the credits recovery mapper', () => {
    const message = source('./StudioSessionMessage.tsx');
    expect(message.includes("item.kind === 'failure' ? resolveVideoFailureRecoveryAction(issueKind) : null")).toBe(
      true
    );
  });
});
