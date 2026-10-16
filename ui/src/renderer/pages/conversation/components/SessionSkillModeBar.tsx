import { ipcBridge } from '@/common';
import type { IMessageSkillLoad } from '@/common/chat/chatLib';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { useMessageList } from '@renderer/pages/conversation/Messages/hooks';
import { Pin } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

type ModeChip = {
  skillId: string;
  name: string;
  auto: boolean;
  pinned: boolean;
};

/**
 * Session-level skill chips: pinned Custom Mode plus skills the model loaded
 * through the Skill tool. Pinning force-loads the skill on later turns.
 */
const SessionSkillModeBar: React.FC = () => {
  const { t } = useTranslation();
  const conversation = useConversationContextSafe();
  const messages = useMessageList();
  const [pins, setPins] = useState<string[]>([]);
  const [dismissed, setDismissed] = useState<string[]>([]);
  const conversationId = conversation?.conversation_id;
  const readOnly = conversation?.readOnly === true;
  const loadedSkills = conversation?.loadedSkills ?? [];

  const refreshPins = useCallback(async () => {
    if (!conversationId) return;
    try {
      const next = await ipcBridge.conversation.listSkillPins.invoke({
        conversation_id: conversationId,
      });
      setPins(Array.isArray(next) ? next : []);
    } catch (error) {
      console.warn('[skills] failed to load session pins', error);
    }
  }, [conversationId]);

  useEffect(() => {
    void refreshPins();
  }, [refreshPins]);

  const chips = useMemo<ModeChip[]>(() => {
    const byId = new Map<string, ModeChip>();
    for (const skillId of pins) {
      byId.set(skillId, {
        skillId,
        name: skillId.includes(':') ? skillId.slice(skillId.lastIndexOf(':') + 1) : skillId,
        auto: false,
        pinned: true,
      });
    }
    for (const message of messages) {
      if (message.type !== 'skill_load') continue;
      const load = message as IMessageSkillLoad;
      const skillId = load.content.skill_id;
      const auto = load.content.invocation === 'model-auto';
      const existing = byId.get(skillId);
      if (existing) {
        existing.name = load.content.name || existing.name;
        existing.auto = existing.auto || auto;
        continue;
      }
      if (!auto && loadedSkills.includes(skillId)) continue;
      if (dismissed.includes(skillId)) continue;
      byId.set(skillId, {
        skillId,
        name: load.content.name,
        auto,
        pinned: false,
      });
    }
    return [...byId.values()];
  }, [dismissed, loadedSkills, messages, pins]);

  const togglePin = async (chip: ModeChip) => {
    if (!conversationId || readOnly) return;
    try {
      await ipcBridge.conversation.setSkillPin.invoke({
        conversation_id: conversationId,
        skill_id: chip.skillId,
        pinned: !chip.pinned,
      });
      await refreshPins();
    } catch (error) {
      console.warn('[skills] failed to update skill pin', error);
    }
  };

  if (chips.length === 0) return null;

  return (
    <div
      className='flex min-w-0 flex-wrap items-center gap-6px px-2px pb-8px'
      data-testid='session-skill-mode-bar'
    >
      <span className='text-11px font-600 uppercase tracking-[0.08em] text-t-tertiary'>
        {t('conversation.skills.modeBar', { defaultValue: 'Session skills' })}
      </span>
      {chips.map((chip) => (
        <div
          key={chip.skillId}
          className='inline-flex max-w-full items-center gap-4px rounded-[100px] border border-solid border-[var(--color-border-2)] bg-fill-2 py-2px pl-8px pr-4px text-11px text-t-secondary'
          title={chip.skillId}
        >
          <span className='min-w-0 truncate'>{chip.name}</span>
          {chip.auto && (
            <span className='shrink-0 rounded-[8px] bg-[rgba(var(--success-6),0.12)] px-5px py-0 text-10px text-[rgb(var(--success-6))]'>
              {t('conversation.skills.autoLoaded', { defaultValue: 'Auto' })}
            </span>
          )}
          {!readOnly && (
            <div
              role='button'
              tabIndex={0}
              data-testid={`session-skill-pin-${chip.skillId}`}
              className={[
                'flex h-18px w-18px cursor-pointer items-center justify-center rounded-full',
                chip.pinned ? 'text-primary-6' : 'text-t-tertiary hover:text-t-secondary',
              ].join(' ')}
              onClick={() => void togglePin(chip)}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault();
                  void togglePin(chip);
                }
              }}
            >
              <Pin theme={chip.pinned ? 'filled' : 'outline'} size={12} />
            </div>
          )}
          {chip.auto && !chip.pinned && !readOnly && (
            <div
              role='button'
              tabIndex={0}
              className='flex h-18px w-18px cursor-pointer items-center justify-center rounded-full text-t-tertiary hover:text-t-secondary'
              onClick={() => setDismissed((current) => [...current, chip.skillId])}
              onKeyDown={(event) => {
                if (event.key === 'Enter' || event.key === ' ') {
                  event.preventDefault();
                  setDismissed((current) => [...current, chip.skillId]);
                }
              }}
            >
              ×
            </div>
          )}
        </div>
      ))}
    </div>
  );
};

export default SessionSkillModeBar;
