import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { MarkdownHeading } from './markdownHeadings';

const STRETCH_MS = 280;
const SECTION_ID_PATTERN = /^mdsec[a-z0-9]+-\d+$/i;

interface MarkdownSectionRailProps {
  containerRef: React.RefObject<HTMLDivElement | null>;
  title?: string;
  items: MarkdownHeading[];
}

function dotSize(distance: number, expanded: boolean): { width: number; height: number } {
  const base = distance === 0 ? 8 : distance === 1 ? 6 : distance === 2 ? 5 : 4;
  if (!expanded) return { width: base, height: base };
  const height = distance === 0 ? 6 : 4;
  const width = distance === 0 ? 12 : 9;
  return { width, height };
}

function sectionNode(container: HTMLElement, id: string): HTMLElement | null {
  if (!SECTION_ID_PATTERN.test(id)) return null;
  return container.querySelector<HTMLElement>(`[data-md-section="${id}"]`);
}

function measureActive(container: HTMLElement, items: MarkdownHeading[]): string {
  const maxScroll = container.scrollHeight - container.clientHeight;
  if (maxScroll > 0 && container.scrollTop >= maxScroll - 2) {
    return items[items.length - 1]?.id ?? '';
  }

  const marker = container.getBoundingClientRect().top + 32;
  let current = items[0]?.id ?? '';
  for (const item of items) {
    const node = sectionNode(container, item.id);
    if (!node) continue;
    if (node.getBoundingClientRect().top <= marker) current = item.id;
  }
  return current;
}

const MarkdownSectionRail: React.FC<MarkdownSectionRailProps> = ({ containerRef, title, items }) => {
  const { t } = useTranslation();
  const [activeId, setActiveId] = useState(items[0]?.id ?? '');
  const [hovering, setHovering] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const pinnedIdRef = useRef<string | null>(null);
  const pinnedTopRef = useRef<number | null>(null);
  const pinDistanceRef = useRef<number | null>(null);

  useEffect(() => {
    if (!items.some((item) => item.id === activeId)) {
      pinnedIdRef.current = null;
      pinnedTopRef.current = null;
      pinDistanceRef.current = null;
      setActiveId(items[0]?.id ?? '');
    }
  }, [activeId, items]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const update = () => {
      const pinnedId = pinnedIdRef.current;
      const pinnedTop = pinnedTopRef.current;
      if (pinnedId && pinnedTop != null) {
        const distance = Math.abs(container.scrollTop - pinnedTop);
        const previousDistance = pinDistanceRef.current;
        if (previousDistance != null && distance > previousDistance + 8) {
          pinnedIdRef.current = null;
          pinnedTopRef.current = null;
          pinDistanceRef.current = null;
        } else {
          pinDistanceRef.current = distance;
          setActiveId((prev) => (prev === pinnedId ? prev : pinnedId));
          if (distance <= 2) {
            pinnedIdRef.current = null;
            pinnedTopRef.current = null;
            pinDistanceRef.current = null;
          }
          return;
        }
      }

      const next = measureActive(container, items);
      setActiveId((prev) => (prev === next ? prev : next));
    };

    const releasePin = () => {
      pinnedIdRef.current = null;
      pinnedTopRef.current = null;
      pinDistanceRef.current = null;
    };

    update();
    container.addEventListener('scroll', update, { passive: true });
    container.addEventListener('wheel', releasePin, { passive: true });
    container.addEventListener('touchmove', releasePin, { passive: true });
    container.addEventListener('pointerdown', releasePin);
    return () => {
      container.removeEventListener('scroll', update);
      container.removeEventListener('wheel', releasePin);
      container.removeEventListener('touchmove', releasePin);
      container.removeEventListener('pointerdown', releasePin);
    };
  }, [containerRef, items]);

  useEffect(() => {
    if (!hovering) {
      setMenuOpen(false);
      return;
    }
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (reduceMotion) {
      setMenuOpen(true);
      return;
    }
    const timer = window.setTimeout(() => setMenuOpen(true), STRETCH_MS);
    return () => window.clearTimeout(timer);
  }, [hovering]);

  const reduceMotion =
    typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  const jumpTo = (id: string) => {
    const container = containerRef.current;
    const node = container ? sectionNode(container, id) : null;
    if (!container || !node) return;

    const rawTop =
      node.getBoundingClientRect().top - container.getBoundingClientRect().top + container.scrollTop - 16;
    const maxScroll = Math.max(0, container.scrollHeight - container.clientHeight);
    const top = Math.min(Math.max(0, rawTop), maxScroll);
    pinnedIdRef.current = id;
    pinnedTopRef.current = top;
    pinDistanceRef.current = Math.abs(container.scrollTop - top);
    setActiveId(id);
    container.scrollTo({ top, behavior: reduceMotion ? 'auto' : 'smooth' });
  };

  const activeIndex = Math.max(
    0,
    items.findIndex((item) => item.id === activeId)
  );

  return (
    <div className='pointer-events-none absolute bottom-0 right-18px top-0 z-20 flex items-center justify-end'>
      <div
        className='pointer-events-auto relative'
        onMouseEnter={() => setHovering(true)}
        onMouseLeave={() => setHovering(false)}
      >
        {menuOpen && (
          <div
            role='menu'
            aria-label={t('preview.sectionMenu')}
            className='absolute right-0 top-[calc(50%+36px)] z-10 max-h-[min(280px,46vh)] w-232px -translate-y-1/2 overflow-y-auto rounded-12px border border-solid border-[var(--border-base)] bg-dialog-fill-0 py-8px shadow-[0_8px_28px_color-mix(in_srgb,var(--color-text-1)_12%,transparent)]'
          >
            {title && (
              <div className='truncate px-12px pb-6px text-12px text-t-secondary' title={title}>
                {title}
              </div>
            )}
            {items.map((item) => {
              const selected = item.id === activeId;
              return (
                <button
                  key={item.id}
                  type='button'
                  role='menuitem'
                  className={`mx-6px block w-[calc(100%-12px)] cursor-pointer truncate rounded-8px border-none px-10px py-7px text-left text-13px text-t-primary ${
                    selected ? 'bg-fill-2' : 'bg-transparent hover:bg-fill-2'
                  }`}
                  onClick={() => jumpTo(item.id)}
                >
                  {item.text}
                </button>
              );
            })}
          </div>
        )}

        <div
          className='flex flex-col items-end py-4px'
          style={{ gap: 2 }}
          role='navigation'
          aria-label={t('preview.sectionRailAria', { count: items.length })}
        >
          {items.map((item, index) => {
            const distance = Math.abs(index - activeIndex);
            const selected = item.id === activeId;
            const size = dotSize(distance, hovering);
            return (
              <button
                key={item.id}
                type='button'
                aria-current={selected ? 'true' : undefined}
                aria-label={t('preview.jumpToSection', { index: index + 1, title: item.text })}
                className='flex cursor-pointer items-center justify-end border-none bg-transparent p-0'
                style={{ width: Math.max(12, size.width), height: 8 }}
                onClick={() => jumpTo(item.id)}
              >
                <span
                  style={{
                    width: size.width,
                    height: size.height,
                    borderRadius: 999,
                    background: selected
                      ? 'var(--color-text-1)'
                      : 'color-mix(in srgb, var(--color-text-1) 34%, transparent)',
                    transition: reduceMotion
                      ? 'none'
                      : 'width 280ms ease, height 280ms ease, background-color 280ms ease',
                  }}
                />
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
};

export default MarkdownSectionRail;
