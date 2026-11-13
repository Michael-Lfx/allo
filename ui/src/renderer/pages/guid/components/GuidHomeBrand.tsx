import { useContext, useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import FlipCard from '@renderer/components/motion/FlipCard';
import { formatSiderAccountLabel } from '@renderer/components/layout/Sider/accountLabel';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { CreditsContext } from '@renderer/hooks/context/CreditsContext';
import { openInAppBilling } from '@renderer/utils/openInAppBilling';
import { ipcBridge } from '@/common';
import type { IMediaCreditsCheckinRecord, IMediaCreditsCheckinRecords } from '@/common/adapter/ipcBridge';
import { getCurrentCronTimeZone } from '@renderer/pages/cron/cronUtils';
import { calendarDays, monthKey, recordDate, zonedDayKey } from '@renderer/pages/billing/billingCreditsModel';
import { membershipDisplayName, shortPlanBadge } from './membershipCardModel';
import layoutStyles from '../index.module.css';
import styles from './GuidHomeCard.module.css';

type RecordsState =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'ready'; data: IMediaCreditsCheckinRecords }
  | { status: 'error' };

type CheckinState = 'guest' | 'checking' | 'signed' | 'loading' | 'ready' | 'unknown';

const WEEKDAY_REFERENCE = new Date(2026, 0, 5);
function Postmark() {
  return (
    <svg className={styles.postmark} viewBox='0 0 46 70' aria-hidden='true'>
      <path d='M4 14H36M1 24H42M6 34H30M2 44H38M8 54H24' />
    </svg>
  );
}

/** Homepage membership card: a full-month check-in calendar with the original
 * FlipCard material and motion. The back is a short message list; detail
 * actions are real buttons outside FlipCard's interactive button shell. */
