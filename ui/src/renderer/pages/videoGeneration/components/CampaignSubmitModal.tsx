
import React, { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Empty, Modal, Spin } from '@arco-design/web-react';
import { Platte, VideoOne } from '@icon-park/react';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { listSessions, publishSessionToTvShow, invalidateCampaignCarouselCache } from '../api';
import { listCanvasProjects, publishCanvasProjectToTvShow } from '../../videoCanvas/api';
import type { SessionSummary } from '../types';
import type { CanvasProjectMeta } from '../../videoCanvas/api';
import styles from '../campaign.module.css';

interface CampaignSubmitModalProps {
  campaignId: number;
  visible: boolean;
  onClose: () => void;
  onSubmitted: () => void;
}

const CampaignSubmitModal: React.FC<CampaignSubmitModalProps> = ({
  campaignId,
  visible,
  onClose,
  onSubmitted,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [message, messageHolder] = useArcoMessage();
  const [loading, setLoading] = useState(false);
  const [publishingId, setPublishingId] = useState<string | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [projects, setProjects] = useState<CanvasProjectMeta[]>([]);

  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    setLoading(true);
    void Promise.all([listSessions(), listCanvasProjects()])
      .then(([allSessions, allProjects]) => {
        if (cancelled) return;
        setSessions(allSessions.filter((s) => s.status === 'succeeded'));
        setProjects(allProjects);
      })
      .catch((e) => {
        if (cancelled) return;
        message.error(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [message, visible]);

  const publishSession = useCallback(
    async (session: SessionSummary) => {
      if (publishingId) return;
      setPublishingId(session.id);
      try {
        await publishSessionToTvShow(session.id, {
          title: session.title || undefined,
          campaignId,
        });
        invalidateCampaignCarouselCache();
        message.success(
          t('videoGeneration.campaign.submit.ok', {
            defaultValue: '已提交审核，通过后会出现在活动作品流。',
          })
        );
        onSubmitted();
        onClose();
      } catch (e) {
        message.error(
          `${t('videoGeneration.campaign.submit.failed', { defaultValue: '投稿失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setPublishingId(null);
      }
    },
    [campaignId, message, onClose, onSubmitted, publishingId, t]
  );

  const publishCanvas = useCallback(
    async (project: CanvasProjectMeta) => {
      if (publishingId) return;
      setPublishingId(project.project_id);
      try {
        await publishCanvasProjectToTvShow(project.project_id, {
          title: project.title || undefined,
          campaignId,
        });
        invalidateCampaignCarouselCache();
        message.success(
          t('videoGeneration.campaign.submit.ok', {
            defaultValue: '已提交审核，通过后会出现在活动作品流。',
          })
        );
        onSubmitted();
        onClose();
      } catch (e) {
        message.error(
          `${t('videoGeneration.campaign.submit.failed', { defaultValue: '投稿失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setPublishingId(null);
      }
    },
    [campaignId, message, onClose, onSubmitted, publishingId, t]
  );

  const empty = sessions.length === 0 && projects.length === 0;

  return (
    <Modal
      title={t('videoGeneration.campaign.submit.title', { defaultValue: '选择要投稿的成片' })}
      visible={visible}
      onCancel={onClose}
      footer={null}
      unmountOnExit
      style={{ width: 480 }}
    >
      {messageHolder}
      {loading ? (
        <div className='flex justify-center py-36px'>
          <Spin />
        </div>
      ) : empty ? (
        <div className='flex flex-col items-center gap-12px py-16px'>
          <Empty
            description={t('videoGeneration.campaign.submit.empty', {
              defaultValue: '还没有可投稿的成片。请先完成一支短剧或画布作品。',
            })}
          />
          <Button
            type='primary'
            size='small'
            onClick={() => {
              onClose();
              navigate('/video-generation');
            }}
          >
            {t('videoGeneration.campaign.submit.goCreate', { defaultValue: '去创作' })}
          </Button>
        </div>
      ) : (
        <div className='flex flex-col gap-8px max-h-420px overflow-y-auto'>
          {sessions.map((session) => (
            <button
              key={session.id}
              type='button'
              className={styles.pickRow}
              disabled={publishingId != null}
              onClick={() => void publishSession(session)}
            >
              <span className='flex h-36px w-52px shrink-0 items-center justify-center rd-8px bg-[var(--color-fill-2)] text-[rgb(var(--primary-6))]'>
                <VideoOne theme='outline' size={16} fill='currentColor' />
              </span>
              <span className='min-w-0 flex-1'>
                <span className='block truncate text-13px font-600 text-[var(--color-text-1)]'>
                  {session.title ||
                    t('videoGeneration.list.untitled', { defaultValue: '未命名任务' })}
                </span>
                <span className='block text-11px text-[var(--color-text-3)]'>
                  {t('videoGeneration.campaign.submit.sessionKind', { defaultValue: '短剧工程' })}
                </span>
              </span>
            </button>
          ))}
          {projects.map((project) => (
            <button
              key={project.project_id}
              type='button'
              className={styles.pickRow}
              disabled={publishingId != null}
              onClick={() => void publishCanvas(project)}
            >
              <span className='flex h-36px w-52px shrink-0 items-center justify-center rd-8px bg-[var(--color-fill-2)] text-[rgb(var(--primary-6))]'>
                <Platte theme='outline' size={16} fill='currentColor' />
              </span>
              <span className='min-w-0 flex-1'>
                <span className='block truncate text-13px font-600 text-[var(--color-text-1)]'>
                  {project.title ||
                    t('videoGeneration.create.gallery.untitled', { defaultValue: '未命名画布' })}
                </span>
                <span className='block text-11px text-[var(--color-text-3)]'>
                  {t('videoGeneration.campaign.submit.canvasKind', { defaultValue: '创作画布' })}
                </span>
              </span>
            </button>
          ))}
        </div>
      )}
    </Modal>
  );
};

export default CampaignSubmitModal;
