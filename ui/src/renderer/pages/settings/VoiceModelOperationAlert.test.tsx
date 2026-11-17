import { describe, expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { VoiceModelOperationAlert } from './VoiceSettings';
import en from '../../services/i18n/locales/en-US/voice.json';
import zh from '../../services/i18n/locales/zh-CN/voice.json';

describe('voice model operation alert', () => {
  test('renders the real sync error in both locales and clears it after recovery', () => {
    for (const locale of [en, zh]) {
      const html = renderToStaticMarkup(<VoiceModelOperationAlert message={locale.errors.modelStatusUnavailable} />);
      expect(html).toContain('role="alert"');
      expect(html).toContain(locale.errors.modelStatusUnavailable);
      expect(html).not.toContain(locale.phases.notInstalled);
      expect(renderToStaticMarkup(<VoiceModelOperationAlert message={null} />)).toBe('');
    }
  });

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
