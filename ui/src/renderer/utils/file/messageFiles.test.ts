import { describe, expect, test } from 'bun:test';

import type { FileOrFolderItem } from '@/renderer/utils/file/fileTypes';
import { collectSelectedFiles, removeSubmittedAttachments } from '@/renderer/utils/file/messageFiles';

const item = (path: string): FileOrFolderItem => ({ path, name: path } as FileOrFolderItem);

/**
 * C2 附件集合差覆盖（对应计划测试 11）。
 *
 * removeSubmittedAttachments 是 NomiSendBox.handleEditResubmit 成功路径所调用的
 * 纯函数：函数式 setUploadFile 与 atPath 经 ref 读取后均委托给它。此处覆盖决策
 * 即覆盖行为契约——提交 X 飞行中新增 Y → 仅剩 Y；飞行中删 X → 幂等；无新增 → 清空。
 *
 * Coverage for the C2 attachment set-difference (plan test 11).
 * removeSubmittedAttachments is the pure function invoked on NomiSendBox's
 * edit-resubmit success path — both the functional setUploadFile update and the
 * ref-read atPath filter delegate to it. Covering the decision covers the contract:
 * submit X + add Y mid-flight → keep only Y; delete X mid-flight → idempotent;
 * nothing added → emptied.
 */
describe('removeSubmittedAttachments', () => {
  test('removes exactly the submitted items, preserving others (mid-flight add)', () => {
    // Submitted X; user added Y mid-flight → current is [X, Y]; result keeps only Y.
    const submitted = new Set(['/x.txt']);
    expect(removeSubmittedAttachments(['/x.txt', '/y.txt'], submitted)).toEqual(['/y.txt']);
  });

  test('submit then nothing added → emptied (full clear via set-difference)', () => {
    const submitted = new Set(['/x.txt', '/z.png']);
    expect(removeSubmittedAttachments(['/x.txt', '/z.png'], submitted)).toEqual([]);
  });

  test('user deleted a submitted item mid-flight → idempotent (no error, no residual)', () => {
    // Submitted X; user removed X mid-flight → current is already []; result [].
    const submitted = new Set(['/x.txt']);
    expect(removeSubmittedAttachments([], submitted)).toEqual([]);
  });

  test('partial mid-flight deletion removes only what remains submitted', () => {
    // Submitted [X, Z]; user removed Z mid-flight → current [X]; result [] (X removed).
    const submitted = new Set(['/x.txt', '/z.png']);
    expect(removeSubmittedAttachments(['/x.txt'], submitted)).toEqual([]);
  });

  test('nothing submitted (empty snapshot) → current selection untouched', () => {
    // filesToSend was empty → empty snapshot; no attachments consumed, keep all.
    expect(removeSubmittedAttachments(['/x.txt', '/y.txt'], new Set<string>())).toEqual(['/x.txt', '/y.txt']);
  });

  test('does not match by substring/path-prefix (exact path only)', () => {
    const submitted = new Set(['/a/b.txt']);
    // A sibling under the same directory must survive; only the exact path is removed.
    expect(removeSubmittedAttachments(['/a/b.txt', '/a/c.txt'], submitted)).toEqual(['/a/c.txt']);
  });

  test('handles FileOrFolderItem objects via .path (mirrors collectSelectedFiles extraction)', () => {
    const submitted = new Set(['/x.txt']);
    expect(removeSubmittedAttachments([item('/x.txt'), item('/y.txt')], submitted)).toEqual([item('/y.txt')]);
  });

  test('handles a mixed string + object selection', () => {
    const submitted = new Set(['/x.txt', '/z.png']);
    expect(
      removeSubmittedAttachments(['/x.txt', item('/y.txt'), '/z.png'], submitted)
    ).toEqual([item('/y.txt')]);
  });

  test('order is preserved for surviving items', () => {
    const submitted = new Set(['/b.txt']);
    expect(
      removeSubmittedAttachments(['/a.txt', '/b.txt', '/c.txt', '/b.txt', '/d.txt'], submitted)
    ).toEqual(['/a.txt', '/c.txt', '/d.txt']);
  });

  test('dedupes nothing itself (callers dedupe at collect time); exact-path removal is per-element', () => {
    // If the same path appears twice and is submitted, both copies are removed.
    const submitted = new Set(['/x.txt']);
    expect(removeSubmittedAttachments(['/x.txt', '/x.txt'], submitted)).toEqual([]);
  });
});

describe('collectSelectedFiles (path-extraction contract)', () => {
  test('unions uploadFile + atPath paths, deduped', () => {
    expect(collectSelectedFiles(['/x.txt'], ['/x.txt', item('/y.txt')])).toEqual(['/x.txt', '/y.txt']);
  });

  test('drops falsy paths from atPath', () => {
    expect(collectSelectedFiles([], ['', '/y.txt'])).toEqual(['/y.txt']);
  });
});
