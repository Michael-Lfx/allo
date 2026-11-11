import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import Bolt from './Bolt';

function stripStyle(html: string): string {
  return html.replace(/<style>[\s\S]*?<\/style>/, '');
}

describe('Bolt character component', () => {
  it('renders idle posture when no motion is provided', () => {
    const html = stripStyle(
      renderToStaticMarkup(React.createElement(Bolt, { mood: 'content', activity: 'idle' }))
    );
    expect(html.includes('nomi-bolt--content')).toBe(true);
    expect(html.includes('nomi-bolt--idle')).toBe(true);
    expect(html.includes('nomi-bolt--listening')).toBe(false);
    expect(html.includes('nomi-bolt--speaking')).toBe(false);
  });

  it('renders listening posture when motion is alert', () => {
    const html = stripStyle(
      renderToStaticMarkup(React.createElement(Bolt, { mood: 'content', activity: 'idle', motion: 'alert' }))
    );
    expect(html.includes('nomi-bolt--listening')).toBe(true);
    expect(html.includes('nomi-bolt--motion-alert')).toBe(true);
    expect(html.includes('nomi-bolt__eyes--listening')).toBe(true);
    expect(html.includes('data-motion="alert"')).toBe(true);
  });

  it('renders speaking posture when motion is notify', () => {
    const html = stripStyle(
      renderToStaticMarkup(React.createElement(Bolt, { mood: 'content', activity: 'idle', motion: 'notify' }))
    );
    expect(html.includes('nomi-bolt--speaking')).toBe(true);
    expect(html.includes('nomi-bolt--motion-notify')).toBe(true);
    expect(html.includes('nomi-bolt__eyes--speaking')).toBe(true);
    expect(html.includes('nomi-bolt__mouth--speaking')).toBe(true);
    expect(html.includes('data-motion="notify"')).toBe(true);
  });

  it('renders thinking posture when motion is orbit or thinking', () => {
    const htmlOrbit = stripStyle(
      renderToStaticMarkup(React.createElement(Bolt, { mood: 'content', activity: 'idle', motion: 'orbit' }))
    );
    expect(htmlOrbit.includes('nomi-bolt__dot')).toBe(true);
    expect(htmlOrbit.includes('data-motion="orbit"')).toBe(true);

    const htmlThinking = stripStyle(
      renderToStaticMarkup(React.createElement(Bolt, { mood: 'content', activity: 'idle', motion: 'thinking' }))
    );
    expect(htmlThinking.includes('nomi-bolt__dot')).toBe(true);
    expect(htmlThinking.includes('data-motion="thinking"')).toBe(true);
  });

  it('renders sleepy posture when motion is sleep', () => {
    const html = stripStyle(
      renderToStaticMarkup(React.createElement(Bolt, { mood: 'happy', activity: 'idle', motion: 'sleep' }))
    );
    expect(html.includes('nomi-bolt__z')).toBe(true);
    expect(html.includes('data-motion="sleep"')).toBe(true);
  });
});
