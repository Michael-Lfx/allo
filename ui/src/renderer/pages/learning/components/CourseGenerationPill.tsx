import { useTranslation } from 'react-i18next';
import { Button, Card, Spin, Tag, Typography } from '@arco-design/web-react';

const { Text } = Typography;

/** 后台生成悬浮指示条：创建对话框关闭后，生成仍在 HTTP 请求内继续——
 * 排队中（m7 资源闸）/运行中/失败/取消/完成五种状态都从这里回到对话框
 * 进度视图，排队期与运行中都可取消（排队取消 = 释放排队位，body 零执行）。
 * 多知识库（t4）：sources 非空时展示「来源：库A、库B」（名称由调用方按库 id 解析）。 */
export function CourseGenerationPill({
  status,
  topic,
  sources = [],
  onView,
  onCancel,
}: {
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled';
  topic: string;
  sources?: string[];
  onView: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const label =
    status === 'queued'
      ? t('learning.genBgQueued')
      : status === 'running'
        ? t('learning.genBgRunning')
        : status === 'failed'
          ? t('learning.genBgFailed')
          : status === 'cancelled'
            ? t('learning.genCancelled')
            : t('learning.genBgDone');
  return (
    <div className='fixed right-24px bottom-24px z-100'>
      <Card size='small' className='shadow-lg'>
        <div className='flex items-center gap-12px'>
          {status === 'running' || status === 'queued' ? (
            <Spin size={14} />
          ) : (
            <Tag
              size='small'
              color={status === 'failed' ? 'red' : status === 'cancelled' ? 'orange' : 'green'}
              className='!mx-0'
            >
              {status === 'completed' ? '✓' : '✕'}
            </Tag>
          )}
          <div className='min-w-0 max-w-260px'>
            <Text className='block truncate text-13px font-500'>{label}</Text>
            {topic && (
              <Text type='secondary' className='block truncate text-12px'>
                {topic}
              </Text>
            )}
            {sources.length > 0 && (
              <Text type='secondary' className='block truncate text-12px'>
                {t('learning.courseSources', {
                  names: sources.join(t('learning.listSeparator')),
                })}
              </Text>
            )}
          </div>
          <Button size='mini' type='secondary' onClick={onView}>
            {t('learning.genView')}
          </Button>
          {(status === 'running' || status === 'queued') && (
            <Button size='mini' status='danger' onClick={onCancel}>
              {t('learning.genCancel')}
            </Button>
          )}
        </div>
      </Card>
    </div>
  );
}
