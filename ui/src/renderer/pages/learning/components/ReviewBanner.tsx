import { Button, Select, Typography } from '@arco-design/web-react';
import { IconCalendar, IconDown } from '@arco-design/web-react/icon';
import { useTranslation } from 'react-i18next';
import { ORPHAN_COURSE_FILTER } from '../constants';
import type { CheckinStatus, CourseSummary, DueReview } from '../types';
import { CheckinPanel } from './CheckinPanel';

const { Title, Text } = Typography;

/** 学习页顶部的复习横幅：到期队列筛选（课程 / 标签）与开始复习；
 *  底部独立"打卡统计"折叠条为展开入口（整条可点击，展开状态由外层持久化）。 */
export function ReviewBanner({
  reviews,
  courses,
  allTags,
  reviewCourseFilter,
  reviewTagFilter,
  busy,
  checkin,
  expanded,
  celebrateToken,
  onCourseFilterChange,
  onTagFilterChange,
  onExpandedChange,
  onStart,
}: {
  reviews: DueReview[];
  courses: CourseSummary[];
  allTags: string[];
  reviewCourseFilter: string[];
  reviewTagFilter: string[];
  busy: boolean;
  checkin: CheckinStatus | null;
  expanded: boolean;
  celebrateToken: number;
  onCourseFilterChange: (value: string[]) => void;
  onTagFilterChange: (value: string[]) => void;
  onExpandedChange: (value: boolean) => void;
  onStart: () => void;
}) {
  const { t } = useTranslation();
  // 紧凑打卡徽章：有目标显示进度，无目标显示已复习次数；完成态绿色高亮
  const badgeText = checkin
    ? checkin.goal > 0
      ? checkin.completed
        ? t('learning.checkinBadgeDone', { reviewed: checkin.reviewed_count, goal: checkin.goal })
        : t('learning.checkinBadge', { reviewed: checkin.reviewed_count, goal: checkin.goal })
      : checkin.completed
        ? t('learning.checkinBadgeNoGoalDone', { reviewed: checkin.reviewed_count })
        : t('learning.checkinBadgeNoGoal', { reviewed: checkin.reviewed_count })
    : '';
  return (
    <div className='rounded-12px border border-solid border-[var(--color-primary-6)] bg-[var(--color-primary-light-1)]'>
      <div className='flex flex-wrap items-center justify-between gap-12px px-20px py-16px'>
        <div className='flex flex-wrap items-center gap-12px'>
          <div className='flex items-baseline gap-8px'>
            <Title heading={5} className='!m-0'>
              {t('learning.reviews')}
            </Title>
            {reviews.length === 0 ? (
              <Text type='secondary'>{t('learning.noReviews')}</Text>
            ) : (
              <Text type='secondary'>
                {t('learning.reviewDueCount', { count: reviews.length })}
              </Text>
            )}
          </div>
          {checkin && (
            <Text
              className={`rounded-full border border-solid px-8px py-2px text-12px leading-18px ${
                checkin.completed
                  ? 'border-[var(--color-success-6)] text-[var(--color-success-6)]'
                  : 'border-[var(--color-primary-6)] text-[var(--color-primary-6)]'
              }`}
            >
              {badgeText}
            </Text>
          )}
          <Select
            className='w-240px'
            mode='multiple'
            allowClear
            placeholder={t('learning.reviewFilterCourse')}
            value={reviewCourseFilter}
            onChange={(value: string[] | undefined) =>
              onCourseFilterChange((value ?? []) as string[])
            }
          >
            {courses.map((course) => (
              <Select.Option key={course.id} value={course.id}>
                {course.title}
              </Select.Option>
            ))}
            <Select.Option value={ORPHAN_COURSE_FILTER}>
              {t('learning.reviewFilterOrphan')}
            </Select.Option>
          </Select>
          <Select
            className='w-200px'
            mode='multiple'
            allowClear
            placeholder={t('learning.reviewFilterTags')}
            value={reviewTagFilter}
            onChange={(value: string[] | undefined) =>
              onTagFilterChange((value ?? []) as string[])
            }
          >
            {allTags.map((tag) => (
              <Select.Option key={tag} value={tag}>
                {tag}
              </Select.Option>
            ))}
          </Select>
        </div>
        <Button
          type='primary'
          size='large'
          loading={busy}
          disabled={reviews.length === 0}
          onClick={onStart}
        >
          {t('learning.startReview')}
        </Button>
      </div>

      {/* 独立打卡统计折叠条：整条可点击，边框 + 图标 + 文字 + 圆形箭头，展开入口一目了然 */}
      <div className='px-20px pb-16px'>
        <button
          type='button'
          aria-expanded={expanded}
          className='flex w-full cursor-pointer items-center justify-between gap-12px rounded-10px border border-solid border-[var(--color-primary-light-3)] bg-[var(--color-bg-2)] px-16px py-12px transition-colors hover:border-[var(--color-primary-6)] hover:bg-[var(--color-primary-light-1)]'
          onClick={() => onExpandedChange(!expanded)}
        >
          <span className='flex min-w-0 flex-wrap items-center gap-8px'>
            <IconCalendar className='shrink-0 text-16px text-[var(--color-primary-6)]' />
            <Text bold>{t('learning.checkinTitle')}</Text>
            {checkin?.completed && (
              <Text className='text-[var(--color-success-6)]'>
                ✓ {t('learning.checkinCompleted')}
              </Text>
            )}
          </span>
          <span className='flex shrink-0 items-center gap-8px'>
            <Text className='text-12px text-[var(--color-primary-6)]'>
              {expanded ? t('learning.checkinCollapse') : t('learning.checkinExpand')}
            </Text>
            <span className='flex h-22px w-22px items-center justify-center rounded-full bg-[var(--color-primary-6)]'>
              <IconDown
                className={`text-12px text-white transition-transform ${expanded ? 'rotate-180' : ''}`}
              />
            </span>
          </span>
        </button>
        {expanded && (
          <div className='pt-12px'>
            <CheckinPanel checkin={checkin} celebrateToken={celebrateToken} />
          </div>
        )}
      </div>
    </div>
  );
}
