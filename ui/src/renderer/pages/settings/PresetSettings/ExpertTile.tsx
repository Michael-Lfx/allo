import React, { useLayoutEffect, useRef, useState } from 'react';
import { displayableExpertAvatar } from './expertCatalogModel';

export const EXPERT_CARD_GRID_COLS = 'repeat(auto-fill, minmax(min(280px, 100%), 1fr))';
const MAX_VISIBLE_TAGS = 3;
const TAG_GAP_PX = 6;
const TAG_CLASS = 'max-w-[160px] shrink-0 truncate rounded-6px bg-fill-2 px-10px py-4px text-12px leading-18px text-t-secondary';
const BADGE_CLASS = 'shrink-0 px-4px text-12px leading-18px text-t-tertiary';

export const ExpertMark: React.FC<{ name: string; avatarUrl?: string; size: 44 | 64 }> = ({
  name,
  avatarUrl,
  size,
}) => <ExpertMarkFace key={avatarUrl?.trim() || 'letter'} name={name} avatarUrl={avatarUrl} size={size} />;

const ExpertMarkFace: React.FC<{ name: string; avatarUrl?: string; size: 44 | 64 }> = ({
  name,
  avatarUrl,
  size,
}) => {
  const [failed, setFailed] = useState(false);
  const src = displayableExpertAvatar(avatarUrl);
  const box = size === 64 ? 'size-64px rounded-16px text-24px' : 'size-44px rounded-12px text-16px';
  return (
    <span
      className={`flex shrink-0 items-center justify-center overflow-hidden bg-fill-2 font-semibold text-t-primary ${box}`}
    >
      {src && !failed ? (
        <img src={src} alt='' className='size-full object-cover' onError={() => setFailed(true)} />
      ) : (
        name.trim().charAt(0) || '?'
      )}
    </span>
  );
};

export type ExpertTileProps = {
  testId: string;
  name: string;
  avatarUrl?: string;
  summary: string;
  tags: string[];
  actionLabel?: string;
  actionPendingLabel?: string;
  actionDoneLabel?: string;
  actionTestId?: string;
  titleTestId?: string;
  summaryTestId?: string;
  actionPending?: boolean;
  actionDone?: boolean;
  footer?: React.ReactNode;
  /** Extra control in the title row, for example a delete action. */
  titleAside?: React.ReactNode;
  className?: string;
  rootRef?: React.Ref<HTMLDivElement>;
  onOpen: () => void;
  onAction?: () => void;
};

const textButtonClass = 'm-0 min-w-0 cursor-pointer border-none bg-transparent p-0 text-left';

