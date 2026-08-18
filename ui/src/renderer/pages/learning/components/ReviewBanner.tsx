import { Badge, Button, Select, Typography } from '@arco-design/web-react';
import { IconDown } from '@arco-design/web-react/icon';
import { useTranslation } from 'react-i18next';
import { ORPHAN_COURSE_FILTER } from '../constants';
import type { CheckinStatus, CourseSummary, DueReview } from '../types';
import { CheckinPanel } from './CheckinPanel';

const { Title, Text } = Typography;

/** 学习页顶部的复习横幅：到期队列筛选（课程 / 标签）、打卡进度徽章与开始复习；
 *  可展开查看打卡统计面板（今日卡片 + 月历/热力图），展开状态由外层持久化。 */
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
          <button
            type='button'
            aria-expanded={expanded}
            className='flex cursor-pointer select-none items-center gap-8px border-0 bg-transparent p-0 text-left'
            onClick={() => onExpandedChange(!expanded)}
          >
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
                className={
                  checkin.completed
                    ? 'text-[var(--color-success-6)]'
                    : 'text-[var(--color-primary-6)]'
                }
              >
                {badgeText}
              </Text>
            )}
            <IconDown
              className={`shrink-0 text-14px text-t-tertiary transition-transform ${expanded ? 'rotate-180' : ''}`}
            />
          </button>
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
        <Badge count={reviews.length}>
          <Button
            type='primary'
            size='large'
            loading={busy}
            disabled={reviews.length === 0}
            onClick={onStart}
          >
            {t('learning.startReview')}
          </Button>
        </Badge>
      </div>
      {expanded && (
        <>
          <div className='mx-20px border-t border-solid border-[var(--color-primary-light-3)]' />
          <div className='px-20px pb-16px pt-12px'>
            <CheckinPanel checkin={checkin} celebrateToken={celebrateToken} />
          </div>
        </>
      )}
    </div>
  );
}
