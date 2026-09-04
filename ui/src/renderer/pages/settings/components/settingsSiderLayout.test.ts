import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const siderSource = readFileSync(new URL('./SettingsSider.tsx', import.meta.url), 'utf8');
const layoutStyles = readFileSync(new URL('../../../styles/layout.css', import.meta.url), 'utf8');

describe('settings sider text layout', () => {
  test('keeps long labels on one line and exposes an ellipsis instead of clipping', () => {
    const groupHeaderStart = layoutStyles.indexOf('.settings-sider__group-header {\n  min-width: 0;');
    const groupHeaderEnd = layoutStyles.indexOf('\n}', groupHeaderStart);
    const groupHeaderStyles = layoutStyles.slice(groupHeaderStart, groupHeaderEnd);

    const itemLabelStart = layoutStyles.indexOf('.settings-sider__item-label {\n  display: block;');
    const itemLabelEnd = layoutStyles.indexOf('\n}', itemLabelStart);
    const itemLabelStyles = layoutStyles.slice(itemLabelStart, itemLabelEnd);

    expect(groupHeaderStyles).toContain('text-overflow: ellipsis;');
    expect(groupHeaderStyles).toContain('white-space: nowrap;');
    expect(itemLabelStyles).toContain('text-overflow: ellipsis;');
    expect(itemLabelStyles).toContain('white-space: nowrap;');
  });

  test('allows the navigation label flex item to shrink before truncating', () => {
    expect(siderSource).toContain("<FlexFullContainer className='h-24px min-w-0 collapsed-hidden'>");
  });
});