const ExpertTile: React.FC<ExpertTileProps> = ({
  testId,
  name,
  avatarUrl,
  summary,
  tags,
  actionLabel,
  actionPendingLabel,
  actionDoneLabel,
  actionTestId,
  titleTestId,
  summaryTestId,
  actionPending = false,
  actionDone = false,
  footer,
  titleAside,
  className,
  rootRef,
  onOpen,
  onAction,
}) => {
  const candidates = tags.slice(0, MAX_VISIBLE_TAGS);
  const tagsKey = tags.join('\n');
  const rowRef = useRef<HTMLDivElement>(null);
  const measureRef = useRef<HTMLDivElement>(null);
  const [shownCount, setShownCount] = useState(candidates.length);
  const splitActions = Boolean(onAction);
  const actionText = actionDone
    ? actionDoneLabel || actionLabel
    : actionPending
      ? actionPendingLabel || actionLabel
      : actionLabel;
  const actionButtonClass = [
    'shrink-0 cursor-pointer rounded-full border-none bg-[var(--color-text-1)] px-10px py-3px text-12px font-semibold leading-18px text-[var(--color-bg-1)] transition-opacity duration-200',
    actionPending || actionDone
      ? 'disabled:cursor-default disabled:opacity-60'
      : 'pointer-events-none opacity-0 group-hover:pointer-events-auto group-hover:opacity-100 group-focus-within:pointer-events-auto group-focus-within:opacity-100',
  ].join(' ');

  useLayoutEffect(() => {
    const row = rowRef.current;
    const measure = measureRef.current;
    if (!row || !measure) return;
    const fit = () => {
      const tagWidths = Array.from(measure.querySelectorAll<HTMLElement>('[data-measure-tag]'), (el) => el.offsetWidth);
      const badgeWidth = measure.querySelector<HTMLElement>('[data-measure-badge]')?.offsetWidth ?? 0;
      const widthOf = (count: number) => {
        const parts = tagWidths.slice(0, count);
        if (count < tags.length) parts.push(badgeWidth);
        return parts.reduce((sum, width) => sum + width, 0) + TAG_GAP_PX * Math.max(parts.length - 1, 0);
      };
      let count = candidates.length;
      while (count > 0 && widthOf(count) > row.clientWidth) count -= 1;
      setShownCount(count);
    };
    const observer = new ResizeObserver(fit);
    observer.observe(row);
    fit();
    return () => observer.disconnect();
  }, [tagsKey]);

  return (
    <div
      role={splitActions ? undefined : 'button'}
      tabIndex={splitActions ? undefined : 0}
      data-testid={testId}
      onClick={splitActions ? undefined : onOpen}
      onKeyDown={
        splitActions
          ? undefined
          : (event) => {
              if (event.target !== event.currentTarget) return;
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                onOpen();
              }
            }
      }
      ref={rootRef}
      className={[
        'group flex cursor-pointer flex-col gap-12px rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--bg-base)] p-16px text-left transition-[background-color,box-shadow,transform] duration-200 ease-out hover:-translate-y-[1px] hover:bg-[var(--color-fill-1)] hover:shadow-[0_10px_24px_-18px_rgba(0,0,0,0.25)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgba(var(--primary-6),0.28)]',
        className,
      ].filter(Boolean).join(' ')}
    >
      <div className='flex min-w-0 items-start gap-12px'>
        <ExpertMark name={name} avatarUrl={avatarUrl} size={44} />
        <div className='min-w-0 flex-1'>
          <div className='flex min-w-0 items-center gap-8px'>
            {splitActions ? (
              <button
                type='button'
                data-testid={titleTestId}
                className={`${textButtonClass} flex-1 truncate text-15px font-semibold leading-22px text-t-primary`}
                onClick={onOpen}
              >
                {name}
              </button>
            ) : (
              <span className='min-w-0 flex-1 truncate text-15px font-semibold leading-22px text-t-primary'>{name}</span>
            )}
            {titleAside}
            {actionLabel && onAction ? (
              <button
                type='button'
                data-testid={actionTestId}
                disabled={actionPending || actionDone}
                aria-busy={actionPending}
                className={actionButtonClass}
                onClick={onAction}
              >
                {actionText}
              </button>
            ) : actionLabel ? (
              <span
                aria-hidden='true'
                className='shrink-0 rounded-full bg-[var(--color-text-1)] px-10px py-3px text-12px font-semibold leading-18px text-[var(--color-bg-1)] opacity-0 transition-opacity duration-200 group-hover:opacity-100'
              >
                {actionLabel}
              </span>
            ) : null}
          </div>
          {summary ? (
            splitActions ? (
              <button
                type='button'
                data-testid={summaryTestId}
                className={`${textButtonClass} mt-4px block w-full truncate text-13px leading-20px text-t-secondary`}
                onClick={onOpen}
              >
                {summary}
              </button>
            ) : (
              <p className='m-0 mt-4px truncate text-13px leading-20px text-t-secondary'>{summary}</p>
            )
          ) : null}
        </div>
      </div>
      {tags.length > 0 ? (
        <div ref={rowRef} className='relative flex h-24px items-center gap-6px overflow-hidden'>
          {candidates.slice(0, shownCount).map((tag) => (
            <span key={tag} title={tag} className={TAG_CLASS}>
              {tag}
            </span>
          ))}
          {tags.length > shownCount ? <span className={BADGE_CLASS}>+{tags.length - shownCount}</span> : null}
          <div
            ref={measureRef}
            aria-hidden='true'
            className='invisible pointer-events-none absolute left-0 top-0 flex gap-6px whitespace-nowrap'
          >
            {candidates.map((tag) => (
              <span key={tag} data-measure-tag className={TAG_CLASS}>
                {tag}
              </span>
            ))}
            <span data-measure-badge className={BADGE_CLASS}>
              +{tags.length}
            </span>
          </div>
        </div>
      ) : null}
      {footer}
    </div>
  );
};

export default ExpertTile;
