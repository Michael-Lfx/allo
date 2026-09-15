import { describe, expect, test } from 'bun:test';
import {
  appendHomeImageLegend,
  detectHomeImageMentionQuery,
  filterHomeImageMentionCandidates,
  insertHomeImageMention,
  materializeHomeImageMentions,
  retargetMentionsAfterRemove,
  rewriteHomeImageMentionsToNodeTokens,
  splitHomeImageMentionParts,
} from './imageMentions';

describe('home image mentions', () => {
  test('detects an open @ query at the caret', () => {
    expect(detectHomeImageMentionQuery('让 @图', 4)).toEqual({ start: 2, query: '图' });
    expect(detectHomeImageMentionQuery('让 图片1 走', 8)).toBeNull();
  });

  test('inserts @图片N in place of the current @ query', () => {
    const next = insertHomeImageMention('让 @图', 4, 0);
    expect(next.text).toBe('让 @图片1 ');
    expect(next.cursor).toBe('让 @图片1 '.length);
  });

  test('inserts at the caret when there is no @ query', () => {
    const next = insertHomeImageMention('让  走', 2, 1);
    expect(next.text).toBe('让 @图片2  走');
  });

  test('retargets remaining mentions after deleting a middle image', () => {
    const text = '让 @图片1 站左，@图片2 站中，@图片3 站右';
    expect(retargetMentionsAfterRemove(text, 1, 3)).toBe('让 @图片1 站左， 站中，@图片2 站右');
  });

  test('does not treat @图片10 as @图片1 when retargeting', () => {
    expect(retargetMentionsAfterRemove('看 @图片1 和 @图片10', 0, 10)).toBe('看 和 @图片9');
  });

  test('materializes @图片N for video models', () => {
    expect(materializeHomeImageMentions('让 @图片1 走进 @图片2')).toBe('让 图片1 走进 图片2');
  });

  test('rewrites ordinal mentions into stable canvas node tokens', () => {
    expect(rewriteHomeImageMentionsToNodeTokens('让 @图片1 配合 @图片2', ['img-a', 'img-b'])).toBe(
      '让 @[node:img-a] 配合 @[node:img-b]',
    );
    expect(rewriteHomeImageMentionsToNodeTokens('让 @图片3 出现', ['img-a'])).toBe('让 @图片3 出现');
  });

  test('appends a cameo legend when reference names exist', () => {
    expect(appendHomeImageLegend('让 @图片1 出场', ['Alice', '教室'])).toBe(
      '让 图片1 出场\n\n图片对照：图片1 → Alice、图片2 → 教室',
    );
    expect(appendHomeImageLegend('hello', [])).toBe('hello');
  });

  test('filters mention candidates by label or name', () => {
    const items = [
      { index: 0, label: '图片1', name: 'Alice' },
      { index: 1, label: '图片2', name: '教室' },
    ];
    expect(filterHomeImageMentionCandidates(items, '2').map((item) => item.index)).toEqual([1]);
    expect(filterHomeImageMentionCandidates(items, 'ali').map((item) => item.index)).toEqual([0]);
  });

  test('splits @图片N tokens for in-prompt highlighting', () => {
    expect(splitHomeImageMentionParts('让 @图片1 走进 @图片2')).toEqual([
      { type: 'text', value: '让 ' },
      { type: 'mention', value: '@图片1' },
      { type: 'text', value: ' 走进 ' },
      { type: 'mention', value: '@图片2' },
    ]);
    expect(splitHomeImageMentionParts('@图片10')).toEqual([{ type: 'mention', value: '@图片10' }]);
  });
});
