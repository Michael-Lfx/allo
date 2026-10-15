import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, Progress, Tooltip, Typography } from '@arco-design/web-react';
import { learningApi } from '../api';
import type { CheckinStatus, XpCalendar, XpReading } from '../types';

const { Text } = Typography;

/** XP 日历窗口（周数）；恒 weeks×7 桶、末桶 = 今日 */
const XP_CALENDAR_WEEKS = 12;

/** 日净 XP → 单元格底色：四级正向色阶；负账如实出桶（danger 染色）；
 *  零桶显式在场但 UI 留空（ faint 底、不填 0）。全部语义令牌，双主题自适配。 */
function xpCellClass(xp: number): string {
  if (xp < 0) return 'bg-[color-mix(in_srgb,rgb(var(--danger-6))_40%,transparent)]';
  if (xp <= 0) return 'bg-[var(--color-fill-1)]';
  if (xp < 10) return 'bg-[color-mix(in_srgb,rgb(var(--primary-6))_24%,transparent)]';
  if (xp < 25) return 'bg-[color-mix(in_srgb,rgb(var(--primary-6))_48%,transparent)]';
  if (xp < 45) return 'bg-[color-mix(in_srgb,rgb(var(--primary-6))_72%,transparent)]';
  return 'bg-primary-6';
}

/** 深色热力格上文字转白保证可读（与色阶阈值一致） */
function xpCellOnDark(xp: number): boolean {
  return xp < 0 || xp >= 45;
}

/** 复习日 YYYYMMDD → 本地 Date（仅用于展示，分桶由后端按学习日完成） */
function reviewDayToDate(reviewDay: number): Date {
  const year = Math.floor(reviewDay / 10000);
  const month = Math.floor((reviewDay % 10000) / 100) - 1;
  const day = reviewDay % 100;
  return new Date(year, month, day);
}

/**
 * XP 读数面板（复习横幅展开区域）：今日 XP 卡 + 进度尺 + 84 格学习日历。
 * 措辞纪律：只报安排与事实（已得/目标/已结算/剩余/现步伐），不承诺记忆
 * 效果。打卡计数口径已退役（j05），本面板即其接管位置；读数与日历数据
 * 自管理加载（XP 读数端点自带学习日语义，不依赖前端时区换算）。
 */
