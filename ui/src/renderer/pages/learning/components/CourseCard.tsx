import { useState } from 'react';
import { Button, Card, Checkbox, Modal, Progress, Tag, Typography } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import type { CourseSummary } from '../types';
import { errorMessage } from '../utils';

const { Text, Paragraph } = Typography;

export function CourseCard({
  course,
  onOpen,
  onReview,
  onEditTags,
  onDelete,
}: {
  course: CourseSummary;
  onOpen: (id: string) => void;
  onReview: (id: string) => void;
  onEditTags: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const percent =
    course.total_lessons === 0
      ? 0
      : Math.round((course.completed_lessons / course.total_lessons) * 100);
  return (
    <Card
      className='h-full'
      extra={
        <Button size='mini' type='text' status='danger' onClick={onDelete}>
          {t('learning.deleteCourse')}
        </Button>
      }
      title={
        <div className='min-w-0'>
          <div className='truncate text-16px font-600'>{course.title}</div>
          <div className='mt-2px text-12px text-t-tertiary'>{course.domain}</div>
        </div>
      }
    >
      <div className='flex h-full flex-col gap-14px'>
        <div className='flex flex-wrap items-center gap-4px'>
          {course.tags.length > 0 ? (
            course.tags.map((tag) => (
              <Tag key={tag} size='small' color='arcoblue'>
                {tag}
              </Tag>
            ))
          ) : (
            <Text type='secondary' className='text-12px'>
              {t('learning.tagsEmpty')}
            </Text>
          )}
          <Button size='mini' type='text' className='ml-auto' onClick={onEditTags}>
            {t('learning.tagsEditCourse')}
          </Button>
        </div>
        <Paragraph className='m-0 text-t-secondary' ellipsis={{ rows: 3 }}>
          {course.description}
        </Paragraph>
        <div className='mt-auto'>
          <div className='mb-6px flex justify-between text-12px text-t-secondary'>
            <span>{t('learning.progress')}</span>
            <span>
              {course.completed_lessons}/{course.total_lessons} {t('learning.lessons')}
            </span>
          </div>
          <Progress percent={percent} showText={false} size='small' />
          <div className='mt-14px flex gap-8px'>
            <Button className='flex-1' type='primary' onClick={() => onOpen(course.id)}>
              {course.enrolled ? t('learning.continue') : t('learning.open')}
            </Button>
            {course.enrolled && (
              <Button onClick={() => onReview(course.id)}>{t('learning.reviewCourse')}</Button>
            )}
          </div>
        </div>
      </div>
    </Card>
  );
}

export function CourseDeleteDialog({
  course,
  onClose,
  onDeleted,
}: {
  course: CourseSummary;
  onClose: () => void;
  onDeleted: () => void;
}) {
  const { t } = useTranslation();
  const [deleteReviews, setDeleteReviews] = useState(false);
  const [busy, setBusy] = useState(false);
  return (
    <Modal
      title={t('learning.deleteCourseTitle')}
      visible
      style={{ width: 480 }}
      confirmLoading={busy}
      okText={t('learning.deleteCourseConfirm')}
      okButtonProps={{ status: 'danger' }}
      onCancel={() => {
        if (!busy) onClose();
      }}
      onOk={() => {
        setBusy(true);
        learningApi
          .deleteCourse(course.id, deleteReviews)
          .then(() => {
            Message.success(t('learning.deleteCourseDone'));
            onDeleted();
          })
          .catch((actionError) => {
            Message.error(errorMessage(t, actionError));
          })
          .finally(() => setBusy(false));
      }}
    >
      <Paragraph className='mt-0'>
        {t('learning.deleteCourseHint', { title: course.title })}
      </Paragraph>
      <Checkbox checked={deleteReviews} onChange={setDeleteReviews}>
        {t('learning.deleteCourseReviews')}
      </Checkbox>
      <Paragraph type='secondary' className='!mb-0 mt-6px text-12px'>
        {deleteReviews
          ? t('learning.deleteCourseReviewsOn')
          : t('learning.deleteCourseReviewsOff')}
      </Paragraph>
    </Modal>
  );
}

