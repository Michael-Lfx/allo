/**
 * SkillConfirmModals — Two small confirmation modals:
 * 1. Delete pending skill confirmation
 * 2. Remove custom skill from preset confirmation
 */
import type { Message } from '@arco-design/web-react';
import type { PendingSkill } from './types';
import {
  pendingSkillSelectionId,
  type SelectedPresetSkill,
} from './presetSkillBindings';
import { Modal } from '@arco-design/web-react';
import React from 'react';
import { useTranslation } from 'react-i18next';

type SkillConfirmModalsProps = {
  // Delete pending skill
  deletePendingSkillName: string | null;
  setDeletePendingSkillName: (v: string | null) => void;
  pendingSkills: PendingSkill[];
  setPendingSkills: (v: PendingSkill[]) => void;

  // Delete custom skill
  deleteCustomSkill: SelectedPresetSkill | null;
  setDeleteCustomSkill: (v: SelectedPresetSkill | null) => void;

  // Shared state
  selectedSkills: string[];
  setSelectedSkills: (v: string[]) => void;

  message: Required<ReturnType<typeof Message.useMessage>[0]>;
};

const SkillConfirmModals: React.FC<SkillConfirmModalsProps> = ({
  deletePendingSkillName,
  setDeletePendingSkillName,
  pendingSkills,
  setPendingSkills,
  deleteCustomSkill,
  setDeleteCustomSkill,
  selectedSkills,
  setSelectedSkills,
  message,
}) => {
  const { t } = useTranslation();

  return (
    <>
      {/* Delete Pending Skill Confirmation Modal */}
      <Modal
        visible={deletePendingSkillName !== null}
        onCancel={() => setDeletePendingSkillName(null)}
        title={t('settings.deletePendingSkillTitle', { defaultValue: 'Delete Pending Skill' })}
        okButtonProps={{ status: 'danger' }}
        okText={t('common.delete', { defaultValue: 'Delete' })}
        cancelText={t('common.cancel', { defaultValue: 'Cancel' })}
        onOk={() => {
          if (deletePendingSkillName) {
            setPendingSkills(pendingSkills.filter((s) => s.name !== deletePendingSkillName));
            setSelectedSkills(
              selectedSkills.filter(
                (skillId) =>
                  !pendingSkills.some(
                    (skill) =>
                      skill.name === deletePendingSkillName &&
                      pendingSkillSelectionId(skill) === skillId,
                  ),
              ),
            );
            setDeletePendingSkillName(null);
            message.success(t('settings.skillDeleted', { defaultValue: 'Skill removed from pending list' }));
          }
        }}
        className='w-[90vw] md:w-[400px]'
        wrapStyle={{ zIndex: 10000 }}
        maskStyle={{ zIndex: 9999 }}
      >
        <p>
          {t('settings.deletePendingSkillConfirm', {
            defaultValue: `Are you sure you want to remove "${deletePendingSkillName}"? This skill has not been imported yet.`,
          })}
        </p>
        <div className='mt-12px text-12px text-t-secondary bg-fill-2 p-12px rounded-lg'>
          {t('settings.deletePendingSkillNote', {
            defaultValue:
              'This will only remove the skill from the pending list. If you want to add it again later, you can use "Add Skills".',
          })}
        </div>
      </Modal>

      {/* Remove Custom Skill from Preset Modal */}
      <Modal
        visible={deleteCustomSkill !== null}
        onCancel={() => setDeleteCustomSkill(null)}
        title={t('settings.removeCustomSkillTitle', { defaultValue: 'Remove Skill from Preset' })}
        okButtonProps={{ status: 'danger' }}
        okText={t('common.remove', { defaultValue: 'Remove' })}
        cancelText={t('common.cancel', { defaultValue: 'Cancel' })}
        onOk={() => {
          if (deleteCustomSkill) {
            setSelectedSkills(selectedSkills.filter((skillId) => skillId !== deleteCustomSkill.skillId));
            setDeleteCustomSkill(null);
            message.success(
              t('settings.skillRemovedFromPreset', { defaultValue: 'Skill removed from this preset' })
            );
          }
        }}
        className='w-[90vw] md:w-[400px]'
        wrapStyle={{ zIndex: 10000 }}
        maskStyle={{ zIndex: 9999 }}
      >
        <p>
          {t('settings.removeCustomSkillConfirm', {
            defaultValue: `Are you sure you want to remove "${deleteCustomSkill?.name ?? ''}" from this preset?`,
          })}
        </p>
        <div className='mt-12px text-12px text-t-secondary bg-fill-2 p-12px rounded-lg'>
          {t('settings.removeCustomSkillNote', {
            defaultValue:
              'This will only remove the skill from this preset. The skill will remain in Builtin Skills and can be re-added later.',
          })}
        </div>
      </Modal>
    </>
  );
};

export default SkillConfirmModals;
