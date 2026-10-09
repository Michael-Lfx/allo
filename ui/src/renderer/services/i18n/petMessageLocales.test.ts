/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import enNomi from './locales/en-US/nomi.json';
import zhNomi from './locales/zh-CN/nomi.json';

type LocaleJson = Record<string, unknown>;

const PET_MESSAGE_KEYS = [
  'petMessage.dismiss',
  'petMessage.sources.conversation',
  'petMessage.sources.cron',
  'petMessage.sources.learning',
  'petMessage.sources.execution',
  'petMessage.sources.ssh',
  'petMessage.sources.robot',
  'petMessage.sources.requirement',
  'petMessage.sources.autowork',
  'petMessage.sources.app',
  'petMessage.conversation.preparing',
  'petMessage.conversation.thinking',
  'petMessage.conversation.thinkingOn',
  'petMessage.conversation.streaming',
  'petMessage.conversation.tooling',
  'petMessage.conversation.calling',
  'petMessage.conversation.callingOn',
  'petMessage.conversation.running',
  'petMessage.conversation.waiting',
  'petMessage.conversation.waitingOn',
  'petMessage.conversation.moa',
  'petMessage.conversation.finalizing',
  'petMessage.conversation.completed',
  'petMessage.conversation.failed',
  'petMessage.conversation.cancelled',
  'petMessage.cron.completed',
  'petMessage.cron.failed',
  'petMessage.cron.skipped',
  'petMessage.learning.course.started',
  'petMessage.learning.course.scope',
  'petMessage.learning.course.round',
  'petMessage.learning.course.audit',
  'petMessage.learning.course.publishing',
  'petMessage.learning.course.completed',
  'petMessage.learning.course.failed',
  'petMessage.learning.lesson.started',
  'petMessage.learning.lesson.round',
  'petMessage.learning.lesson.audit',
  'petMessage.learning.lesson.completed',
  'petMessage.learning.lesson.failed',
  'petMessage.execution.created',
  'petMessage.execution.status_changed',
  'petMessage.execution.plan_changed',
  'petMessage.execution.step_changed',
  'petMessage.execution.attempt_changed',
  'petMessage.execution.decision_requested',
  'petMessage.execution.decision_answered',
  'petMessage.execution.deleted',
  'petMessage.execution.planning',
  'petMessage.execution.adjust',
  'petMessage.ssh.connecting',
  'petMessage.ssh.connected',
  'petMessage.ssh.degraded',
  'petMessage.ssh.reconnecting',
  'petMessage.ssh.dropped',
  'petMessage.ssh.closed',
  'petMessage.ssh.idle',
  'petMessage.robot.listening',
  'petMessage.robot.speaking',
  'petMessage.robot.idle',
  'petMessage.requirement.pending',
  'petMessage.requirement.in_progress',
  'petMessage.requirement.done',
  'petMessage.requirement.failed',
  'petMessage.requirement.cancelled',
  'petMessage.requirement.needs_review',
  'petMessage.autowork.running',
  'petMessage.autowork.runningTagged',
  'petMessage.autowork.completed',
  'characters.puff.name',
  'characters.puff.style',
] as const;

function getLocaleValue(locale: LocaleJson, key: string): unknown {
  let cursor: unknown = locale;
  for (const segment of key.split('.')) {
    if (!cursor || typeof cursor !== 'object' || !Object.prototype.hasOwnProperty.call(cursor, segment)) {
      return undefined;
    }
    cursor = (cursor as LocaleJson)[segment];
  }
  return cursor;
}

describe('pet message locale coverage', () => {
  test('every pet-channel string has a label in both locales', () => {
    const failures: string[] = [];
    for (const [name, locale] of [
      ['en-US', enNomi as unknown as LocaleJson],
      ['zh-CN', zhNomi as unknown as LocaleJson],
    ] as const) {
      for (const key of PET_MESSAGE_KEYS) {
        const value = getLocaleValue(locale, key);
        if (typeof value !== 'string' || !value.trim()) failures.push(`${name} nomi.${key}`);
      }
    }
    expect(failures).toEqual([]);
  });
});
