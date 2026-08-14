import { useState } from 'react';
import {
  Button,
  Empty,
  Modal,
  Popconfirm,
  Progress,
  Space,
  Table,
  Tag,
  Tooltip,
  Typography,
} from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import type { CourseJobStatus, CourseJobView } from '../types';
import LearningModelSelector, {
  useLearningAutogenModel,
  type LearningModelChoice,
} from './LearningModelSelector';

const { Text } = Typography;

/** 运行中的任务阶段集合：这些状态下展示取消操作而非继续/重试 */
const RUNNING_STATUSES: ReadonlySet<CourseJobStatus> = new Set([
  'queued',
  'sampling',
  'blueprint',
  'lessons',
  'importing',
]);

function jobStatusMeta(
  status: CourseJobStatus,
  job: CourseJobView,
  t: (key: string, options?: Record<string, unknown>) => string
): { label: string; color: string } {
  switch (status) {
    case 'queued':
      return { label: t('learning.jobStatusQueued'), color: 'arcoblue' };
    case 'sampling':
      return { label: t('learning.jobStatusSampling'), color: 'arcoblue' };
    case 'blueprint':
      return { label: t('learning.jobStatusBlueprint'), color: 'arcoblue' };
    case 'lessons':
      return {
        label: t('learning.jobStatusLessons', {
          current: job.current_lesson,
          total: job.total_lessons,
        }),
        color: 'arcoblue',
      };
    case 'importing':
      return { label: t('learning.jobStatusImporting'), color: 'arcoblue' };
    case 'completed':
      return { label: t('learning.jobStatusCompleted'), color: 'green' };
    case 'failed':
      return { label: t('learning.jobStatusFailed'), color: 'red' };
    case 'cancelled':
      return { label: t('learning.jobStatusCancelled'), color: 'gray' };
    case 'interrupted':
      return { label: t('learning.jobStatusInterrupted'), color: 'orange' };
  }
}

/** 重试确认对话框：失败原因 + 可选的重试模型（默认沿用任务原模型，默认
 * 模型繁忙时可换用其他模型）。确认后由父级发起带模型偏好的重试请求。 */
function RetryJobDialog({
  job,
  onCancel,
  onConfirm,
}: {
  job: CourseJobView;
  onCancel: () => void;
  onConfirm: (choice: LearningModelChoice) => void;
}) {
  const { t } = useTranslation();
  // 以学习页持久化的模型偏好为默认值，与创建课程对话框保持一致
  const { choice: defaultChoice } = useLearningAutogenModel();
  const [choice, setChoice] = useState<LearningModelChoice>(defaultChoice);
  return (
    <Modal
      title={t('learning.jobRetryTitle')}
      visible
      style={{ width: 480 }}
      okText={t('learning.jobRetry')}
      cancelText={t('common.cancel')}
      onCancel={onCancel}
      onOk={() => onConfirm(choice)}
    >
      {job.error !== null && (
        <Text
          type='error'
          className='mb-12px block max-h-96px overflow-y-auto whitespace-pre-wrap break-all text-12px'
        >
          {t('learning.jobError', { error: job.error })}
        </Text>
      )}
      <div className='flex items-center justify-between gap-12px'>
        <Text>{t('learning.model')}</Text>
        <LearningModelSelector choice={choice} onChange={setChoice} size='small' />
      </div>
      <Text type='secondary' className='mt-8px block text-12px'>
        {t('learning.jobRetryHint')}
      </Text>
    </Modal>
  );
}

/** 学习页「生成任务」标签页：课程生成任务表格，展示阶段/进度/失败原因与
 * 取消、继续、重试、删除、打开课程动作。非终态任务的轮询由父级驱动。 */
