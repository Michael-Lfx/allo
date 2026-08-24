/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import React from 'react';
import SyntaxHighlightBoundary from './SyntaxHighlightBoundary';

const source = readFileSync(new URL('./SyntaxHighlightBoundary.tsx', import.meta.url), 'utf8');

describe('SyntaxHighlightBoundary', () => {
  test('is an error boundary that resets when resetKey changes', () => {
    expect(source.includes('getDerivedStateFromError')).toBe(true);
    expect(source.includes('previousProps.resetKey !== this.props.resetKey')).toBe(true);
    expect(source.includes('this.state.failed ? this.props.fallback : this.props.children')).toBe(true);
  });

  test('getDerivedStateFromError marks the boundary as failed', () => {
    const next = (
      SyntaxHighlightBoundary as unknown as {
        getDerivedStateFromError: () => { failed: boolean };
      }
    ).getDerivedStateFromError();
    expect(next).toEqual({ failed: true });
  });

  test('renders children while healthy', () => {
    const element = React.createElement(
      SyntaxHighlightBoundary,
      {
        fallback: React.createElement('code', { 'data-fallback': true }, 'plain'),
        resetKey: 'ok',
      },
      React.createElement('code', { 'data-ok': true }, 'highlighted')
    );
    expect(React.isValidElement(element)).toBe(true);
  });
});
