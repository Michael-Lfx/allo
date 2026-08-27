import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Collapse, Input, Select, Switch } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import type { MeetingDevice, MeetingListenStatus, MeetingSession, MeetingVoiceprint, SttBackendChoice } from '@/common/adapter/ipcBridge';

const STT_OPTIONS: SttBackendChoice[] = ['auto', 'local_sherpa', 'cloud_model_invoke'];

type MeetingAdvancedPanelProps = {
  session: MeetingSession;
  devices: MeetingDevice[];
  voiceprints: MeetingVoiceprint[];
  listenStatus: MeetingListenStatus | null;
  busy: boolean;
  onBind: (conversationId: string | null) => Promise<unknown>;
  onListenToggle: (enabled: boolean, conversationId: string) => Promise<unknown>;
  onOpenVoiceprint: () => void;
};

const MeetingAdvancedPanel: React.FC<MeetingAdvancedPanelProps> = ({
  session,
  devices,
  voiceprints,
  listenStatus,
  busy,
  onBind,
  onListenToggle,
  onOpenVoiceprint,
}) => {
  const { t } = useTranslation();
  const [bindId, setBindId] = useState(session.bound_conversation_id ?? '');

  useEffect(() => {
    setBindId(session.bound_conversation_id ?? '');
  }, [session.bound_conversation_id, session.session_id]);

  const handleBind = useCallback(async () => {
    try {
      await onBind(bindId.trim() || null);
      Message.success(t('meeting.bindSuccess'));
    } catch (err) {
      Message.error(String(err));
    }
  }, [bindId, onBind, t]);

  const handleListen = useCallback(
    async (checked: boolean) => {
      try {
        await onListenToggle(checked, bindId.trim());
        Message.success(checked ? t('meeting.listen.enabled') : t('meeting.listen.disabled'));
      } catch (err) {
        Message.error(String(err));
      }
    },
    [bindId, onListenToggle, t]
  );

  return (
    <Collapse bordered={false} className='meeting-advanced'>
      <Collapse.Item header={t('meeting.advanced.title')} name='advanced'>
        <div className='flex flex-col gap-14px'>
          <div className='flex flex-wrap items-center gap-8px'>
            <Input
              className='min-w-220px flex-1'
              value={bindId}
              onChange={setBindId}
              placeholder={t('meeting.bindConversationPlaceholder')}
              addBefore={t('meeting.bindConversation')}
            />
            <Button loading={busy} onClick={() => void handleBind()}>
              {bindId.trim() ? t('meeting.bind') : t('meeting.unbind')}
            </Button>
          </div>

          <div className='flex flex-wrap items-center gap-8px'>
            <span className='text-13px text-t-primary'>{t('meeting.listen.title')}</span>
            <Switch
              checked={Boolean(listenStatus?.enabled)}
              loading={busy}
              onChange={(checked) => void handleListen(checked)}
            />
            <span className='text-12px text-t-tertiary'>{t('meeting.listen.hint')}</span>
          </div>

          <div className='flex min-w-0 flex-col gap-4px'>
            <span className='text-12px text-t-tertiary'>{t('meeting.sttBackend')}</span>
            <Select disabled value={session.stt_backend} options={STT_OPTIONS.map((value) => ({
              value,
              label: t(`meeting.stt.${value}`),
            }))} />
          </div>

          <div className='flex flex-col gap-6px'>
            <div className='text-12px text-t-tertiary'>{t('meeting.devices.title')}</div>
            {devices.length === 0 ? (
              <div className='text-13px text-t-secondary'>{t('meeting.devices.empty')}</div>
            ) : (
              <ul className='m-0 flex list-none flex-col gap-2px p-0 text-13px text-t-secondary'>
                {devices.map((device) => (
                  <li key={device.id}>
                    {device.kind === 'input' ? t('meeting.devices.input') : t('meeting.devices.output')}
                    {' · '}
                    {device.name}
                    {device.is_default ? ` (${t('meeting.devices.default')})` : ''}
                  </li>
                ))}
              </ul>
            )}
          </div>

          <div className='flex items-center justify-between gap-8px'>
            <div className='text-13px text-t-secondary'>
              {t('meeting.voiceprint.listCount', { count: voiceprints.length })}
            </div>
            <Button size='small' onClick={onOpenVoiceprint}>
              {t('meeting.voiceprint.manage')}
            </Button>
          </div>
        </div>
      </Collapse.Item>
    </Collapse>
  );
};

export default MeetingAdvancedPanel;
