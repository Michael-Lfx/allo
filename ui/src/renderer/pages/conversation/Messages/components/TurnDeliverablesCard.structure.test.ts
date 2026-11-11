/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const cardSource = readFileSync(new URL('./TurnDeliverablesCard.tsx', import.meta.url), 'utf8');
const listSource = readFileSync(new URL('../MessageList.tsx', import.meta.url), 'utf8');

describe('TurnDeliverablesCard structure', () => {
  test('never renders an empty or unverified card', () => {
    // Availability must settle before anything is shown, and zero trustworthy
    // items must render nothing.
    expect(cardSource.includes('DELIVERABLE_SETTLE_MS')).toBe(true);
    expect(cardSource.includes('if (pending || available.length === 0) return null;')).toBe(true);
    // Reported items are gated on a backend existence probe; only committed
    // receipts skip the client-side probe.
    expect(cardSource.includes("if (item.tier === 'receipt') return { ...item, statPath };")).toBe(true);
    expect(cardSource.includes('probeAvailability(statPath, workspace)')).toBe(true);
  });

  test('hides dead action buttons on degraded surfaces', () => {
    // No preview affordance without a PreviewProvider (read-only projections).
    expect(cardSource.includes('canPreview && isPreviewSupportedExt(item.fileName)')).toBe(true);
    // Shell open/reveal execute on the backend host; hide them in WebUI mode.
    expect(cardSource.includes('isDesktopShell()')).toBe(true);
  });

  test('truncates to the first three files with an explicit reveal control', () => {
    expect(cardSource.includes('DEFAULT_VISIBLE_COUNT = 3')).toBe(true);
    expect(cardSource.includes('available.slice(0, DEFAULT_VISIBLE_COUNT)')).toBe(true);
    expect(cardSource.includes("aria-expanded={showAll}")).toBe(true);
    expect(cardSource.includes('messages.turnDeliverables.showMore')).toBe(true);
    expect(cardSource.includes('messages.turnDeliverables.showLess')).toBe(true);
  });

  test('lays file cards out two-up and collapses to one column when the row is narrow', () => {
    const cardCss = readFileSync(new URL('./TurnDeliverablesCard.module.css', import.meta.url), 'utf8');
    expect(cardSource.includes('data-testid=\'turn-deliverables-grid\'')).toBe(true);
    expect(cardSource.includes('styles.grid')).toBe(true);
    expect(cardCss.includes('grid-template-columns: repeat(2, minmax(0, 1fr));')).toBe(true);
    expect(cardCss.includes('@container turn-deliverables (max-width: 520px)')).toBe(true);
    expect(cardCss.includes('grid-template-columns: minmax(0, 1fr);')).toBe(true);
    expect(cardCss.includes('repeat(3')).toBe(false);
    expect(cardSource.includes("title={t('preview.preview')}")).toBe(true);
  });

  test('uses supplied file-type artwork while preserving the custom Markdown glyph', () => {
    expect(cardSource.includes("from '@/renderer/assets/file-types/CODE.svg'")).toBe(true);
    expect(cardSource.includes("from '@/renderer/assets/file-types/JAVA.svg'")).toBe(true);
    expect(cardSource.includes("from '@/renderer/assets/file-types/JPG.svg'")).toBe(true);
    expect(cardSource.includes("from '@/renderer/assets/file-types/ZIP.svg'")).toBe(true);
    expect(cardSource.includes('const FILE_ICON_BY_KIND')).toBe(true);
    expect(cardSource.includes("kind === 'code' && extension === 'java'")).toBe(true);
    expect(cardSource.includes('styles.assetGlyph')).toBe(true);
    expect(cardSource.includes('const CUSTOM_FILE_MARK_BY_KIND')).toBe(true);
    expect(cardSource.includes('{CUSTOM_FILE_MARK_BY_KIND[kind]}')).toBe(true);
    expect(cardSource.includes("kind === 'markdown' ? 32 : 28")).toBe(true);
    expect(cardSource.includes('GLYPH_BY_KIND')).toBe(false);
    expect(cardSource.includes('styles.glyphMarkdown')).toBe(false);
  });

  test('renders a filename title with a kind and size meta line', () => {
    expect(cardSource.includes('splitFileDisplayPath(item.relativePath, item.fileName)')).toBe(true);
    expect(cardSource.includes('fileTitle(displayPath.fileName)')).toBe(true);
    expect(cardSource.includes('messages.turnDeliverables.meta')).toBe(true);
    expect(cardSource.includes('messages.turnDeliverables.kind.')).toBe(true);
    expect(cardSource.includes('title={displayPath.fullPath}')).toBe(true);
    expect(cardSource.includes('secondaryPath')).toBe(false);
    expect(cardSource.includes('key={item.absolutePath ?? item.relativePath}')).toBe(true);
  });

  test('message list mounts the card once per turn behind the deliverables model', () => {
    expect(listSource.includes('collectTurnDeliverables')).toBe(true);
    expect(listSource.includes('shouldPresentTurnDeliverables')).toBe(true);
    expect(listSource.includes('shouldPrefetchOlderHistoryForDeliverables')).toBe(true);
    expect(listSource.includes("type: 'turn_deliverables'")).toBe(true);
    // Stable anchor id keyed by turn identity (React key + jump-target contract).
    expect(listSource.includes('`turn-deliverables-${turnId}`')).toBe(true);
    // The card gates on the same turn lifecycle the disclosure header uses.
    expect(listSource.includes('turnGates.set(entry.msg_id, { running: entry.running, state: entry.state })')).toBe(
      true
    );
  });

  test('places the final assistant actions after the turn deliverables card', () => {
    expect(listSource.includes("type: 'turn_actions'")).toBe(true);
    expect(listSource.includes('`turn-actions-${turnId}`')).toBe(true);
    expect(listSource.indexOf("type: 'turn_deliverables'")).toBeLessThan(
      listSource.lastIndexOf("type: 'turn_actions'")
    );
    expect(listSource.includes('actionsOnly')).toBe(true);
    expect(listSource.includes('creditTurnId={item.turn_id}')).toBe(true);
    expect(listSource.includes('conversationContext?.activeTurnId === item.turn_id')).toBe(true);
    expect(listSource.includes('movedActionMessageIds.has((item as TMessage).id)')).toBe(true);
  });
});
