/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import CollapsibleContent from '@renderer/components/chat/CollapsibleContent';
import { shouldCollapseContent } from '@renderer/components/chat/collapsibleContentModel';

const BLOCKQUOTE_MAX_HEIGHT = 200;
const COLLAPSIBLE_BODY_SELECTOR = '[data-testid="collapsible-content-body"]';

type CollapsibleBlockquoteProps = React.PropsWithChildren<React.JSX.IntrinsicElements['blockquote']>;

const getMeasurementElement = (blockquote: HTMLQuoteElement): HTMLElement =>
  blockquote.querySelector<HTMLElement>(COLLAPSIBLE_BODY_SELECTOR) ?? blockquote;

/**
 * Keep short quotes on their original DOM path. A collapsible body is mounted
 * only after the quote itself has been measured above the UX threshold.
 */
const CollapsibleBlockquote: React.FC<CollapsibleBlockquoteProps> = ({ children, ...rest }) => {
  const blockquoteRef = useRef<HTMLQuoteElement>(null);
  const [needsCollapse, setNeedsCollapse] = useState<boolean | null>(null);

  const measure = useCallback(() => {
    const blockquote = blockquoteRef.current;
    if (!blockquote) return;

    const element = getMeasurementElement(blockquote);
    const nextNeedsCollapse = shouldCollapseContent(element.scrollHeight, BLOCKQUOTE_MAX_HEIGHT);
    setNeedsCollapse((value) => (value === nextNeedsCollapse ? value : nextNeedsCollapse));
  }, []);

  // Measure before paint so a long quote does not first flash in its expanded
  // form, while a short quote keeps the direct blockquote -> paragraph tree.
  useLayoutEffect(() => {
    measure();
  }, [measure]);

  useEffect(() => {
    // Once the wrapper exists, CollapsibleContent owns the natural-height
    // measurement and its own cleanup. The direct quote path only needs an
    // observer while deciding whether a wrapper is necessary.
    if (needsCollapse === true) return undefined;

    const blockquote = blockquoteRef.current;
    if (!blockquote || typeof ResizeObserver === 'undefined') return undefined;

    let frameId: number | null = null;
    let disposed = false;

    const update = () => {
      frameId = null;
      if (disposed || blockquoteRef.current !== blockquote) return;
      measure();
    };

    const schedule = () => {
      if (disposed) return;
      if (frameId !== null) window.cancelAnimationFrame(frameId);
      frameId = window.requestAnimationFrame(update);
    };

    const observer = new ResizeObserver(schedule);
    observer.observe(blockquote);
    schedule();

    return () => {
      disposed = true;
      if (frameId !== null) window.cancelAnimationFrame(frameId);
      observer.disconnect();
    };
  }, [measure, needsCollapse]);

  return (
    <blockquote {...rest} ref={blockquoteRef}>
      {needsCollapse ? (
        <CollapsibleContent
          maxHeight={BLOCKQUOTE_MAX_HEIGHT}
          defaultCollapsed
          useMask
          className='markdown-blockquote-collapsible'
        >
          {children}
        </CollapsibleContent>
      ) : (
        children
      )}
    </blockquote>
  );
};

export default CollapsibleBlockquote;
