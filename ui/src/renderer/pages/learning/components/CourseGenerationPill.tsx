import { useTranslation } from 'react-i18next';
import { Button, Card, Spin, Tag, Typography } from '@arco-design/web-react';

const { Text } = Typography;

/** 后台生成悬浮指示条：创建对话框关闭后，生成仍在 HTTP 请求内继续——
 * 运行中/失败/完成三种状态都从这里回到对话框进度视图，运行中可取消。 */
export function CourseGenerationPill({
  status,
  topic,
  onView,
  onCancel,
}: {
  status: 'running' | 'completed' | 'failed';
  topic: string;
  onView: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const label =
    status === 'running'
      ? t('learning.genBgRunning')
      : status === 'failed'
        ? t('learning.genBgFailed')
        : t('learning.genBgDone');
  return (
    <div className='fixed right-24px bottom-24px z-100'>
      <Card size='small' className='shadow-lg'>
        <div className='flex items-center gap-12px'>
          {status === 'running' ? (
            <Spin size={14} />
          ) : (
            <Tag size='small' color={status === 'failed' ? 'red' : 'green'} className='!mx-0'>
              {status === 'failed' ? '✕' : '✓'}
            </Tag>
          )}
          <div className='min-w-0 max-w-260px'>
            <Text className='block truncate text-13px font-500'>{label}</Text>
            {topic && (
              <Text type='secondary' className='block truncate text-12px'>
                {topic}
              </Text>
            )}
          </div>
          <Button size='mini' type='secondary' onClick={onView}>
            {t('learning.genView')}
          </Button>
          {status === 'running' && (
            <Button size='mini' status='danger' onClick={onCancel}>
              {t('learning.genCancel')}
            </Button>
          )}
        </div>
      </Card>
    </div>
  );
}
