import React, { useEffect, useRef, useState } from 'react';
import { motion, useReducedMotion } from 'motion/react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@/common';
import type { IMediaCreditsCheckinRecord, IMediaCreditsCheckinRecords, IMediaCreditsUsageByType } from '@/common/adapter/ipcBridge';
import { useCredits } from '@renderer/hooks/context/CreditsContext';
import { getCurrentCronTimeZone } from '@renderer/pages/cron/cronUtils';
import { calendarDays, expiringBuckets, monthKey, recordDate, zonedDayKey } from './billingCreditsModel';

type ReadState<T> = { status: 'loading' } | { status: 'ready'; data: T } | { status: 'error' };

export default function BillingCreditsPanel({ onPurchase, onPacks, onReady }: { onPurchase: () => void; onPacks: () => void; onReady: () => void }) {
  const { t, i18n } = useTranslation();
  const credits = useCredits();
  const reduced = useReducedMotion();
  const [usage, setUsage] = useState<ReadState<IMediaCreditsUsageByType>>({ status: 'loading' });
  const [serverDate, setServerDate] = useState('');
  // The check-in day is resolved in the same IANA time zone the check-in
  // request posts, never by slicing the UTC timestamp or the browser clock.
  const timeZone = getCurrentCronTimeZone();
  const now = new Date();
  const localDate = zonedDayKey(now, timeZone) ?? `${monthKey(now)}-${String(now.getDate()).padStart(2, '0')}`;
  const todayDate = /^\d{4}-\d{2}-\d{2}$/.test(serverDate) ? serverDate : localDate;
  const currentMonth = todayDate.slice(0, 7);
  const [month, setMonth] = useState(currentMonth);
  const [selected, setSelected] = useState(todayDate);
  const [records, setRecords] = useState<ReadState<IMediaCreditsCheckinRecords>>({ status: 'loading' });
  const [todayHistory, setTodayHistory] = useState<ReadState<IMediaCreditsCheckinRecord | null>>({ status: 'loading' });
  const [retry, setRetry] = useState(0);
  const [celebrate, setCelebrate] = useState(false);
  const receiptRef = useRef(credits.checkInReceipt);
  const initialMonthRef = useRef(currentMonth);
  useEffect(() => {
    if (currentMonth === initialMonthRef.current) return;
    const previousMonth = initialMonthRef.current;
    setMonth((value) => value === previousMonth ? currentMonth : value);
    setSelected((value) => value === localDate ? todayDate : value);
    initialMonthRef.current = currentMonth;
  }, [currentMonth, localDate, todayDate]);
  useEffect(() => {
    if (records.status !== 'loading' && usage.status !== 'loading') onReady();
  }, [records.status, usage.status, onReady]);
  const number = (n: number) => n.toLocaleString(i18n.language);
  const time = (value: string) => new Date(value).toLocaleString(i18n.language);

  useEffect(() => {
    let cancelled = false;
    setRecords({ status: 'loading' });
    const request = ipcBridge.media.getCheckinRecords.invoke({ month });
    void request.then((data) => {
      if (!cancelled) setRecords(data.month === month ? { status: 'ready', data } : { status: 'error' });
    }).catch(() => { if (!cancelled) setRecords({ status: 'error' }); });
    const todayRequest = month === currentMonth ? request : ipcBridge.media.getCheckinRecords.invoke({ month: currentMonth });
    void todayRequest.then((data) => {
      if (!cancelled) setTodayHistory(data.month === currentMonth ? { status: 'ready', data: data.list.find((record) => recordDate(record) === todayDate) ?? null } : { status: 'error' });
    }).catch(() => { if (!cancelled) setTodayHistory({ status: 'error' }); });
    return () => { cancelled = true; };
  }, [month, currentMonth, todayDate, retry, credits.accountId, credits.lastRefreshAt, credits.checkInReceipt]);

  useEffect(() => {
    let cancelled = false;
    setUsage({ status: 'loading' });
    void ipcBridge.media.getCreditsUsageByType.invoke().then((data) => {
      if (!cancelled) {
        setUsage({ status: 'ready', data });
        setServerDate(zonedDayKey(data.serverTime, timeZone) ?? localDate);
      }
    }).catch(() => { if (!cancelled) setUsage({ status: 'error' }); });
    return () => { cancelled = true; };
  }, [retry, credits.accountId, credits.lastRefreshAt, credits.checkInReceipt]);

  useEffect(() => {
    const receipt = credits.checkInReceipt;
    const fresh = receipt !== receiptRef.current && receipt?.authenticated && !receipt.alreadyCheckedIn && receipt.grantedPoints > 0;
    receiptRef.current = receipt;
    if (!fresh || reduced) return;
    setCelebrate(true);
    const timer = setTimeout(() => setCelebrate(false), 700);
    return () => { clearTimeout(timer); setCelebrate(false); };
  }, [credits.checkInReceipt, reduced]);

  const list = records.status === 'ready' ? records.data.list : [];
  const byDate = new Map(list.map((record) => [recordDate(record), record]));
  const receipt = credits.checkInReceipt;
  const receiptDate = receipt?.dayKey ? recordDate({ dayKey: receipt.dayKey, date: '' }) : '';
  const todayRecord = todayHistory.status === 'ready' ? todayHistory.data : null;
  const todayReceipt = receipt?.authenticated && receiptDate === todayDate && !receipt.alreadyCheckedIn ? receipt : null;
  const confirmed = todayRecord ?? todayReceipt;
  const selectedRecord = byDate.get(selected);
  const sourceLabel = (type: string) => t(`billing.panel.source.${type as 'DAILY_CHECKIN' | 'PLAN' | 'PACK' | 'SIGNUP' | 'TEAM_SEAT' | 'OTHER'}`, { defaultValue: t('billing.panel.source.OTHER') });
  const changeMonth = (delta: number) => {
    const [year, index] = month.split('-').map(Number);
    const next = monthKey(new Date(year, index - 1 + delta, 1));
    setMonth(next); setSelected(`${next}-01`);
  };
  const retryRead = () => setRetry((value) => value + 1);
  const allBuckets = usage.status === 'ready' ? expiringBuckets(usage.data) : [];
  const buckets = allBuckets.slice(0, 3);

  return <section className='billing-credits' aria-label={t('billing.panel.entry')}>
    <div className='billing-balance-row'>
      <div className='billing-balance-info'>
        <span className='billing-muted'>{t('billing.panel.balance')}</span>
        <strong className='billing-balance'>{credits.authenticated && credits.balanceConfirmed ? number(credits.balance) : t('billing.panel.unconfirmed')}</strong>
        <button className='billing-btn billing-btn-text' disabled={!credits.canRefresh || credits.isCheckingIn} onClick={() => { credits.manualRefresh(); retryRead(); }}>{t('billing.panel.refresh')}</button>
      </div>
      <div className='billing-credit-actions'>
        <button className='billing-btn billing-btn-primary' onClick={onPurchase}>{t('billing.panel.buyPlan')}</button>
        <button className='billing-btn billing-btn-ghost' onClick={onPacks}>{t('billing.panel.buyPack')}</button>
      </div>
    </div>
    <motion.div className={`billing-checkin-status ${confirmed ? 'is-confirmed' : ''}`} role='status' animate={celebrate ? { scale: [1, 1.008, 1] } : { scale: 1 }} transition={{ duration: .35 }}>
      <div><strong>{confirmed ? t('billing.panel.arrived', { points: number(confirmed.grantedPoints) }) : credits.isCheckingIn ? t('billing.panel.checking') : credits.checkInFailed ? t('billing.panel.checkinFailed') : todayHistory.status === 'loading' ? t('billing.panel.confirming') : todayHistory.status === 'error' ? t('billing.panel.recordsError') : t('billing.panel.notChecked')}</strong>{confirmed?.checkInAt && <span className='billing-muted'>{new Date(confirmed.checkInAt).toLocaleTimeString(i18n.language, { hour: '2-digit', minute: '2-digit' })}</span>}</div>
      {!confirmed && !credits.isCheckingIn && <button className='billing-btn billing-btn-ghost' onClick={() => { void credits.checkIn().then(() => { void credits.fetchBalance(); retryRead(); }); }}>{t('billing.panel.retryCheckin')}</button>}
    </motion.div>
    <div className='billing-credits-grid'>
      <section id='billing-checkin' tabIndex={-1} className='billing-calendar-section' aria-labelledby='billing-checkin-title'>
        <div className='billing-section-head'><h2 id='billing-checkin-title'>{t('billing.panel.calendar')}</h2><span className='billing-muted'>{records.status === 'ready' ? t('billing.panel.counts', { month: list.length, total: records.data.totalCheckInDays }) : t('billing.panel.unconfirmed')}</span></div>
        <div className='billing-month-controls'><button className='billing-btn billing-btn-ghost' aria-label={t('billing.panel.prevMonth')} onClick={() => changeMonth(-1)}>‹</button><strong>{new Date(`${month}-01T12:00:00`).toLocaleDateString(i18n.language, { year: 'numeric', month: 'long' })}</strong><button className='billing-btn billing-btn-ghost' aria-label={t('billing.panel.nextMonth')} disabled={month >= currentMonth} onClick={() => changeMonth(1)}>›</button>{month !== currentMonth && <button className='billing-btn billing-btn-ghost' onClick={() => { setMonth(currentMonth); setSelected(todayDate); }}>{t('billing.panel.thisMonth')}</button>}</div>
        <div className='billing-calendar-frame' aria-busy={records.status === 'loading'}>
          {records.status === 'loading' ? <p className='billing-muted' role='status'>{t('billing.panel.loading')}</p> : records.status === 'error' ? <div className='billing-read-error'><p>{t('billing.panel.recordsError')}</p><button className='billing-btn billing-btn-ghost' onClick={retryRead}>{t('billing.catalog.retry')}</button></div> : <motion.div key={month} initial={reduced ? false : { opacity: 0, y: 4 }} animate={{ opacity: 1, y: 0 }} transition={{ duration: .16, ease: 'easeOut' }}>
            <div className='billing-calendar-weekdays' aria-hidden='true'>{Array.from({ length: 7 }, (_, index) => <span key={index}>{new Date(2026, 0, 5 + index).toLocaleDateString(i18n.language, { weekday: 'short' })}</span>)}</div>
            <div className='billing-calendar-days' role='group' aria-label={t('billing.panel.calendar')}>{calendarDays(month).map((date, index) => date ? <button key={date} type='button' className={`billing-calendar-day ${date === todayDate ? 'is-today' : ''} ${date === todayDate && celebrate ? 'is-fresh' : ''} ${date > todayDate ? 'is-future' : ''} ${date === selected ? 'is-selected' : ''} ${byDate.has(date) ? 'is-signed' : ''}`} aria-pressed={date === selected} aria-current={date === todayDate ? 'date' : undefined} aria-label={`${date} · ${byDate.has(date) ? t('billing.panel.arrived', { points: number(byDate.get(date)!.grantedPoints) }) : date > todayDate ? t('billing.panel.futureDay') : t('billing.panel.noRecord')}`} onClick={() => setSelected(date)} onKeyDown={(event) => {
              const delta = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 }[event.key as 'ArrowLeft'];
              if (delta === undefined) return;
              event.preventDefault();
              const days = calendarDays(month).filter((day): day is string => day !== null);
              const next = days[Math.max(0, Math.min(days.length - 1, days.indexOf(date) + delta))];
              setSelected(next);
              const buttons = event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('button');
              buttons?.[days.indexOf(next)]?.focus();
            }}><span>{Number(date.slice(-2))}</span>{byDate.has(date) && <><span className='billing-day-check' aria-hidden='true'>✓</span><small>+{number(byDate.get(date)!.grantedPoints)}</small></>}</button> : <span key={`blank-${index}`} aria-hidden='true' />)}</div>
          </motion.div>}
        </div>
        {records.status === 'ready' && !list.length && <p className='billing-muted'>{t('billing.panel.empty')}</p>}
        <p className='billing-day-detail' aria-live='polite'>{selected} · {records.status !== 'ready' ? t('billing.panel.unconfirmed') : selectedRecord ? t('billing.panel.recordDetail', { points: number(selectedRecord.grantedPoints), time: time(selectedRecord.checkInAt) }) : selected > todayDate ? t('billing.panel.futureDay') : t('billing.panel.noRecord')}</p>
      </section>
      <aside className='billing-credit-rules'>
        <h2>{t('billing.panel.expiry')}</h2>
        {usage.status === 'loading' ? <p className='billing-muted'>{t('billing.panel.loading')}</p> : usage.status === 'error' ? <div className='billing-read-error'><p>{t('billing.panel.usageError')}</p><button className='billing-btn billing-btn-ghost' onClick={retryRead}>{t('billing.catalog.retry')}</button></div> : <>{buckets.map((bucket, index) => <div className='billing-source-row' key={`${bucket.type}-${bucket.expireAt}-${index}`}><div>{time(bucket.expireAt!)}<small className='billing-muted'>{sourceLabel(bucket.type)}</small></div><strong>{number(bucket.remaining)}</strong></div>)}{!buckets.length && <p className='billing-muted'>{t('billing.panel.noExpiry')}</p>}<details className='billing-rules-disclosure'><summary>{t('billing.panel.sources')}</summary>{allBuckets.length ? allBuckets.map((bucket, index) => <div className='billing-source-row' key={`${bucket.type}-${bucket.expireAt}-${index}`}><div>{time(bucket.expireAt!)}<small className='billing-muted'>{sourceLabel(bucket.type)}</small></div><strong>{number(bucket.remaining)}</strong></div>) : <p className='billing-muted'>{t('billing.panel.noExpiry')}</p>}</details></>}
        <details className='billing-rules-disclosure'>
          <summary>{t('billing.panel.rules')}</summary>
          <dl className='billing-validity-rules'>
            <div><dt>{t('billing.panel.freeCredits')}</dt><dd>{t('billing.panel.sameDay')}</dd></div>
            <div><dt>{t('billing.panel.paidCredits')}</dt><dd>{t('billing.panel.oneYear')}</dd></div>
            <div><dt>{t('billing.panel.packCredits')}</dt><dd>{t('billing.panel.productValidity')}</dd></div>
          </dl>
        </details>
      </aside>
    </div>
  </section>;
}
