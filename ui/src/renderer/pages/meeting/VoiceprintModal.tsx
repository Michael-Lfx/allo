import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input, Modal } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import type { MeetingVoiceprint } from '@/common/adapter/ipcBridge';

type VoiceprintModalProps = {
  visible: boolean;
  mode?: 'start' | 'manage';
  voiceprints: MeetingVoiceprint[];
  onCancel: () => void;
  onSkipAndStart?: () => void | Promise<void>;
  onEnrollAndStart?: (displayName: string) => void | Promise<void>;
  onEnroll: (displayName: string) => Promise<void>;
  onDelete: (voiceprintId: string) => Promise<void>;
};

const VoiceprintModal: React.FC<VoiceprintModalProps> = ({
  visible,
  mode = 'start',
  voiceprints,
  onCancel,
  onSkipAndStart,
  onEnrollAndStart,
  onEnroll,
  onDelete,
}) => {
  const { t } = useTranslation();
  const [name, setName] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (visible) setName('');
  }, [visible]);

  const handleEnroll = useCallback(async () => {
    const trimmed = name.trim();
    if (!trimmed) return;
    setBusy(true);
    try {
      await onEnroll(trimmed);
      setName('');
      Message.success(t('meeting.voiceprint.enrollSuccess'));
    } catch (err) {
      Message.error(String(err));
    } finally {
      setBusy(false);
    }
  }, [name, onEnroll, t]);

  const handleEnrollAndStart = useCallback(async () => {
    const trimmed = name.trim();
    setBusy(true);
    try {
      if (trimmed && onEnrollAndStart) {
        await onEnrollAndStart(trimmed);
      } else if (onSkipAndStart) {
        await onSkipAndStart();
      }
    } catch (err) {
      Message.error(String(err));
    } finally {
      setBusy(false);
    }
  }, [name, onEnrollAndStart, onSkipAndStart]);

  const handleSkip = useCallback(async () => {
    setBusy(true);
    try {
      await onSkipAndStart?.();
    } catch (err) {
      Message.error(String(err));
    } finally {
      setBusy(false);
    }
  }, [onSkipAndStart]);

  return (
    <Modal
      title={t('meeting.voiceprint.title')}
      visible={visible}
      onCancel={onCancel}
      footer={null}
      unmountOnExit
    >
      <p className='m-0 mb-14px text-13px leading-20px text-t-secondary'>
        {t('meeting.voiceprint.description')}
      </p>
      <div className='flex flex-col gap-10px'>
        <label className='text-12px text-t-tertiary'>{t('meeting.voiceprint.name')}</label>
        <div className='flex gap-8px'>
          <Input
            value={name}
            onChange={setName}
            placeholder={t('meeting.voiceprint.namePlaceholder')}
            disabled={busy}
          />
          <Button onClick={() => void handleEnroll()} loading={busy} disabled={!name.trim()}>
            {t('meeting.voiceprint.enroll')}
          </Button>
        </div>

        <div className='mt-8px'>
          <div className='mb-6px text-12px text-t-tertiary'>{t('meeting.voiceprint.list')}</div>
          {voiceprints.length === 0 ? (
            <div className='text-13px text-t-secondary'>{t('meeting.voiceprint.empty')}</div>
          ) : (
            <ul className='m-0 flex list-none flex-col gap-6px p-0'>
              {voiceprints.map((vp) => (
                <li
                  key={vp.voiceprint_id}
                  className='flex items-center justify-between gap-8px text-13px text-t-primary'
                >
                  <span className='min-w-0 truncate'>{vp.display_name}</span>
                  <Button
                    size='mini'
                    type='text'
                    status='danger'
                    disabled={busy}
                    onClick={() => {
                      void onDelete(vp.voiceprint_id).catch((err) => Message.error(String(err)));
                    }}
                  >
                    {t('meeting.voiceprint.delete')}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>

        {mode === 'start' ? (
          <div className='mt-12px flex justify-end gap-8px'>
            <Button
              onClick={() => void handleSkip()}
              loading={busy}
              disabled={!onSkipAndStart}
            >
              {t('meeting.voiceprint.skip')}
            </Button>
            <Button type='primary' onClick={() => void handleEnrollAndStart()} loading={busy}>
              {t('meeting.voiceprint.start')}
            </Button>
          </div>
        ) : (
          <div className='mt-12px flex justify-end'>
            <Button type='primary' onClick={onCancel}>
              {t('meeting.voiceprint.done')}
            </Button>
          </div>
        )}
      </div>
    </Modal>
  );
};

export default VoiceprintModal;
