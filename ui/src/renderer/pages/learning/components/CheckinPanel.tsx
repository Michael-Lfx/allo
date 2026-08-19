import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, Progress, Tooltip, Typography } from '@arco-design/web-react';
import SegmentedTabs from '@/renderer/components/base/SegmentedTabs';
import { learningApi } from '../api';
import type { CalendarDayStats, CalendarStats, CheckinStatus } from '../types';

const { Text } = Typography;

/** 复习数 → 单元格底色（四级色阶 + 零值灰） */
function heatClass(reviewedCount: number): string {
  if (reviewedCount <= 0) return 'bg-[var(--color-fill-1)]';
  if (reviewedCount <= 2) return 'bg-[var(--color-primary-light-1)]';
  if (reviewedCount <= 5) return 'bg-[var(--color-primary-light-3)]';
  if (reviewedCount <= 9) return 'bg-[var(--color-primary-light-4)]';
  return 'bg-[var(--color-primary-6)]';
}

function isLeapYear(year: number): boolean {
  return (year % 4 === 0 && year % 100 !== 0) || year % 400 === 0;
}

/** 单元格角标：打卡完成 / 完成课时数 / 创建课程数（数字直显，最多 3 个）；
 *  深色热力格（≥10 次复习）上自动转白字保证可读。 */
function dayBadges(
  day: CalendarDayStats | undefined,
  onDark: boolean
): { label: string; className: string }[] {
  if (!day) return [];
  const badges: { label: string; className: string }[] = [];
  const primaryTone = onDark ? 'text-white' : 'text-[var(--color-primary-6)]';
  if (day.checkin_completed) {
    badges.push({
      label: '✓',
      className: onDark ? 'text-white' : 'text-[var(--color-success-6)]',
    });
  }
  if (day.completed_lessons.length > 0) {
    badges.push({ label: `📖${day.completed_lessons.length}`, className: primaryTone });
  }
  if (day.created_courses.length > 0) {
    badges.push({
      label: `📚${day.created_courses.length}`,
      className: onDark ? 'text-white' : 'text-warning-6',
    });
  }
  return badges.slice(0, 3);
}

/** 单元格 tooltip：复习数、打卡状态、完成课时与创建课程明细 */
function dayTooltip(day: CalendarDayStats | undefined, t: (key: string, opts?: Record<string, unknown>) => string): string {
  if (!day) return t('learning.checkinNoActivity');
  const parts: string[] = [];
  if (day.due_count > 0) {
    parts.push(t('learning.checkinTooltipDue', { count: day.due_count }));
  }
  if (day.reviewed_count > 0) {
    parts.push(t('learning.checkinTooltipReviews', { count: day.reviewed_count }));
  }
  if (day.checkin_completed) parts.push(t('learning.checkinTooltipCheckin'));
  if (day.completed_lessons.length > 0) {
    parts.push(
      t('learning.checkinTooltipLessons', {
        titles: day.completed_lessons.map((lesson) => lesson.title).join(', '),
      })
    );
  }
  if (day.created_courses.length > 0) {
    parts.push(
      t('learning.checkinTooltipCourses', {
        titles: day.created_courses.map((course) => course.title).join(', '),
      })
    );
  }
  return parts.length > 0 ? parts.join(' · ') : t('learning.checkinNoActivity');
}

/**
 * 打卡统计面板（复习横幅展开区域）：今日打卡卡片 + 月历/热力图两级视图。
 * 日历数据自管理加载（显式 tz_offset，不依赖偏好），打卡状态由外层传入。
 */
