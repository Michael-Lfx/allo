/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Button } from '@arco-design/web-react';
import { Search } from '@icon-park/react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, test } from 'bun:test';

describe('Arco + Icon Park button DOM contract', () => {
  test('uses direct Icon Park and text children instead of removed Arco wrappers', () => {
    const markup = renderToStaticMarkup(
      <Button className='flowy-icon-text-btn' icon={<Search theme='outline' size='14' />}>
        检索
      </Button>
    );

    expect(markup).toContain('flowy-icon-text-btn');
    expect(markup).toContain('i-icon');
    expect(markup).toContain('检索');
    expect(markup).not.toContain('arco-btn-content');
    expect(markup).not.toContain('arco-btn-icon');
  });
});
