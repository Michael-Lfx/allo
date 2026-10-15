import { useState } from 'react';
import { Button, Card, Input, Modal, Progress, Tag, Tooltip, Typography } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import type { CourseSummary } from '../types';
import { errorMessage } from '../utils';

const { Text, Paragraph } = Typography;

/** 归档徽标（三轮 g3）：课程处于可逆暂停态，安排面不再出现本课。 */
export function CourseArchivedBadge() {
  const { t } = useTranslation();
  return (
    <Tag size='small' color='gray' className='!mx-0 shrink-0'>
      {t('learning.courseArchivedBadge')}
    </Tag>
  );
}

/** 改名入口（三轮 g3）：只动显示名，目标/快照/学习数据不动。 */
export function CourseRenameButton({
  course,
  onChanged,
  size,
}: {
  course: CourseSummary;
  onChanged: () => void;
  size?: 'mini' | 'small' | 'default';
}) {
  const { t } = useTranslation();
  const [renaming, setRenaming] = useState(false);
  const [renameTitle, setRenameTitle] = useState('');
  const [busy, setBusy] = useState(false);
  const submitRename = () => {
    const title = renameTitle.trim();
    if (!title) {
      Message.warning(t('learning.courseRenameRequired'));
      return;
    }
    setBusy(true);
    learningApi
      .renameCourse(course.id, title)
      .then(() => {
        Message.success(t('learning.courseRenameDone'));
        setRenaming(false);
        onChanged();
      })
      .catch((actionError) => {
        Message.error(errorMessage(t, actionError));
      })
      .finally(() => setBusy(false));
  };
  return (
    <>
      <Button
        size={size}
        type='text'
        onClick={() => {
          setRenameTitle(course.title);
          setRenaming(true);
        }}
      >
        {t('learning.courseRename')}
      </Button>
      <Modal
        title={t('learning.courseRenameTitle', { title: course.title })}
        visible={renaming}
        style={{ width: 420 }}
        confirmLoading={busy}
        okText={t('learning.tagsSave')}
        onCancel={() => {
          if (!busy) setRenaming(false);
        }}
        onOk={submitRename}
      >
        <Input
          value={renameTitle}
          onChange={setRenameTitle}
          placeholder={t('learning.courseRenamePlaceholder')}
          maxLength={120}
        />
        <Paragraph type='secondary' className='!mb-0 mt-8px text-12px'>
          {t('learning.courseRenameHint')}
        </Paragraph>
      </Modal>
    </>
  );
}

export function CourseCard({
  course,
  onOpen,
  onReview,
  onEditTags,
  onDelete,
  onChanged,
}: {
  course: CourseSummary;
  onOpen: (id: string) => void;
  onReview: (id: string) => void;
  onEditTags: () => void;
  onDelete: () => void;
  /** 课程级写操作（改名/归档/恢复）成功后的刷新回调 */
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const percent =
    course.total_lessons === 0
      ? 0
      : Math.round((course.completed_lessons / course.total_lessons) * 100);

  const archive = () => {
    setBusy(true);
    learningApi
      .archiveCourse(course.id)
      .then(() => {
        Message.success(t('learning.courseArchiveDone'));
        onChanged();
      })
      .catch((actionError) => {
        Message.error(errorMessage(t, actionError));
      })
      .finally(() => setBusy(false));
  };

  const restore = () => {
    setBusy(true);
    learningApi
      .restoreCourse(course.id)
      .then(() => {
        Message.success(t('learning.courseRestoreDone'));
        onChanged();
      })
      .catch((actionError) => {
        Message.error(errorMessage(t, actionError));
      })
      .finally(() => setBusy(false));
  };

  return (
    <Card
      className='h-full'
      extra={
        <div className='flex shrink-0 gap-2px'>
          <CourseRenameButton course={course} onChanged={onChanged} size='mini' />
          <Button size='mini' type='text' status='danger' onClick={onDelete}>
            {t('learning.deleteCourse')}
          </Button>
        </div>
      }
      title={
        <div className='min-w-0'>
          <div className='flex items-center gap-6px'>
            <Tooltip content={course.learning_goal ?? course.title}>
              <span className='truncate text-16px font-600'>{course.title}</span>
            </Tooltip>
            {course.course_kind === 'learning_graph' && (
              <Tag size='small' color='orangered' className='!mx-0 shrink-0'>
                {t('learning.learningGraphBeta')}
              </Tag>
            )}
            {!course.enabled && <CourseArchivedBadge />}
          </div>
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
              {course.settled_or_completed}/{course.total_lessons} {t('learning.lessons')}
            </span>
          </div>
          <Progress percent={percent} showText={false} size='small' />
          <div className='mt-4px flex flex-wrap gap-12px text-12px text-t-tertiary'>
            <span>{t('learning.courseDueToday', { count: course.due_today })}</span>
            {course.total_xp !== null && (
              <span>{t('learning.courseTotalXp', { count: course.total_xp })}</span>
            )}
          </div>
          <div className='mt-14px flex gap-8px'>
            {course.enabled ? (
              <>
                <Button className='flex-1' type='primary' onClick={() => onOpen(course.id)}>
                  {course.enrolled ? t('learning.continue') : t('learning.open')}
                </Button>
                {course.enrolled && (
                  <Button onClick={() => onReview(course.id)}>{t('learning.reviewCourse')}</Button>
                )}
                <Button onClick={archive} disabled={busy}>
                  {t('learning.courseArchive')}
                </Button>
              </>
            ) : (
              <Button className='flex-1' type='primary' onClick={restore} loading={busy}>
                {t('learning.courseRestore')}
              </Button>
            )}
          </div>
        </div>
      </div>
    </Card>
  );
}

/**
 * 删除课程对话框（三轮 g3 强确认）：逐字输入课程显示名 + 三条如实后果。
 * 删除永远保留学习记录（复习卡/作答/进度）——没有连带删卡选项；只想暂停
 * 请用归档。
 */
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
  const [confirmation, setConfirmation] = useState('');
  const [busy, setBusy] = useState(false);
  const confirmed = confirmation.trim() === course.title.trim();
  return (
    <Modal
      title={t('learning.deleteCourseTitle')}
      visible
      style={{ width: 480 }}
      confirmLoading={busy}
      okText={t('learning.deleteCourseConfirm')}
      okButtonProps={{ status: 'danger', disabled: !confirmed }}
      onCancel={() => {
        if (!busy) onClose();
      }}
      onOk={() => {
        setBusy(true);
        learningApi
          .deleteCourse(course.id)
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
      <ul className='mt-0 mb-12px pl-18px text-13px text-t-secondary'>
        <li>{t('learning.deleteCourseConsequenceContent')}</li>
        <li>{t('learning.deleteCourseConsequenceRecords')}</li>
        <li>{t('learning.deleteCourseConsequenceIrreversible')}</li>
      </ul>
      <Input
        value={confirmation}
        onChange={setConfirmation}
        placeholder={t('learning.deleteCourseRequireInput', { title: course.title })}
      />
      {!confirmed && confirmation.length > 0 && (
        <Paragraph type='secondary' className='!mb-0 mt-6px text-12px'>
          {t('learning.deleteCourseMismatchHint')}
        </Paragraph>
      )}
    </Modal>
  );
}
