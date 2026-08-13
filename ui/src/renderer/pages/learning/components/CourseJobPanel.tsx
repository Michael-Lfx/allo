import { Button, Card, Progress, Space, Spin, Typography } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import type { CourseJobStatus, CourseJobView } from '../types';
import { isCourseJobTerminal } from '../hooks/useCourseJobs';

const { Title, Text } = Typography;

function jobStatusLabel(
  status: CourseJobStatus,
  job: CourseJobView,
  t: (key: string, options?: Record<string, unknown>) => string
) {
  switch (status) {
    case 'queued':
      return t('learning.jobStatusQueued');
    case 'sampling':
      return t('learning.jobStatusSampling');
    case 'blueprint':
      return t('learning.jobStatusBlueprint');
    case 'lessons':
      return t('learning.jobStatusLessons', {
        current: job.current_lesson,
        total: job.total_lessons,
      });
    case 'importing':
      return t('learning.jobStatusImporting');
    case 'completed':
      return t('learning.jobStatusCompleted');
    case 'failed':
      return t('learning.jobStatusFailed');
    case 'cancelled':
      return t('learning.jobStatusCancelled');
    case 'interrupted':
      return t('learning.jobStatusInterrupted');
  }
}

/** 单个生成任务的紧凑卡片：阶段文案 + 进度 + 上下文动作 */
function CourseJobCard({
  job,
  busy,
  onCancel,
  onResume,
  onRetry,
  onOpenCourse,
}: {
  job: CourseJobView;
  busy: boolean;
  onCancel: () => void;
  onResume: () => void;
  onRetry: () => void;
  onOpenCourse: () => void;
}) {
  const { t } = useTranslation();
  const running =
    job.status === 'queued' ||
    job.status === 'sampling' ||
    job.status === 'blueprint' ||
    job.status === 'lessons' ||
    job.status === 'importing';
  const percent =
    job.status === 'lessons' && job.total_lessons > 0
      ? Math.round((job.current_lesson / job.total_lessons) * 100)
      : 0;
  return (
    <Card size='small' className='!w-full'>
      <div className='flex flex-wrap items-center justify-between gap-12px'>
        <div className='flex min-w-0 flex-1 flex-col gap-6px'>
          <div className='flex flex-wrap items-center gap-8px'>
            <Text bold className='text-14px'>
              {jobStatusLabel(job.status, job, t)}
            </Text>
            <Text type='secondary' className='text-12px'>
              {job.source === 'agent' ? t('learning.jobSourceAgent') : t('learning.jobSourceHttp')}
            </Text>
          </div>
          {running ? (
            <Progress
              percent={percent}
              size='small'
              showText={false}
              animation={job.status !== 'lessons'}
              className='!w-full max-w-320px'
            />
          ) : (
            job.status === 'failed' &&
            job.error !== null && (
              <Text type='error' className='text-12px'>
                {t('learning.jobError', { error: job.error })}
              </Text>
            )
          )}
        </div>
        <Space>
          {running && (
            <Button size='small' loading={busy} onClick={onCancel}>
              {t('learning.jobCancel')}
            </Button>
          )}
          {(job.status === 'cancelled' || job.status === 'interrupted') && (
            <Button size='small' type='primary' loading={busy} onClick={onResume}>
              {t('learning.jobResume')}
            </Button>
          )}
          {job.status === 'failed' && (
            <Button size='small' type='primary' loading={busy} onClick={onRetry}>
              {t('learning.jobRetry')}
            </Button>
          )}
          {job.status === 'completed' && job.course_id !== null && (
            <Button size='small' type='primary' onClick={onOpenCourse}>
              {t('learning.jobOpenCourse')}
            </Button>
          )}
        </Space>
      </div>
    </Card>
  );
}

/** 学习页任务面板（ReviewBanner 之下、Tabs 之上）：展示课程生成任务的
 * 进度与取消/继续/重试/打开课程动作。任务全为终态后由父级轮询停止。 */
export function CourseJobPanel({
  jobs,
  loading,
  busyId,
  onCancel,
  onResume,
  onRetry,
  onOpenCourse,
}: {
  jobs: CourseJobView[];
  loading: boolean;
  busyId: string | null;
  onCancel: (jobId: string) => void;
  onResume: (jobId: string) => void;
  onRetry: (jobId: string) => void;
  onOpenCourse: (courseId: string) => void;
}) {
  const { t } = useTranslation();
  if (jobs.length === 0) return null;
  return (
    <section className='flex flex-col gap-10px'>
      <div className='flex items-center gap-8px'>
        <Title heading={5} className='!m-0'>
          {t('learning.jobsTitle')}
        </Title>
        {loading && <Spin size={14} />}
        {!jobs.some((job) => !isCourseJobTerminal(job)) && (
          <Text type='secondary' className='text-12px'>
            {t('learning.jobsAllTerminal')}
          </Text>
        )}
      </div>
      {jobs.map((job) => (
        <CourseJobCard
          key={job.job_id}
          job={job}
          busy={busyId === `job-${job.job_id}`}
          onCancel={() => onCancel(job.job_id)}
          onResume={() => onResume(job.job_id)}
          onRetry={() => onRetry(job.job_id)}
          onOpenCourse={() => {
            if (job.course_id !== null) onOpenCourse(job.course_id);
          }}
        />
      ))}
    </section>
  );
}
