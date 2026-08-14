import type { TMessage } from '@/common/chat/chatLib';
import type { MessageId } from '@/common/types/ids';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { emitter } from '@/renderer/utils/emitter';
import SelectionActions from '@renderer/components/beautifulUi/selectionActions/SelectionActions';
import { selectionActionLabelKey } from '@renderer/components/beautifulUi/selectionActions/selectionActionsModel';
import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './SelectionReplyButton.module.css';

type ReplyPos = { top: number; left: number; text: string; messageId: MessageId; msgPos: string };

/**
 * Get the current selection, checking Shadow DOM roots if needed.
 * MarkdownView renders inside Shadow DOM, so document.getSelection() may
 * return a collapsed/empty selection while the real selection lives inside
 * a shadowRoot.
 */
function getEffectiveSelection(target: EventTarget | null): Selection | null {
  // First try the standard selection
  const docSel = document.getSelection();
  if (docSel && !docSel.isCollapsed && docSel.toString().trim()) {
    return docSel;
  }

  // If standard selection is empty, search for selection inside Shadow DOM
  // Walk up from the mouseup target to find a shadow host
  let el = target instanceof Node ? target : null;
  while (el) {
    if (el instanceof Element && el.shadowRoot) {
      const shadowSel = (el.shadowRoot as unknown as { getSelection?: () => Selection | null }).getSelection?.();
      if (shadowSel && !shadowSel.isCollapsed && shadowSel.toString().trim()) {
        return shadowSel;
      }
    }
    el = el.parentNode;
  }

  return docSel;
}

/**
 * Find the closest message container from a selection's anchor node.
 * Handles both regular DOM and Shadow DOM cases.
 */
function findMessageElement(sel: Selection): Element | null {
  let node: Node | null = sel.anchorNode;
  if (!node) return null;

  // In Shadow DOM, anchorNode is inside the shadow tree.
  // We need to walk up through shadow boundaries to find the message container.
  let el: Element | null = node instanceof Element ? node : node.parentElement;

  // Try finding within current DOM tree first
  const msgEl = el?.closest?.('[id^="message-"]');
  if (msgEl) return msgEl;

  // If not found, walk up through shadow host boundaries
  while (el) {
    const root = el.getRootNode();
    if (root instanceof ShadowRoot) {
      el = root.host;
      const hostMsgEl = el.closest('[id^="message-"]');
      if (hostMsgEl) return hostMsgEl;
    } else {
      break;
    }
  }

  return null;
}

const BUTTON_HEIGHT = 36;

const SelectionReplyButton: React.FC<{ messages: TMessage[] }> = ({ messages }) => {
  const { t } = useTranslation();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const [pos, setPos] = useState<ReplyPos | null>(null);
  const messagesRef = useRef(messages);
  messagesRef.current = messages;
  const buttonRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // Disable on mobile — conflicts with native text selection menu
    if (isMobile) return;

    let mounted = true;
    let scrollTimer: ReturnType<typeof setTimeout> | null = null;

    const onMouseUp = (e: MouseEvent) => {
      // Skip if mouseup is on the reply button itself
      if (buttonRef.current?.contains(e.target as Node)) return;

      window.setTimeout(() => {
        if (!mounted) return;

        const sel = getEffectiveSelection(e.target);
        if (!sel || sel.isCollapsed) return;
        const text = sel.toString().trim();
        if (!text) return;

        const msgEl = findMessageElement(sel);
        if (!msgEl) return;

        const messageId = msgEl.getAttribute('data-message-business-id');
        if (!messageId) return;
        const msg = messagesRef.current.find(
          (message) => (message.message_id ?? message.msg_id) === messageId
        );
        const businessMessageId = msg?.message_id ?? msg?.msg_id;
        if (!businessMessageId) return;
        const rect = sel.getRangeAt(0).getBoundingClientRect();

        // Place above selection; if too close to top, place below
        const above = rect.top - BUTTON_HEIGHT - 8;
        const below = rect.bottom + 8;
        const top = above >= 0 ? above : below;

        setPos({
          top,
          left: Math.max(60, Math.min(rect.left + rect.width / 2, window.innerWidth - 60)),
          text,
          messageId: businessMessageId,
          msgPos: msg?.position ?? 'left',
        });
      }, 20);
    };

    const onMouseDown = (e: MouseEvent) => {
      if (buttonRef.current?.contains(e.target as Node)) return;
      setPos(null);
    };

    // Clear floating button on scroll, debounced to avoid clearing during
    // the tiny scrolls that happen while selecting text
    const onScroll = () => {
      if (scrollTimer) clearTimeout(scrollTimer);
      scrollTimer = setTimeout(() => setPos(null), 100);
    };

    document.addEventListener('mouseup', onMouseUp);
    document.addEventListener('mousedown', onMouseDown);
    document.addEventListener('scroll', onScroll, true);
    return () => {
      mounted = false;
      if (scrollTimer) clearTimeout(scrollTimer);
      document.removeEventListener('mouseup', onMouseUp);
      document.removeEventListener('mousedown', onMouseDown);
      document.removeEventListener('scroll', onScroll, true);
    };
  }, [isMobile]);

  if (!pos) return null;

  const handleQuote = () => {
    emitter.emit('sendbox.reply', {
      messageId: pos.messageId,
      content: pos.text,
      position: pos.msgPos as 'left' | 'right' | 'center' | 'pop',
    });
    setPos(null);
    window.getSelection()?.removeAllRanges();
  };

  return (
    <div ref={buttonRef} className={styles.host}>
      <SelectionActions
        top={pos.top}
        left={pos.left}
        actions={[
          {
            id: 'quote',
            label: t(selectionActionLabelKey('quote'), { defaultValue: 'Reply' }),
            onClick: handleQuote,
          },
        ]}
      />
    </div>
  );
};

export default SelectionReplyButton;
