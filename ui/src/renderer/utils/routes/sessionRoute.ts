/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import {
  tryParseEntityId,
  type ConversationId,
  type SessionTarget,
  type TerminalId,
} from '@/common/types/ids';

/**
 * Parses the two canonical session detail routes without throwing.
 *
 * Persistent layout components (titlebar, session sidebar and shortcuts) stay
 * mounted while the leaf route changes. They must never use a strict entity-id
 * parser during render: a route for the other session kind, or a malformed URL,
 * would otherwise take down the entire shared application shell.
 */
export const parseSessionRoute = (pathname: string): SessionTarget | null => {
  const match = pathname.match(/^\/(conversation|terminal)\/([^/?#]+)\/?$/);
  if (!match) return null;

  if (match[1] === 'conversation') {
    const id: ConversationId | null = tryParseEntityId('conversation', match[2]);
    return id ? { kind: 'conversation', id } : null;
  }

  const id: TerminalId | null = tryParseEntityId('terminal', match[2]);
  return id ? { kind: 'terminal', id } : null;
};

/**
 * Reads the live route pathname from a location href.
 *
 * The SPA runs under `HashRouter`, so the route lives in the URL fragment for
 * desktop/deep-link entries; browser-hosted builds may expose it as the real
 * pathname. Code that must re-read the route *after* an await cannot rely on
 * the `useLocation()` value captured by an effect closure, so it reads the
 * live URL through this helper instead.
 */
export const routePathnameFromHref = (href: string): string => {
  try {
    const url = new URL(href);
    if (url.hash.startsWith('#/')) {
      const queryIndex = url.hash.indexOf('?');
      return queryIndex === -1 ? url.hash.slice(1) : url.hash.slice(1, queryIndex);
    }
    return url.pathname || '/';
  } catch {
    return '/';
  }
};
