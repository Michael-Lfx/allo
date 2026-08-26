/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessageText.tsx', import.meta.url), 'utf8');
const typographySource = readFileSync(new URL('../typography.ts', import.meta.url), 'utf8');
const messagesCss = readFileSync(new URL('../messages.css', import.meta.url), 'utf8');

describe('MessageText process action chrome', () => {
  test('keeps copy, edit, and rollback actions always visible', () => {
    expect(source.includes('hideActions?: boolean')).toBe(true);
    expect(source.includes('const shouldShowActions = !hideActions;')).toBe(true);
    expect(source.includes("data-testid='message-copy-action'")).toBe(true);
    expect(source.includes("fill='currentColor'")).toBe(true);
    const copyButtonSource =
      source.match(/const copyButton = \([\s\S]*?const canEdit =/)?.[0] ?? '';
    expect(copyButtonSource.includes('opacity-0')).toBe(false);
    expect(copyButtonSource.includes('pointer-events-none')).toBe(false);
    expect(copyButtonSource.includes('message-text-actions__reveal')).toBe(false);
    expect(copyButtonSource.includes('hoverRevealActionClass')).toBe(false);
    expect(source.includes('message-text-hover-root')).toBe(false);
    expect(messagesCss.includes('message-text-actions__reveal')).toBe(false);
    expect(
      source.includes("className='message-text-actions__time text-12px leading-20px text-inherit select-none'")
    ).toBe(true);
  });

  test('shows billed model names inline on the turn-credit chip', () => {
    expect(source.includes("from './turnCreditsLabel'")).toBe(true);
    expect(source.includes('content={turnCreditDetails}')).toBe(true);
    expect(source.includes("t('messages.turnCredits.consumedBy'")).toBe(true);
    const iconImport = source.match(/import \{([^}]+)\} from '@icon-park\/react'/)?.[1] ?? '';
    expect(iconImport.includes('Star')).toBe(false);
  });

  test('can render the unchanged message actions at the visual end of a turn', () => {
    expect(source.includes('actionsOnly?: boolean')).toBe(true);
    expect(source.includes('if (actionsOnly)')).toBe(true);
    expect(source.includes('{actionsRow}')).toBe(true);
  });

  test('wraps the markdown and plain body in the Beautiful UI streaming shell', () => {
    expect(source.includes('<StreamingText')).toBe(true);
    expect(source.includes('fontSize={MESSAGE_BODY_FONT_SIZE}')).toBe(true);
    expect(source.includes('hideActions?: boolean')).toBe(true);
    expect(source.includes('actionsOnly?: boolean')).toBe(true);
  });

  test('gives StreamingText the markdown and plain bodies without an extra block wrapper', () => {
    expect(source.includes("data-testid='message-text-content'")).toBe(true);
    const streamingBlock = source.match(/<StreamingText[\s\S]*?<\/StreamingText>/)?.[0] ?? '';
    expect(streamingBlock.includes('<MarkdownView')).toBe(true);
    expect(streamingBlock.includes('className={MESSAGE_BODY_CLASS_NAME}')).toBe(true);
    expect(streamingBlock.includes('<CollapsibleContent')).toBe(true);
    expect(streamingBlock.includes("data-testid='message-text-content'")).toBe(false);
    expect(streamingBlock.includes("className='message-streaming-content'")).toBe(false);
  });

  test('does not pop the bubble while text is still streaming', () => {
    expect(source.includes("'message-bubble-enter': shouldPlayEnterAnimation && !isStreaming")).toBe(true);
  });

  test('keeps completed markdown fences expanded while the reply is still streaming', () => {
    expect(source.includes("streamingParts.tailKind === 'code'")).toBe(true);
    const codeBranch = source.slice(
      source.indexOf("streamingParts.tailKind === 'code'"),
      source.indexOf('streamingParts.codeContent')
    );
    expect(codeBranch.includes('isStreaming')).toBe(true);
  });

  test('renders streaming prose through one MarkdownView so tables and lists do not restyle on promotion', () => {
    const streamingBlock = source.match(/<StreamingText[\s\S]*?<\/StreamingText>/)?.[0] ?? '';
    expect(streamingBlock.includes("streamingParts.tailKind === 'code'")).toBe(true);
    expect(streamingBlock.includes("className={`${MESSAGE_BODY_CLASS_NAME} message-streaming-body`}")).toBe(false);
    expect(streamingBlock.includes('{data}')).toBe(true);
  });

  test('does not pretty-rebalance wrap points on the live streaming tail', () => {
    expect(messagesCss.includes('text-wrap: pretty')).toBe(true);
    expect(messagesCss.includes('.message-text-body.message-streaming-body')).toBe(true);
    const prettyIndex = messagesCss.indexOf('text-wrap: pretty');
    const streamingWrapIndex = messagesCss.indexOf('.message-text-body.message-streaming-body');
    expect(streamingWrapIndex).toBeGreaterThan(prettyIndex);
    const streamingWrapRule = messagesCss.slice(
      streamingWrapIndex,
      messagesCss.indexOf('}', streamingWrapIndex) + 1
    );
    expect(streamingWrapRule.includes('text-wrap: wrap')).toBe(true);
    expect(streamingWrapRule.includes('text-wrap: pretty')).toBe(false);
  });

  test('skips JSON probing while the reply is still streaming', () => {
    expect(source.includes('useFormatContent(text, isStreaming)')).toBe(true);
    const formatHook = source.match(/const useFormatContent = [\s\S]*?^};/m)?.[0] ?? '';
    expect(formatHook.includes('if (isStreaming)')).toBe(true);
    expect(formatHook.includes('JSON.parse(content)')).toBe(true);
  });

  test('uses one body typography contract for plain text and markdown text', () => {
    expect(typographySource.includes("export const MESSAGE_BODY_FONT_SIZE = 'var(--conversation-message-font-size)';")).toBe(
      true
    );
    expect(
      typographySource.includes("export const MESSAGE_BODY_LINE_HEIGHT = 'var(--conversation-message-line-height)';")
    ).toBe(true);
    expect(typographySource.includes("export const MESSAGE_BODY_CLASS_NAME = 'message-text-body whitespace-pre-wrap break-words';")).toBe(
      true
    );
    expect(source.includes("from '../typography'")).toBe(true);
    expect(source.includes('className={MESSAGE_BODY_CLASS_NAME}')).toBe(true);
    expect(source.includes('fontSize={MESSAGE_BODY_FONT_SIZE}')).toBe(true);
    expect(source.includes('lineHeight={MESSAGE_BODY_LINE_HEIGHT}')).toBe(true);
  });

  test('keeps the knowledge writeback icon optically centered with the status text', () => {
    expect(source.includes('h-14px w-14px shrink-0 items-center justify-center self-center leading-none')).toBe(true);
    expect(source.includes("className='block shrink-0'")).toBe(true);
  });

  test('offers coding turn rollback on the latest editable user message when available', () => {
    expect(source.includes("data-testid='message-coding-rollback-action'")).toBe(true);
    expect(source.includes('ipcBridge.conversation.codingTurnRollbackAvailability.invoke')).toBe(true);
    expect(source.includes('ipcBridge.conversation.codingTurnRollback.invoke')).toBe(true);
    expect(source.includes("reason: 'coding-rollback'")).toBe(true);
    expect(source.includes("emitter.emit('nomi.workspace.refresh')")).toBe(true);
    expect(source.includes("t('conversation.codingRollback.confirmTitle'")).toBe(true);
  });

  test('offers one explicit retry action only for retryable terminal writeback state', () => {
    expect(source.includes('displayState.retryable === true')).toBe(true);
    expect(source.includes('!RUNNING_WRITEBACK_STATUSES.has(displayState.status)')).toBe(true);
    expect(source.includes('ipcBridge.conversation.retryKnowledgeWriteback.invoke')).toBe(true);
    expect(source.includes('messageId={message.message_id ?? message.msg_id}')).toBe(true);
    expect(source.includes('disabled={retrying}')).toBe(true);
    expect(source.includes("event.stopPropagation();")).toBe(true);
  });

  test('strips memory citation protocol blocks before rendering', () => {
    expect(source.includes("from '@renderer/utils/chat/memCitationFilter'")).toBe(true);
    expect(source.includes('hasMemCitations(content)')).toBe(true);
    expect(source.includes('content = stripMemCitations(content)')).toBe(true);
  });

  test('routes file marker parsing through the message-side trust boundary', () => {
    expect(source.includes("import { parseMessageFileMarker } from './messageFileMarker';")).toBe(true);
    expect(source.includes('parseMessageFileMarker(contentToRender, message.position)')).toBe(true);
    expect(source.includes('const parseFileMarker')).toBe(false);
  });

  test('offers only same-key confirmation continuation while confirming an edit', () => {
    expect(source.includes("import { Alert, Button, Modal, Tooltip } from '@arco-design/web-react';")).toBe(true);
    expect(source.includes("import { AppMessage as Message } from '@/renderer/components/notifications';")).toBe(true);
    expect(source.includes("editingState?.phase === 'confirming' && editingState.continueConfirmation")).toBe(true);
    expect(source.includes('editingState.continueConfirmation?.();')).toBe(true);
    expect(source.includes("t('conversation.editMessage.continueConfirmation')")).toBe(true);
  });
});