export function CourseJobTable({
  jobs,
  loading,
  busyId,
  onCancel,
  onResume,
  onRetry,
  onDelete,
  onOpenCourse,
}: {
  jobs: CourseJobView[];
  loading: boolean;
  busyId: string | null;
  onCancel: (jobId: string) => void;
  onResume: (jobId: string) => void;
  onRetry: (jobId: string, choice: LearningModelChoice) => void;
  onDelete: (jobId: string) => void;
  onOpenCourse: (courseId: string) => void;
}) {
  const { t } = useTranslation();
  const [retryingJob, setRetryingJob] = useState<CourseJobView | null>(null);
  const columns = [
    {
      title: t('learning.jobColStatus'),
      dataIndex: 'status',
      width: 160,
      render: (_value: unknown, job: CourseJobView) => {
        const meta = jobStatusMeta(job.status, job, t);
        return <Tag color={meta.color}>{meta.label}</Tag>;
      },
    },
    {
      title: t('learning.jobColSubject'),
      dataIndex: 'subject',
      width: 200,
      render: (_value: unknown, job: CourseJobView) => {
        const subject = [job.knowledge_base_name, job.domain].filter((part) => part !== null).join(' · ');
        return subject === '' ? '—' : <span className='block truncate'>{subject}</span>;
      },
    },
    {
      title: t('learning.jobColProgress'),
      dataIndex: 'progress',
      width: 150,
      render: (_value: unknown, job: CourseJobView) => {
        if (job.status !== 'lessons' || job.total_lessons <= 0) return '—';
        const percent = Math.round((job.current_lesson / job.total_lessons) * 100);
        return (
          <div className='flex items-center gap-8px'>
            <Progress percent={percent} size='small' showText={false} className='!min-w-80px' />
            <Text type='secondary' className='text-12px'>
              {job.current_lesson}/{job.total_lessons}
            </Text>
          </div>
        );
      },
    },
    {
      title: t('learning.jobColSource'),
      dataIndex: 'source',
      width: 110,
      render: (_value: unknown, job: CourseJobView) =>
        job.source === 'agent' ? t('learning.jobSourceAgent') : t('learning.jobSourceHttp'),
    },
    {
      title: t('learning.jobColError'),
      dataIndex: 'error',
      width: 160,
      render: (_value: unknown, job: CourseJobView) =>
        job.status === 'failed' && job.error !== null ? (
          <Tooltip content={job.error} position='tl'>
            <Text type='error' className='block truncate text-12px'>
              {t('learning.jobError', { error: job.error })}
            </Text>
          </Tooltip>
        ) : (
          '—'
        ),
    },
    {
      title: t('learning.jobColCreated'),
      dataIndex: 'created_at',
      width: 160,
      render: (value: unknown) =>
        typeof value === 'number' ? (
          <span className='block truncate'>{new Date(value).toLocaleString()}</span>
        ) : (
          '—'
        ),
    },
    {
      title: t('learning.jobColActions'),
      dataIndex: 'actions',
      width: 160,
      render: (_value: unknown, job: CourseJobView) => {
        const busy = busyId === `job-${job.job_id}`;
        const courseId = job.course_id;
        return (
          <Space>
            {RUNNING_STATUSES.has(job.status) && (
              <Button size='small' loading={busy} onClick={() => onCancel(job.job_id)}>
                {t('learning.jobCancel')}
              </Button>
            )}
            {(job.status === 'cancelled' || job.status === 'interrupted') && (
              <Button size='small' type='primary' loading={busy} onClick={() => onResume(job.job_id)}>
                {t('learning.jobResume')}
              </Button>
            )}
            {job.status === 'failed' && (
              <Button size='small' type='primary' loading={busy} onClick={() => setRetryingJob(job)}>
                {t('learning.jobRetry')}
              </Button>
            )}
            {job.status === 'completed' && courseId !== null && (
              <Button size='small' type='primary' onClick={() => onOpenCourse(courseId)}>
                {t('learning.jobOpenCourse')}
              </Button>
            )}
            {(job.status === 'completed' || job.status === 'failed' || job.status === 'cancelled') && (
              <Popconfirm
                title={t('learning.jobDeleteConfirm')}
                okText={t('common.confirm')}
                cancelText={t('common.cancel')}
                onOk={() => onDelete(job.job_id)}
              >
                <Button size='small' loading={busy} status='danger'>
                  {t('learning.jobDelete')}
                </Button>
              </Popconfirm>
            )}
          </Space>
        );
      },
    },
  ];
  return (
    <>
      <Table
        rowKey='job_id'
        columns={columns}
        data={jobs}
        loading={loading}
        tableLayoutFixed
        pagination={{ pageSize: 10, hideOnSinglePage: true }}
        noDataElement={<Empty description={t('learning.jobsEmpty')} />}
      />
      {retryingJob !== null && (
        <RetryJobDialog
          job={retryingJob}
          onCancel={() => setRetryingJob(null)}
          onConfirm={(choice) => {
            onRetry(retryingJob.job_id, choice);
            setRetryingJob(null);
          }}
        />
      )}
    </>
  );
}
