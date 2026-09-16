/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import { parseSessionRoute, routePathnameFromHref } from './sessionRoute';

const CONVERSATION_ID = '0190f5fe-7c00-7a00-8000-000000000001';
const TERMINAL_ID = '0190f5fe-7c00-7a00-8000-000000000002';

describe('parseSessionRoute', () => {
  test('returns a discriminated target for each canonical session route', () => {
    expect(parseSessionRoute(`/conversation/${CONVERSATION_ID}`)).toEqual({
      kind: 'conversation',
      id: CONVERSATION_ID,
    });
    expect(parseSessionRoute(`/terminal/${TERMINAL_ID}`)).toEqual({
      kind: 'terminal',
      id: TERMINAL_ID,
    });
  });

  test('uses the route namespace because bare UUIDs carry no encoded entity kind', () => {
    expect(parseSessionRoute(`/conversation/${TERMINAL_ID}`)).toEqual({
      kind: 'conversation',
      id: TERMINAL_ID,
    });
    expect(parseSessionRoute(`/terminal/${CONVERSATION_ID}`)).toEqual({
      kind: 'terminal',
      id: CONVERSATION_ID,
    });
  });

  test('returns null instead of throwing for malformed or non-detail routes', () => {
    for (const pathname of [
      '/conversation/not-an-id',
      '/conversation/0190f5fe-7c00-7a00-8000-00000000000g',
      '/terminal/42',
      `/terminal/${TERMINAL_ID}/unexpected`,
      '/terminal-new',
      '/guid',
    ]) {
      expect(parseSessionRoute(pathname)).toBeNull();
    }
  });
});

describe('routePathnameFromHref', () => {
  test('reads the hash route used by the desktop shell, query stripped', () => {
    expect(routePathnameFromHref(`https://tauri.localhost/#/conversation/${CONVERSATION_ID}`)).toBe(
      `/conversation/${CONVERSATION_ID}`
    );
    expect(
      routePathnameFromHref(
        `https://tauri.localhost/#/conversation/${CONVERSATION_ID}?attention_id=conversation:${CONVERSATION_ID}:turn:abc`
      )
    ).toBe(`/conversation/${CONVERSATION_ID}`);
  });

  test('falls back to the real pathname when the hash carries no route', () => {
    expect(routePathnameFromHref(`http://127.0.0.1:5173/conversation/${CONVERSATION_ID}?x=1`)).toBe(
      `/conversation/${CONVERSATION_ID}`
    );
    expect(routePathnameFromHref('http://127.0.0.1:5173/')).toBe('/');
  });

  test('never throws on malformed hrefs', () => {
    expect(routePathnameFromHref('not a url')).toBe('/');
  });
});
