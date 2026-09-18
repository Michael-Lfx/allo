/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const actionsSource = readFileSync(new URL('./hooks/useConversationActions.ts', import.meta.url), 'utf8');
const listSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const routerSource = readFileSync(new URL('../../../components/layout/Router.tsx', import.meta.url), 'utf8');
const boundarySource = readFileSync(
  new URL('../../../components/layout/RouteErrorBoundary.tsx', import.meta.url),
  'utf8'
);
const mainSource = readFileSync(new URL('../../../main.tsx', import.meta.url), 'utf8');

describe('shared session shell route safety', () => {
  test('conversation actions receive an already-discriminated conversation id', () => {
    expect(actionsSource.includes('useParams')).toBe(false);
    expect(actionsSource.includes('parseConversationId')).toBe(false);
    expect(actionsSource.includes('activeConversationId: ConversationId | null')).toBe(true);
    expect(listSource.includes('parseSessionRoute(pathname)')).toBe(true);
    expect(listSource.includes('useConversationActions({\n    activeConversationId,')).toBe(true);
  });

  test('the persistent session shell has a navigation-resetting error boundary', () => {
    expect(routerSource.includes('<Route element={<SessionShellRoute />}>')).toBe(true);
    expect(routerSource.includes('<RouteErrorBoundary resetKey={resetKey}>')).toBe(true);
    expect(boundarySource.includes('componentDidUpdate(previousProps: RouteErrorBoundaryProps)')).toBe(true);
    expect(boundarySource.includes('previousProps.resetKey !== this.props.resetKey')).toBe(true);
  });

  test('the renderer root has a final visible failure boundary', () => {
    expect(mainSource).toMatch(/<RouteErrorBoundary scope='application'>\s*<AppProviders>/);
    expect(mainSource.includes('return <AppLoader />')).toBe(true);
    expect(mainSource.includes('.finally(() => setConfigReady(true))')).toBe(false);
    expect(boundarySource.includes("window.location.reload()")).toBe(true);
  });

  test('does not mount a global cloud-model loading banner', () => {
    expect(mainSource.includes('CloudModelEnvironmentGate')).toBe(false);
    expect(mainSource.includes('cloudModelEnvironment.restoringTitle')).toBe(false);
  });

  test('expired WebUI auth returns to login without tripping the application boundary', () => {
    expect(mainSource).toMatch(/const \{ ready, status(?:, user)? \} = useAuth\(\)/);
    expect(mainSource.includes("if (!ready || status !== 'authenticated') {")).toBe(true);
    expect(mainSource.includes('if (!active || isHandledAuthExpiredHttpError(error)) return;')).toBe(true);
    expect(mainSource.includes("if (status !== 'authenticated') {\n    return router;")).toBe(true);
  });

  test('route parameter boundaries do not throw on malformed entity IDs', () => {
    const conversationSource = readFileSync(new URL('../index.tsx', import.meta.url), 'utf8');
    const knowledgeSource = readFileSync(new URL('../../knowledge/KnowledgeDetailPage/index.tsx', import.meta.url), 'utf8');
    const terminalSource = readFileSync(new URL('../../terminal/TerminalSessionPage.tsx', import.meta.url), 'utf8');

    expect(conversationSource).toContain("tryParseEntityId('conversation'");
    expect(conversationSource).toContain("<Navigate to='/guid' replace />");
    expect(knowledgeSource).toContain("tryParseEntityId('knowledge-base'");
    expect(knowledgeSource).toContain("<Navigate to='/knowledge' replace />");
    expect(terminalSource).toContain("tryParseEntityId('terminal'");
    expect(terminalSource).toContain("<Navigate to='/guid' replace />");
  });

  test('legacy requirement deep links encode query parameters before redirecting', () => {
    expect(routerSource).toContain("new URLSearchParams({ req: id ?? '', edit: '1' })");
  });
});