export function CheckinPanel({
  checkin,
  celebrateToken,
}: {
  checkin: CheckinStatus | null;
  celebrateToken: number;
}) {
  const { t } = useTranslation();
  const [view, setView] = useState<'month' | 'year'>('month');
  const [cursorYear, setCursorYear] = useState(() => new Date().getFullYear());
  const [cursorMonth, setCursorMonth] = useState(() => new Date().getMonth() + 1);
  const [stats, setStats] = useState<CalendarStats | null>(null);
  const [celebrating, setCelebrating] = useState(false);
  // 与后端 SchedulerSettings.tz_offset_minutes 同号（UTC 向东为正）；
  // JS API 符号相反，必须取反（东八区 getTimezoneOffset() = -480 → +480）。
  const tzOffset = useMemo(() => -new Date().getTimezoneOffset(), []);

  const loadStats = useCallback(async () => {
    try {
      const next = await learningApi.getCalendarStats(
        cursorYear,
        view === 'month' ? cursorMonth : undefined,
        tzOffset
      );
      setStats(next);
    } catch {
      setStats(null); // 日历拉取失败静默：面板保留空态，不影响复习主流程
    }
  }, [cursorYear, cursorMonth, view, tzOffset]);

  useEffect(() => {
    void loadStats();
  }, [loadStats]);

  // 页面重新可见时刷新日历，覆盖跨 02:00 复习日切换场景
  useEffect(() => {
    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') void loadStats();
    };
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => document.removeEventListener('visibilitychange', onVisibilityChange);
  }, [loadStats]);

  // 完成仪式：completed false→true 翻转时触发一次高亮动画（3 秒后回落）
  useEffect(() => {
    if (celebrateToken <= 0) return;
    setCelebrating(true);
    const timer = window.setTimeout(() => setCelebrating(false), 3000);
    return () => window.clearTimeout(timer);
  }, [celebrateToken]);

  const dayMap = useMemo(() => {
    const map = new Map<number, CalendarDayStats>();
    for (const day of stats?.days ?? []) map.set(day.review_day, day);
    return map;
  }, [stats]);

  const today = new Date();
  const todayReviewDay =
    today.getFullYear() * 10000 + (today.getMonth() + 1) * 100 + today.getDate();

  const percent = checkin
    ? checkin.goal > 0
      ? Math.min(100, Math.round((checkin.reviewed_count / checkin.goal) * 100))
      : checkin.completed
        ? 100
        : 0
    : 0;
  const progressLabel = checkin
    ? checkin.goal > 0
      ? `${checkin.reviewed_count}/${checkin.goal}`
      : String(checkin.reviewed_count)
    : '';

  const statusText = !checkin
    ? ''
    : checkin.completed
      ? t('learning.checkinCompleted')
      : checkin.goal > 0
        ? t('learning.checkinRemaining', {
            count: Math.max(0, checkin.goal - checkin.reviewed_count),
          })
        : t('learning.checkinClearQueue');

  const monthCells = useMemo(() => {
    const lead = new Date(cursorYear, cursorMonth - 1, 1).getDay();
    const daysInMonth = new Date(cursorYear, cursorMonth, 0).getDate();
    const cells: (number | null)[] = [];
    for (let i = 0; i < lead; i++) cells.push(null);
    for (let day = 1; day <= daysInMonth; day++) cells.push(day);
    return cells;
  }, [cursorYear, cursorMonth]);

  // 年视图：7 行 × 53 列（GitHub 风格），每列是一周（周日起）
  const heatColumns = useMemo(() => {
    const lead = new Date(cursorYear, 0, 1).getDay();
    const totalDays = isLeapYear(cursorYear) ? 366 : 365;
    const columns: (number | null)[][] = [];
    let day = 1;
    const first: (number | null)[] = [];
    for (let i = 0; i < lead; i++) first.push(null);
    while (first.length < 7) first.push(day++);
    columns.push(first);
    while (day <= totalDays) {
      const column: (number | null)[] = [];
      for (let i = 0; i < 7 && day <= totalDays; i++) column.push(day++);
      columns.push(column);
    }
    return columns;
  }, [cursorYear]);

  const pickHeatDay = (dayOfYear: number) => {
    const date = new Date(cursorYear, 0, dayOfYear);
    setCursorMonth(date.getMonth() + 1);
    setView('month');
  };

  // 年视图月份标签：每列 16px（14px 格子 + 2px 间距），标签绝对定位在
  // 月份第一天所在列，不受列宽限制，完整显示不裁切。
  const monthStarts = useMemo(() => {
    const starts: { month: number; columnIndex: number }[] = [];
    heatColumns.forEach((column, columnIndex) => {
      // 该列内任一天是某月 1 日即标记该列（1 号不一定在列首行）
      for (const day of column) {
        if (day === null) continue;
        const date = new Date(cursorYear, 0, day);
        if (date.getDate() === 1) {
          starts.push({ month: date.getMonth() + 1, columnIndex });
          break;
        }
      }
    });
    return starts;
  }, [heatColumns, cursorYear]);

  const moveMonth = (delta: number) => {
    const nextMonth = cursorMonth + delta;
    if (nextMonth < 1) {
      setCursorYear((year) => year - 1);
      setCursorMonth(12);
    } else if (nextMonth > 12) {
      setCursorYear((year) => year + 1);
      setCursorMonth(1);
    } else {
      setCursorMonth(nextMonth);
    }
  };

  const weekdayLabels = (t('learning.checkinWeekdays', { returnObjects: true }) ?? []) as string[];

  return (
    <div className='flex flex-col gap-16px'>
      {/* 一级：今日打卡卡片 */}
      <div
        className={`flex flex-col gap-8px rounded-12px border border-solid p-16px transition-colors ${
          celebrating
            ? 'border-[var(--color-success-6)] bg-[var(--color-success-light-1)]'
            : 'border-[var(--color-border-2)] bg-[var(--color-bg-2)]'
        }`}
      >
        <div className='flex flex-wrap items-center justify-between gap-8px'>
          <div className='flex items-center gap-8px'>
            <Text bold>{t('learning.checkinTitle')}</Text>
            {checkin?.completed && (
              <Text className='text-[var(--color-success-6)]'>✓ {t('learning.checkinCompleted')}</Text>
            )}
            {stats && stats.streak > 0 && (
              <Text type='secondary'>{t('learning.checkinStreak', { count: stats.streak })}</Text>
            )}
          </div>
          <Link href='/settings/learning'>{t('learning.checkinGoalLink')}</Link>
        </div>
        <div className='flex items-center gap-12px'>
          <Progress
            className='flex-1'
            percent={percent}
            formatText={() => progressLabel}
            color={checkin?.completed ? 'var(--color-success-6)' : undefined}
          />
          <Text type='secondary' className='shrink-0'>
            {statusText}
          </Text>
        </div>
      </div>

      {/* 二级：月历 / 热力图切换 */}
      <div className='flex flex-col gap-8px'>
        <div className='flex flex-wrap items-center justify-between gap-8px'>
          <SegmentedTabs
            size='sm'
            items={[
              { key: 'month', label: t('learning.checkinViewMonth') },
              { key: 'year', label: t('learning.checkinViewYear') },
            ]}
            activeKey={view}
            onChange={(key) => setView(key as 'month' | 'year')}
          />
          <div className='flex items-center gap-8px'>
            {view === 'month' ? (
              <>
                <Text type='secondary' className='min-w-120px text-center'>
                  {t('learning.checkinYearMonth', { year: cursorYear, month: cursorMonth })}
                </Text>
                <Text
                  className='cursor-pointer select-none hover:text-[var(--color-primary-6)]'
                  onClick={() => moveMonth(-1)}
                >
                  ‹
                </Text>
                <Text
                  className='cursor-pointer select-none hover:text-[var(--color-primary-6)]'
                  onClick={() => moveMonth(1)}
                >
                  ›
                </Text>
              </>
            ) : (
              <>
                <Text
                  className='cursor-pointer select-none hover:text-[var(--color-primary-6)]'
                  onClick={() => setCursorYear((year) => year - 1)}
                >
                  ‹
                </Text>
                <Text type='secondary' className='min-w-80px text-center'>
                  {t('learning.checkinYear', { year: cursorYear })}
                </Text>
                <Text
                  className='cursor-pointer select-none hover:text-[var(--color-primary-6)]'
                  onClick={() => setCursorYear((year) => year + 1)}
                >
                  ›
                </Text>
                <Text
                  className='cursor-pointer select-none text-[var(--color-primary-6)]'
                  onClick={() => {
                    const nowDate = new Date();
                    setCursorYear(nowDate.getFullYear());
                    setCursorMonth(nowDate.getMonth() + 1);
                  }}
                >
                  {t('learning.checkinBackToNow')}
                </Text>
              </>
            )}
          </div>
        </div>

        {view === 'month' ? (
          <div className='flex flex-col gap-4px'>
            <div className='grid grid-cols-7 gap-4px'>
              {weekdayLabels.map((label) => (
                <Text key={label} type='secondary' className='text-center text-12px'>
                  {label}
                </Text>
              ))}
            </div>
            <div className='grid grid-cols-7 gap-4px'>
              {monthCells.map((day, index) => {
                if (day === null) {
                  return <div key={`empty-${index}`} />;
                }
                const reviewDay = cursorYear * 10000 + cursorMonth * 100 + day;
                const statsDay = dayMap.get(reviewDay);
                const isToday = reviewDay === todayReviewDay;
                const dayDue = statsDay?.due_count ?? 0;
                // 深色热力格（≥10 次复习）上角标转白字，保证可读
                const onDark = (statsDay?.reviewed_count ?? 0) >= 10;
                return (
                  <Tooltip key={reviewDay} content={dayTooltip(statsDay, t)} position='top'>
                    <div
                      className={`flex min-h-40px flex-col items-center justify-between rounded-4px px-2px py-1px ${
                        heatClass(statsDay?.reviewed_count ?? 0)
                      } ${isToday ? 'ring-1 ring-[var(--color-primary-6)]' : ''}`}
                    >
                      <div className='flex max-w-full flex-wrap items-center gap-3px'>
                        {dayDue > 0 && (
                          <span className='rounded-full bg-danger-6 px-6px text-11px font-medium leading-16px text-white'>
                            {t('learning.checkinDueBadge', { count: dayDue })}
                          </span>
                        )}
                        {dayBadges(statsDay, onDark).map((badge, badgeIndex) => (
                          <span
                            key={badgeIndex}
                            className={`text-11px font-medium leading-16px ${badge.className}`}
                          >
                            {badge.label}
                          </span>
                        ))}
                      </div>
                      <Text className='text-12px leading-16px'>{day}</Text>
                    </div>
                  </Tooltip>
                );
              })}
            </div>
            <div className='flex flex-wrap items-center gap-12px text-12px text-[var(--color-text-3)]'>
              <span className='flex items-center gap-4px'>
                <span className='rounded-full bg-danger-6 px-6px text-10px font-medium leading-14px text-white'>
                  {t('learning.checkinDueBadge', { count: 3 })}
                </span>
                {t('learning.checkinLegendDue')}
              </span>
              <span>✓ {t('learning.checkinLegendCheckin')}</span>
              <span>📖 {t('learning.checkinLegendLessons')}</span>
              <span>📚 {t('learning.checkinLegendCourses')}</span>
              <span>
                {t('learning.checkinLegendIntensity')}
                <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-[var(--color-fill-1)]' />
                <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-[var(--color-primary-light-1)]' />
                <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-[var(--color-primary-light-3)]' />
                <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-[var(--color-primary-light-4)]' />
                <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-[var(--color-primary-6)]' />
              </span>
            </div>
          </div>
        ) : (
          <div className='flex flex-col gap-4px overflow-x-auto'>
            <div className='flex w-max flex-col gap-2px'>
              {/* 月份标签独立绝对定位行：不参与列宽计算，标签从月首列起完整显示不裁切 */}
              <div className='relative h-16px'>
                {monthStarts.map(({ month, columnIndex }) => (
                  <span
                    key={month}
                    className='absolute top-0 whitespace-nowrap text-10px font-medium leading-16px text-[var(--color-text-2)]'
                    style={{ left: columnIndex * 16 }}
                  >
                    {t('learning.checkinMonthLabel', { month })}
                  </span>
                ))}
              </div>
              <div className='flex gap-2px'>
                {heatColumns.map((column, columnIndex) => (
                  <div key={columnIndex} className='flex flex-col gap-2px'>
                    {column.map((day, rowIndex) => {
                      if (day === null) {
                        return <div key={`empty-${rowIndex}`} className='h-14px w-14px' />;
                      }
                      const date = new Date(cursorYear, 0, day);
                      const reviewDay =
                        date.getFullYear() * 10000 + (date.getMonth() + 1) * 100 + date.getDate();
                      const statsDay = dayMap.get(reviewDay);
                      // 深色热力格（≥10 次复习）上打勾转白字，保证可读
                      const onDark = (statsDay?.reviewed_count ?? 0) >= 10;
                      return (
                        <Tooltip key={reviewDay} content={dayTooltip(statsDay, t)} position='top'>
                          <div
                            className={`flex h-14px w-14px cursor-pointer items-center justify-center rounded-2px ${heatClass(
                              statsDay?.reviewed_count ?? 0
                            )} ${statsDay?.checkin_completed ? 'ring-1 ring-[var(--color-success-6)]' : ''}`}
                            onClick={() => pickHeatDay(day)}
                          >
                            {statsDay?.checkin_completed && (
                              <span
                                className={`text-10px font-medium leading-14px ${onDark ? 'text-white' : 'text-[var(--color-success-6)]'}`}
                              >
                                ✓
                              </span>
                            )}
                          </div>
                        </Tooltip>
                      );
                    })}
                  </div>
                ))}
              </div>
            </div>
            <div className='flex items-center gap-8px text-12px text-[var(--color-text-3)]'>
              <span>{t('learning.checkinLegendIntensity')}</span>
              <span className='inline-block h-10px w-10px rounded-2px bg-[var(--color-fill-1)]' />
              <span className='inline-block h-10px w-10px rounded-2px bg-[var(--color-primary-light-1)]' />
              <span className='inline-block h-10px w-10px rounded-2px bg-[var(--color-primary-light-3)]' />
              <span className='inline-block h-10px w-10px rounded-2px bg-[var(--color-primary-light-4)]' />
              <span className='inline-block h-10px w-10px rounded-2px bg-[var(--color-primary-6)]' />
              <span className='ml-4px'>✓ {t('learning.checkinLegendCheckin')}</span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
