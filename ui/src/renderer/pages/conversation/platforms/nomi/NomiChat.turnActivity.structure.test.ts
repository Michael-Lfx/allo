/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const chatSource = readFileSync(new URL('./NomiChat.tsx', import.meta.url), 'utf8');
const sendBoxSource = readFileSync(new URL('./NomiSendBox.tsx', import.meta.url), 'utf8');
const messageSource = readFileSync(new URL('./useNomiMessage.ts', import.meta.url), 'utf8');

describe('NomiChat turn activity ownership', () => {
  test('shares the local stream lifecycle with the message list and send box', () => {
    expect(chatSource.includes('useNomiMessage(conversation_id')).toBe(true);
    expect(chatSource.includes('turnActivity.running')).toBe(true);
    expect(chatSource.includes('turnActivity.hasHydratedRunningState')).toBe(true);
    expect(chatSource.includes('resolvedIsProcessing')).toBe(true);
    expect(chatSource.includes('!turnActivity.presentation.streamFinished')).toBe(true);
    expect(chatSource.includes('isProcessing === true || turnActivity.running')).toBe(true);
    expect(chatSource.includes('turnActivity={turnActivity}')).toBe(true);
    expect(chatSource.includes('activeTurnId: turnActivity.activeTurnId')).toBe(true);
    expect(chatSource.includes('activeRequestMessageId: turnActivity.activeRequestMessageId')).toBe(true);
    expect(chatSource.includes('TurnStatusRail')).toBe(true);
  });

  test('uses the initial processing snapshot only until live turn state is hydrated', () => {
    expect(
      /const resolvedIsProcessing = turnActivity\.hasHydratedRunningState\s+\? turnActivity\.running\s+: isProcessing === true \|\| turnActivity\.running;/.test(
        chatSource
      )
    ).toBe(true);
  });

  test('does not let the send box own the stream subscription by itself', () => {
    expect(sendBoxSource.includes('useNomiMessage(')).toBe(false);
    expect(sendBoxSource.includes('turnActivity: NomiMessageRuntime')).toBe(true);
  });

  test('keeps local turn activity separate from the canonical user bubble', () => {
    const executeStart = sendBoxSource.indexOf('const executeCommand = useCallback(');
    const post = sendBoxSource.indexOf('sendMessage.invoke({', executeStart);
    const preResponse = sendBoxSource.slice(executeStart, post);

    expect(preResponse.includes('notifyLocalSubmit(id)')).toBe(true);
    expect(preResponse.includes('addOrUpdateMessage(')).toBe(false);
    expect(preResponse.includes('setActiveMsgId(')).toBe(false);
    expect(sendBoxSource.includes('localMsgId')).toBe(false);

    const executeEnd = sendBoxSource.indexOf('const {', executeStart);
    const executeSource = sendBoxSource.slice(executeStart, executeEnd);
    expect(executeSource.includes('removeMessageByMsgId(')).toBe(false);
  });

  test('adopts the server message id before admitting the visible user row', () => {
    const executeStart = sendBoxSource.indexOf('const executeCommand = useCallback(');
    const post = sendBoxSource.indexOf('sendMessage.invoke({', executeStart);
    const fresh = sendBoxSource.indexOf("if (disposition === 'fresh') {", post);
    const accepted = sendBoxSource.indexOf('notifyAccepted(msg_id', fresh);
    const active = sendBoxSource.indexOf('setActiveMsgId(msg_id)', accepted);
    const canonicalRow = sendBoxSource.indexOf('addOrUpdateMessage({', active);

    expect(post > executeStart).toBe(true);
    expect(fresh > post).toBe(true);
    expect(accepted > fresh).toBe(true);
    expect(active > accepted).toBe(true);
    expect(canonicalRow > active).toBe(true);

    const replayStart = sendBoxSource.indexOf('} else {', canonicalRow);
    const replayEnd = sendBoxSource.indexOf("emitter.emit('chat.history.refresh')", replayStart);
    const replaySource = sendBoxSource.slice(replayStart, replayEnd);
    expect(replaySource.includes('reconcilePublicDeliveryReplay(res.completed)')).toBe(true);
    expect(replaySource.includes('addOrUpdateMessage(')).toBe(false);
    expect(replaySource.includes('removeMessageByMsgId(')).toBe(false);

    const catchStart = sendBoxSource.indexOf('} catch (error) {', canonicalRow);
    const catchEnd = sendBoxSource.indexOf('throw error;', catchStart);
    const catchSource = sendBoxSource.slice(catchStart, catchEnd);
    expect(catchSource.includes('addOrUpdateMessage(')).toBe(false);
    expect(catchSource.includes('removeMessageByMsgId(')).toBe(false);
  });

  test('ends visual turn activity on stream terminal events', () => {
    const finishHandler = messageSource.indexOf("case 'finish':");
    const errorHandler = messageSource.indexOf("if (message.type === 'error')");

    expect(messageSource.indexOf("dispatchTurnIfOpen({ type: 'finish' });", finishHandler)).toBeGreaterThan(
      finishHandler
    );
    expect(messageSource.indexOf("dispatchTurnIfOpen({ type: 'error' });", errorHandler)).toBeGreaterThan(
      errorHandler
    );
  });
});