const GuidHomeBrand: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const { whoami, status } = useCloudAuth();
  const credits = useContext(CreditsContext);
  const slotRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 227, height: 244 });
  const [flipped, setFlipped] = useState(false);
  const [records, setRecords] = useState<RecordsState>({ status: 'idle' });

  useEffect(() => {
    const el = slotRef.current;
    if (!el || typeof ResizeObserver === 'undefined') return;
    const measure = () => {
      const rect = el.getBoundingClientRect();
      const width = Math.max(180, Math.round(rect.width));
      const height = Math.max(180, Math.round(rect.height));
      setSize((current) => (current.width === width && current.height === height ? current : { width, height }));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const checkingAuth = status === 'checking';
  const signedIn = status === 'authenticated' && !!whoami?.authenticated;
  const accountLabel = signedIn
    ? formatSiderAccountLabel({
        nickname: whoami?.nickname,
        username: whoami?.username,
        email: whoami?.email,
      })
    : '';
  const name = signedIn
    ? membershipDisplayName(accountLabel) || t('guid.home.cardGuest')
    : checkingAuth
      ? ''
      : t('guid.home.cardGuest');
  const badge = signedIn ? shortPlanBadge(whoami?.plan, whoami?.planCode) : '';
  const planDisplay = signedIn ? (whoami?.plan?.trim() || badge) : '';
  const locale = i18n.language;

  const now = new Date();
  const timeZone = getCurrentCronTimeZone();
  const localMonth = monthKey(now);
  const fallbackToday = `${localMonth}-${String(now.getDate()).padStart(2, '0')}`;
  const todayDate = zonedDayKey(now, timeZone) ?? fallbackToday;
  const month = todayDate.slice(0, 7);
  const monthLabel = new Date(`${month}-01T12:00:00`).toLocaleDateString(locale, {
    year: 'numeric',
    month: 'long',
  });
  const monthDays = useMemo(() => calendarDays(month), [month]);
  const englishWeekdays = locale.toLowerCase().startsWith('en');
  const weekdayFormatter = useMemo(
    () => new Intl.DateTimeFormat(locale, { weekday: englishWeekdays ? 'short' : 'narrow' }),
    [englishWeekdays, locale],
  );
  const weekdayLabel = (date: Date) => {
    const label = weekdayFormatter.format(date);
    return englishWeekdays ? label.slice(0, 2) : label.replace(/^周/, '');
  };

  useEffect(() => {
    if (!signedIn || !credits?.accountId) {
      setRecords({ status: 'idle' });
      return;
    }
    let cancelled = false;
    setRecords({ status: 'loading' });
    void ipcBridge.media.getCheckinRecords
      .invoke({ month })
      .then((data) => {
        if (!cancelled) setRecords({ status: 'ready', data });
      })
      .catch(() => {
        if (!cancelled) setRecords({ status: 'error' });
      });
    return () => {
      cancelled = true;
    };
  }, [signedIn, credits?.accountId, month, credits?.checkInReceipt, credits?.lastRefreshAt]);

  const list = records.status === 'ready' ? records.data.list : [];
  const byDate = useMemo(
    () => new Map(list.map((record) => [recordDate(record), record])),
    [list],
  );
  const receipt = credits?.checkInReceipt;
  const receiptDate = receipt?.dayKey ? recordDate({ dayKey: receipt.dayKey, date: '' }) : '';
  const receiptToday = Boolean(receipt?.authenticated && receiptDate === todayDate);
  const dayKeyToday = Number(todayDate.replaceAll('-', ''));
  const todayRecord = byDate.get(todayDate);
  const checkedToday = Boolean(todayRecord) || receiptToday || credits?.lastCheckInDayKey === dayKeyToday;
  const todayPoints = todayRecord?.grantedPoints ?? (receiptToday ? receipt?.grantedPoints : undefined);

  const state: CheckinState = !signedIn
    ? 'guest'
    : credits?.isCheckingIn
      ? 'checking'
      : checkedToday
        ? 'signed'
        : records.status === 'idle' || records.status === 'loading'
          ? 'loading'
          : records.status === 'error' || credits?.checkInFailed
            ? 'unknown'
            : 'ready';

  const monthCount = records.status === 'ready' ? records.data.list.length : undefined;
  const balance =
    signedIn && credits?.authenticated && credits.balanceConfirmed
      ? credits.balance.toLocaleString(locale)
      : '—';

  const backTodayValue = (() => {
    if (state === 'signed') {
      return todayPoints && todayPoints > 0
        ? t('guid.home.cardTodayPoints', { points: todayPoints.toLocaleString(locale) })
        : t('guid.home.cardCheckinSigned');
    }
    if (state === 'checking') return t('guid.home.cardCheckinChecking');
    if (state === 'ready') return t('guid.home.cardCheckinReady');
    if (state === 'unknown') return t('guid.home.cardCheckinUnknown');
    if (state === 'guest') return t('guid.home.cardCheckinGuest');
    return t('guid.home.cardCheckinLoading');
  })();

  const monthCountValue = monthCount === undefined ? '—' : `${monthCount.toLocaleString(locale)} ${t('guid.home.cardDaysUnit')}`;

  const handleCardClick = (event: ReactMouseEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement | null)?.closest('button,a')) return;
    setFlipped((value) => !value);
  };

  const handleCardKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement | null)?.closest('button,a')) return;
    if (event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    if (!event.repeat) setFlipped((value) => !value);
  };

  const openCredits = (event: ReactMouseEvent<HTMLButtonElement>) => {
    event.stopPropagation();
    openInAppBilling(navigate, { target: 'checkin' });
  };

  const dayLabel = (date: string, record: IMediaCreditsCheckinRecord | undefined) => {
    const display = new Date(`${date}T12:00:00`).toLocaleDateString(locale, {
      month: 'short',
      day: 'numeric',
    });
    if (record) return t('guid.home.cardDaySigned', { date: display, points: record.grantedPoints.toLocaleString(locale) });
    if (date > todayDate) return t('guid.home.cardDayFuture', { date: display });
    if (date === todayDate) return `${t('guid.home.cardToday')} ${display}`;
    return t('guid.home.cardDayNone', { date: display });
  };

  const front = (
    <div className={styles.front}>
      <div className={styles.frontTop}>
        <span className={styles.month}>{monthLabel}</span>
      </div>
      <div className={styles.weekdays} aria-hidden='true'>
        {Array.from({ length: 7 }, (_, index) => (
          <span key={index}>
            {weekdayLabel(
              new Date(
                WEEKDAY_REFERENCE.getFullYear(),
                WEEKDAY_REFERENCE.getMonth(),
                WEEKDAY_REFERENCE.getDate() + index,
              ),
            )}
          </span>
        ))}
      </div>
      <div
        className={styles.calendar}
        role='grid'
        aria-label={t('guid.home.cardCalendarLabel', { month: monthLabel })}
        data-testid='guid-home-calendar'
      >
        {monthDays.map((date, index) =>
          date ? (
            <div
              key={date}
              className={`${styles.day} ${date === todayDate ? styles.today : ''} ${
                date > todayDate ? styles.future : ''
              } ${byDate.has(date) ? styles.signed : ''}`}
              role='gridcell'
              aria-label={dayLabel(date, byDate.get(date))}
            >
              <span className={styles.dayNumber}>{Number(date.slice(-2))}</span>
              {byDate.has(date) ? <span className={styles.dayCheck} aria-hidden='true'>✓</span> : null}
            </div>
          ) : (
            <span key={`blank-${index}`} className={styles.blankDay} aria-hidden='true' />
          ),
        )}
      </div>
      <Postmark />
    </div>
  );

  const back = (
    <div className={styles.back}>
      <div className={styles.backHead}>{t('guid.home.cardMessages')}</div>
      <div className={styles.messageList}>
        <div className={styles.messageRow}>
          <span>{t('guid.home.cardTodayCheckin')}</span>
          <strong
            className={styles.todayValue}
            role='status'
            aria-live='polite'
            data-testid='guid-home-checkin-status'
          >
            {backTodayValue}
          </strong>
        </div>
        <div className={styles.messageRow}>
          <span>{t('guid.home.cardAvailableCredits')}</span>
          <strong>{balance}</strong>
        </div>
        <div className={styles.messageRow}>
          <span>{t('guid.home.cardMonthCredits')}</span>
          <strong>{monthCountValue}</strong>
        </div>
      </div>
      <div className={styles.backBottom}>
        <div className={styles.identity}>
          <span className={styles.name} title={name}>{name || t('guid.home.cardGuest')}</span>
          {planDisplay ? <span className={styles.badge}>{planDisplay}</span> : null}
        </div>
        {signedIn ? (
          <button
            type='button'
            className={styles.detailButton}
            aria-label={t('guid.home.cardViewCredits')}
            data-testid='guid-home-credits-detail'
            onPointerDown={(event) => event.stopPropagation()}
            onPointerUp={(event) => event.stopPropagation()}
            onClick={openCredits}
          >
            {t('guid.home.cardViewDetails')}
          </button>
        ) : null}
      </div>
    </div>
  );

  return (
    <div
      ref={slotRef}
      className={`${layoutStyles.homeBrand} ${styles.slot}`}
      data-testid='guid-home-brand'
      data-flipped={flipped ? 'true' : 'false'}
      role='group'
      tabIndex={0}
      aria-label={flipped ? t('guid.home.cardFlipToCheckin') : t('guid.home.cardFlipToAccount')}
      onClick={handleCardClick}
      onKeyDown={handleCardKeyDown}
    >
      <FlipCard
        axis='y'
        flipOnClick={false}
        interactive={false}
        draggable={false}
        tilt
        tiltMax={8}
        glare={false}
        glareOpacity={0}
        hoverScale={1.01}
        perspective={1200}
        stiffness={160}
        damping={22}
        width={size.width}
        height={size.height}
        radius={24}
        background='transparent'
        color='#17171a'
        shadow
        shadowColor='var(--flowy-accent)'
        shadowOpacity={0.12}
        flipped={flipped}
        front={front}
        back={back}
      />
    </div>
  );
};

export default GuidHomeBrand;
