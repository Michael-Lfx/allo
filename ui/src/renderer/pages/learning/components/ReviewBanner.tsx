import { Badge, Button, Select, Typography } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { ORPHAN_COURSE_FILTER } from '../constants';
import type { CourseSummary, DueReview } from '../types';

const { Title, Text } = Typography;

/** 学习页顶部的复习横幅：到期队列筛选（课程 / 标签）与开始复习 */
export function ReviewBanner({
  reviews,
  courses,
  allTags,
  reviewCourseFilter,
  reviewTagFilter,
  busy,
  onCourseFilterChange,
  onTagFilterChange,
  onStart,
}: {
  reviews: DueReview[];
  courses: CourseSummary[];
  allTags: string[];
  reviewCourseFilter: string[];
  reviewTagFilter: string[];
  busy: boolean;
  onCourseFilterChange: (value: string[]) => void;
  onTagFilterChange: (value: string[]) => void;
  onStart: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className='flex flex-wrap items-center justify-between gap-12px rounded-12px border border-solid border-[var(--color-primary-6)] bg-[var(--color-primary-light-1)] px-20px py-16px'>
      <div className='flex flex-wrap items-center gap-12px'>
        <div className='flex items-baseline gap-8px'>
          <Title heading={5} className='!m-0'>
            {t('learning.reviews')}
          </Title>
          <Text type='secondary'>
            {t('learning.reviewDueCount', { count: reviews.length })}
          </Text>
        </div>
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
        <Button type='primary' size='large' loading={busy} onClick={onStart}>
          {t('learning.startReview')}
        </Button>
      </Badge>
    </div>
  );
}
