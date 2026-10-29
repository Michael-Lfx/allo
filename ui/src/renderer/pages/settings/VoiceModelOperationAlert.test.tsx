import { describe, expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { VoiceModelOperationAlert } from './VoiceSettings';

describe('voice model operation alert', () => {
  test('shows a delete failure even while the authoritative state is Ready', () => {
    const html = renderToStaticMarkup(
      <VoiceModelOperationAlert message='Delete did not complete; some files are still present' />,
    );
    expect(html).toContain('role="alert"');
    expect(html).toContain('Delete did not complete');
    expect(html).toContain('text-danger-6');
  });

  test('does not duplicate the error when the install state itself is Failed', () => {
    const html = renderToStaticMarkup(
      <VoiceModelOperationAlert message='backend reason' hidden />,
    );
    expect(html).toBe('');
  });

  test('renders nothing without an operation error', () => {
    expect(renderToStaticMarkup(<VoiceModelOperationAlert message={null} />)).toBe('');
  });
});
