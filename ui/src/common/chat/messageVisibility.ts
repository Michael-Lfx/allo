/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { IMessageText, TMessage } from './chatLib';

/** The durable identity shared by live events, send responses, and DB rows. */
export const getMessageBusinessIdentity = (
  message: Pick<TMessage, 'message_id' | 'msg_id'>
) => message.message_id ?? message.msg_id;

/**
 * The single visibility rule for user text that can become a rendered turn.
 * Blank transport rows remain useful to the runtime, but are not user-visible.
 */
export const isVisibleUserTextMessage = (message: TMessage): message is IMessageText =>
  message.type === 'text' &&
  message.position === 'right' &&
  message.hidden !== true &&
  typeof message.content?.content === 'string' &&
  message.content.content.trim().length > 0;

/** Visible non-empty text, including assistant rows used for turn previews. */
export const isVisibleTextMessage = (message: TMessage): message is IMessageText =>
  message.type === 'text' &&
  message.hidden !== true &&
  typeof message.content?.content === 'string' &&
  message.content.content.trim().length > 0;