export function XpPanel({
  checkin,
  celebrateToken,
}: {
  checkin: CheckinStatus | null;
  celebrateToken: number;
}) {
  const { t } = useTranslation();
  const [reading, setReading] = useState<XpReading | null>(null);
  const [calendar, setCalendar] = useState<XpCalendar | null>(null);
  const [celebrating, setCelebrating] = useState(false);

  const load = useCallback(async () => {
    try {
      setReading(await learningApi.getXpReading());
    } catch {
      setReading(null); // 读数拉取失败静默：面板保留空态，不影响复习主流程
    }
    try {
      setCalendar(await learningApi.getXpCalendar(XP_CALENDAR_WEEKS));
    } catch {
      setCalendar(null);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // 页面重新可见时刷新，覆盖跨 02:00 复习日切换场景
  useEffect(() => {
    const onVisibilityChange = () => {
      if (document.visibilityState === 'visible') void load();
    };
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => document.removeEventListener('visibilitychange', onVisibilityChange);
  }, [load]);

  // 完成仪式：completed false→true 翻转时触发一次高亮动画（3 秒后回落）
  useEffect(() => {
    if (celebrateToken <= 0) return;
    setCelebrating(true);
    const timer = window.setTimeout(() => setCelebrating(false), 3000);
    return () => window.clearTimeout(timer);
  }, [celebrateToken]);

  // 今日 XP 卡：锁定态（completed）优先，读数缺失时退回锁定快照字段
  const todayXp = reading?.today_xp ?? checkin?.reviewed_count ?? 0;
  const goal = reading?.goal ?? checkin?.goal ?? 0;
  const goalAchieved = checkin?.completed ?? reading?.goal_achieved ?? false;

  const percent =
    goal > 0 ? Math.min(100, Math.round((Math.max(0, todayXp) / goal) * 100)) : goalAchieved ? 100 : 0;
  const progressLabel = goal > 0 ? `${todayXp}/${goal}` : String(todayXp);

  // 84 格：桶已按学习日分好（旧 → 新，末桶 = 今日），对齐星期行（周日
  // 起），首列前置空位；负账如实出桶，空日留空不填 0。
  const today = useMemo(() => new Date(), []);
  const xpByDay = useMemo(() => {
    const map = new Map<number, number>();
    for (const day of calendar?.days ?? []) map.set(day.review_day, day.xp);
    return map;
  }, [calendar]);
  const grid = useMemo(() => {
    const days = calendar?.days ?? [];
    if (days.length === 0) return null;
    const firstDate = reviewDayToDate(days[0].review_day);
    const lead = firstDate.getDay();
    const cells: (number | null)[] = Array.from({ length: lead }, () => null);
    for (const day of days) cells.push(day.review_day);
    return { cells, lastDay: days[days.length - 1].review_day };
  }, [calendar]);
  const todayReviewDay =
    today.getFullYear() * 10000 + (today.getMonth() + 1) * 100 + today.getDate();

  const dayTooltip = (reviewDay: number | null): string => {
    if (reviewDay === null) return '';
    const xp = xpByDay.get(reviewDay) ?? 0;
    const date = reviewDayToDate(reviewDay);
    const label = t('learning.xpDayLabel', {
      month: date.getMonth() + 1,
      day: date.getDate(),
    });
    if (xp === 0) return `${label} · ${t('learning.xpDayNone')}`;
    return `${label} · ${t('learning.xpDayValue', { xp })}`;
  };

  const weekdayLabels = (t('learning.xpWeekdays', { returnObjects: true }) ?? []) as string[];

  return (
    <div className='flex flex-col gap-16px'>
      {/* 一级：今日 XP 卡（打卡计数口径的接管位置） */}
      <div
        className={`flex flex-col gap-8px rounded-12px border border-solid p-16px transition-colors ${
          celebrating
            ? 'border-[var(--color-success-6)] bg-[color-mix(in_srgb,rgb(var(--success-6))_12%,var(--color-bg-2))]'
            : 'border-[var(--color-border-2)] bg-[var(--color-bg-2)]'
        }`}
      >
        <div className='flex flex-wrap items-center justify-between gap-8px'>
          <div className='flex items-center gap-8px'>
            <Text bold>{t('learning.xpTodayTitle')}</Text>
            {goalAchieved && (
              <Text className='text-[var(--color-success-6)]'>✓ {t('learning.xpGoalReached')}</Text>
            )}
          </div>
          <Link href='/settings/learning'>{t('learning.xpGoalLink')}</Link>
        </div>
        <div className='flex items-center gap-12px'>
          <Progress
            className='flex-1'
            percent={percent}
            formatText={() => progressLabel}
            color={goalAchieved ? 'var(--color-success-6)' : undefined}
          />
          <Text type='secondary' className='shrink-0'>
            {goalAchieved
              ? t('learning.xpGoalReached')
              : t('learning.xpRemaining', { count: Math.max(0, goal - todayXp) })}
          </Text>
        </div>
        {/* 进度尺行（现存且启用口径，与分子分母同筛）：只报事实与安排 */}
        {reading && (
          <div className='flex flex-wrap items-center gap-x-16px gap-y-4px'>
            <Text type='secondary' className='text-12px'>
              {t('learning.xpGaugeSettled', {
                settled: reading.settled_nodes,
                total: reading.total_nodes,
              })}
            </Text>
            <Text type='secondary' className='text-12px'>
              {t('learning.xpGaugeRemaining', { count: reading.remaining_est })}
            </Text>
            <Text type='secondary' className='text-12px'>
              {reading.eta_days === null
                ? t('learning.xpEtaNone')
                : t('learning.xpEta', { count: reading.eta_days })}
            </Text>
          </div>
        )}
      </div>

      {/* 二级：84 格学习日历（近 {weeks} 周，末桶 = 今日；不过课程筛） */}
      {grid && (
        <div className='flex flex-col gap-4px'>
          <div className='flex gap-4px'>
            <div className='flex flex-col justify-between gap-2px py-0px'>
              {weekdayLabels.map((label) => (
                <Text key={label} type='secondary' className='h-16px text-right text-10px leading-16px'>
                  {label}
                </Text>
              ))}
            </div>
            <div className='flex gap-2px'>
              {Array.from({ length: Math.ceil(grid.cells.length / 7) }, (_, column) => (
                <div key={column} className='flex flex-col gap-2px'>
                  {Array.from({ length: 7 }, (_, row) => {
                    const index = column * 7 + row;
                    const reviewDay = grid.cells[index] ?? null;
                    if (reviewDay === null) {
                      return <div key={`blank-${column}-${row}`} className='h-16px w-16px' />;
                    }
                    const xp = xpByDay.get(reviewDay) ?? 0;
                    const isToday = reviewDay === grid.lastDay;
                    return (
                      <Tooltip key={reviewDay} content={dayTooltip(reviewDay)} position='top'>
                        <div
                          className={`h-16px w-16px rounded-2px ${xpCellClass(xp)}`}
                          style={
                            isToday
                              ? { boxShadow: '0 0 0 1px rgb(var(--primary-6))' }
                              : undefined
                          }
                        >
                          {xp !== 0 && (
                            <span
                              className={`block h-16px w-16px text-center text-9px font-medium leading-16px ${
                                xpCellOnDark(xp) ? 'text-white' : 'text-[var(--color-text-2)]'
                              }`}
                            >
                              {Math.abs(xp) >= 100 ? '99+' : Math.abs(xp)}
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
          <div className='flex flex-wrap items-center gap-12px text-12px text-[var(--color-text-3)]'>
            <span className='flex items-center gap-4px'>
              <span className='inline-block h-12px w-12px rounded-full' style={{ boxShadow: '0 0 0 1px rgb(var(--primary-6))' }} />
              {t('learning.xpLegendToday')}
            </span>
            <span>
              {t('learning.xpLegendIntensity')}
              <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-[var(--color-fill-1)]' />
              <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-[color-mix(in_srgb,rgb(var(--primary-6))_24%,transparent)]' />
              <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-[color-mix(in_srgb,rgb(var(--primary-6))_48%,transparent)]' />
              <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-[color-mix(in_srgb,rgb(var(--primary-6))_72%,transparent)]' />
              <span className='ml-1 inline-block h-10px w-10px rounded-2px bg-primary-6' />
            </span>
            <span className='flex items-center gap-4px'>
              <span className='inline-block h-10px w-10px rounded-2px bg-[color-mix(in_srgb,rgb(var(--danger-6))_40%,transparent)]' />
              {t('learning.xpLegendNegative')}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}
