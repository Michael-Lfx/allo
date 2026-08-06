

import { ipcBridge } from '@/common';
import type { ICronJob } from '@/common/adapter/ipcBridge';
import type { ConversationId, CronJobId } from '@/common/types/ids';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { Button, Popover, Tooltip } from '@arco-design/web-react';
import { AlarmClock } from '@icon-park/react';
import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { CAPABILITY_COLORS } from '@/renderer/components/capability/CapabilityIcon';
import { useCronJobs } from '../useCronJobs';
import { getJobStatusFlags } from '../cronUtils';
import {
  capabilityHeaderButtonClass,
  capabilityHeaderButtonStyle,
} from '@/renderer/pages/conversation/components/CapabilityHeaderButton';

interface CronJobManagerProps {
  conversation_id: ConversationId;
  /** When provided from the first-class conversation.cron_job_id field, fetch the job directly. */
  cron_job_id?: CronJobId;
  /** Whether the cron skill is loaded for this conversation. When false and no jobs exist, the component is hidden. */
  hasCronSkill?: boolean;
}

/**
 * Cron job manager component for ChatLayout headerExtra
 * Shows a single job per conversation with navigation to task detail
 */
const CronJobManager: React.FC<CronJobManagerProps> = ({ conversation_id, cron_job_id, hasCronSkill = true }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const layout = useLayoutContext();

  // For child conversations spawned by a cron job, fetch the job directly by ID
  const [directJob, setDirectJob] = useState<ICronJob | null>(null);
  const [directLoading, setDirectLoading] = useState(!!cron_job_id);

  useEffect(() => {
    if (!cron_job_id) {
      setDirectJob(null);
      setDirectLoading(false);
      return;
    }

    setDirectLoading(true);
    ipcBridge.cron.getJob
      .invoke({ cron_job_id: cron_job_id })
      .then((job) => setDirectJob(job ?? null))
      .catch(() => setDirectJob(null))
      .finally(() => setDirectLoading(false));
  }, [cron_job_id]);

  useEffect(() => {
    if (!cron_job_id) return;

    const unsubCreated = ipcBridge.cron.onJobCreated.on((created) => {
      if (created.cron_job_id === cron_job_id) {
        setDirectJob(created);
        setDirectLoading(false);
      }
    });
    const unsubUpdated = ipcBridge.cron.onJobUpdated.on((updated) => {
      if (updated.cron_job_id === cron_job_id) {
        setDirectJob(updated);
        setDirectLoading(false);
      }
    });
    const unsubRemoved = ipcBridge.cron.onJobRemoved.on(({ cron_job_id: removed_cron_job_id }) => {
      if (removed_cron_job_id === cron_job_id) {
        setDirectJob(null);
        setDirectLoading(false);
      }
    });

    return () => {
      unsubCreated();
      unsubUpdated();
      unsubRemoved();
    };
  }, [cron_job_id]);

  // For regular conversations, use the existing hook
  const { jobs, loading: listLoading, hasJobs } = useCronJobs(cron_job_id ? undefined : conversation_id);

  const job = cron_job_id ? directJob : (jobs[0] ?? null);
  const loading = cron_job_id ? directLoading : listLoading;
  const found = cron_job_id ? !!directJob : hasJobs;

  // Handle unconfigured state (no jobs)
  // If cron skill is not loaded for this conversation, hide entirely
  if (!found && !loading && !hasCronSkill) return null;

  // Hide on mobile/narrow widths to keep the titlebar slot uncluttered;
  // scheduling stays accessible via the sidebar entry.
  if (layout?.isMobile) return null;

  const handleCreateClick = () => {
    navigate(`/scheduled?create=conversation&conversation_id=${encodeURIComponent(conversation_id)}`);
  };

  const { hasError, isPaused } = job ? getJobStatusFlags(job) : { hasError: false, isPaused: false };
  const statusColor = !found
    ? CAPABILITY_COLORS.off
    : hasError
      ? CAPABILITY_COLORS.danger
      : isPaused
        ? CAPABILITY_COLORS.idle
        : CAPABILITY_COLORS.active;
  const triggerButton = (
    <Button
      size='mini'
      shape='round'
      type='secondary'
      className={capabilityHeaderButtonClass(found, 'shrink-0')}
      style={capabilityHeaderButtonStyle(statusColor)}
      onClick={found && job ? () => navigate(`/scheduled/${job.cron_job_id}`) : handleCreateClick}
    >
      <span className='inline-flex items-center gap-6px leading-none'>
        <AlarmClock theme='outline' size='14' fill={statusColor} />
        <span className='text-12px'>{t('cron.scheduledTasks')}</span>
      </span>
    </Button>
  );

  if (!found && !loading) {
    return (
      <Popover
        className='cron-job-manager-popover'
        trigger='hover'
        position='bottom'
        content={
          <div className='flex flex-col gap-8px p-4px max-w-240px'>
            <div className='text-13px text-t-secondary'>{t('cron.status.unconfiguredHint')}</div>
            <Button type='primary' size='mini' onClick={handleCreateClick}>
              {t('cron.status.createNow')}
            </Button>
          </div>
        }
      >
        {triggerButton}
      </Popover>
    );
  }

  if (loading || !job) return null;

  const tooltipContent = isPaused ? t('cron.status.paused') : hasError ? t('cron.status.error') : job.name;

  return (
    <Tooltip content={tooltipContent}>
      {triggerButton}
    </Tooltip>
  );
};

export default CronJobManager;
