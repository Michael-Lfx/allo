/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import type { PreviewContentType } from '@/common/types/office/preview';
import { findWorkspacePreviewTab, firstTabOfKind, upsertMixedPreviewTab } from './previewTabUpsert';

type Tab = {
  id: string;
  title: string;
  kind?: 'file' | 'terminal' | 'browser' | 'workspace';
  content_type?: PreviewContentType;
  workspaceTabKey?: string;
};

const file = (id: string, title: string): Tab => ({ id, title, kind: 'file', content_type: 'markdown' });
const term = (id: string): Tab => ({ id, title: id, kind: 'terminal', content_type: 'code' });
const browser = (id: string): Tab => ({ id, title: id, kind: 'browser', content_type: 'url' });
const workspace = (id: string, workspaceTabKey: string): Tab => ({
  id,
  title: workspaceTabKey,
  kind: 'workspace',
  content_type: 'code',
  workspaceTabKey,
});

describe('upsertMixedPreviewTab', () => {
  test('keeps a single file tab and replaces it when another file opens', () => {
    const tabs = [file('f1', 'a.md'), term('t1')];
    const next = upsertMixedPreviewTab(tabs, 'file', undefined, file('f2', 'b.md'));

    expect(next.filter((tab) => tab.kind === 'file')).toHaveLength(1);
    expect(next.find((tab) => tab.kind === 'file')?.title).toBe('b.md');
    expect(next.find((tab) => tab.kind === 'file')?.id).toBe('f1');
    expect(next.some((tab) => tab.id === 't1')).toBe(true);
  });

  test('focuses an already-open file instead of inserting another', () => {
    const existing = file('f1', 'a.md');
    const next = upsertMixedPreviewTab([existing, term('t1')], 'file', existing, {
      ...existing,
      title: 'a.md',
    });

    expect(next.filter((tab) => tab.kind === 'file')).toHaveLength(1);
    expect(next[0]?.id).toBe('f1');
  });

  test('allows multiple browser and terminal tabs', () => {
    const withBrowser = upsertMixedPreviewTab([file('f1', 'a.md')], 'browser', undefined, browser('b1'));
    const withTwoBrowsers = upsertMixedPreviewTab(withBrowser, 'browser', undefined, browser('b2'));
    const withTerminal = upsertMixedPreviewTab(withTwoBrowsers, 'terminal', undefined, term('t1'));
    const withTwoTerminals = upsertMixedPreviewTab(withTerminal, 'terminal', undefined, term('t2'));

    expect(withTwoBrowsers.filter((tab) => tab.kind === 'browser')).toHaveLength(2);
    expect(withTwoTerminals.filter((tab) => tab.kind === 'terminal')).toHaveLength(2);
    expect(withTwoTerminals.filter((tab) => tab.kind === 'file')).toHaveLength(1);
  });

  test('keeps one tab for each fixed workspace view', () => {
    const files = workspace('w-files', 'files');
    const next = upsertMixedPreviewTab([files], 'workspace', files, workspace('other', 'files'));
    const withChanges = upsertMixedPreviewTab(next, 'workspace', undefined, workspace('w-changes', 'changes'));

    expect(withChanges.filter((tab) => tab.kind === 'workspace' && tab.workspaceTabKey === 'files')).toHaveLength(1);
    expect(withChanges.filter((tab) => tab.kind === 'workspace')).toHaveLength(2);
    expect(findWorkspacePreviewTab(withChanges, 'changes')?.id).toBe('w-changes');
  });
});

describe('firstTabOfKind', () => {
  test('returns the first matching tab so a repeated rail click can jump instead of create', () => {
    const tabs = [term('t1'), file('f1', 'a.md'), term('t2')];
    expect(firstTabOfKind(tabs, 'terminal')?.id).toBe('t1');
    expect(firstTabOfKind(tabs, 'file')?.id).toBe('f1');
  });
});
