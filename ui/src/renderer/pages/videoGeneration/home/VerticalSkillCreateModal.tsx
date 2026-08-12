/**
 * Centered modal card for creating a vertical Skill.
 */
import React, { useState } from 'react';
import { Input, Modal, Select } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { createVerticalSkill } from '../api';
import type { VerticalSkillDraft } from '../types';
import styles from './verticalSkillHub.module.css';

const EMPTY_DRAFT: VerticalSkillDraft = {
  name: '',
  display_name: '',
  description: '',
  category: '',
  version: '1.0.0',
  tags: [],
  compatible_modes: ['idea2video', 'script2video', 'novel2video'],
  requirement_overlay: '',
  style_overlay: '',
  playbook: '',
};

export interface VerticalSkillCreateModalProps {
  visible: boolean;
  onClose: () => void;
  onCreated: (skillId: string) => void;
}

const VerticalSkillCreateModal: React.FC<VerticalSkillCreateModalProps> = ({
  visible,
  onClose,
  onCreated,
}) => {
  const { t } = useTranslation();
  const [message, messageHolder] = useArcoMessage();
  const [draft, setDraft] = useState<VerticalSkillDraft>(EMPTY_DRAFT);
  const [saving, setSaving] = useState(false);

  const handleClose = () => {
    if (saving) return;
    setDraft(EMPTY_DRAFT);
    onClose();
  };

  const handleCreate = async () => {
    if (!draft.name.trim() || !draft.description.trim()) {
      message.warning(
        t('videoGeneration.skills.createRequired', {
          defaultValue: '请填写名称与描述。',
        })
      );
      return;
    }
    setSaving(true);
    try {
      const created = await createVerticalSkill({
        ...draft,
        name: draft.name.trim(),
        display_name: draft.display_name?.trim() || undefined,
        description: draft.description.trim(),
        tags: (draft.tags ?? []).map(String).filter(Boolean),
      });
      message.success(
        t('videoGeneration.skills.createOk', {
          name: created.display_name || created.name,
          defaultValue: '已创建 {{name}}',
        })
      );
      setDraft(EMPTY_DRAFT);
      onCreated(created.id);
      onClose();
    } catch (error) {
      message.error(error instanceof Error ? error.message : String(error));
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      {messageHolder}
      <Modal
        title={t('videoGeneration.skills.createTitle', {
          defaultValue: '创建垂直 Skill',
        })}
        visible={visible}
        onCancel={handleClose}
        onOk={() => void handleCreate()}
        confirmLoading={saving}
        okText={t('videoGeneration.skills.createSubmit', {
          defaultValue: '创建并挂载',
        })}
        cancelText={t('videoGeneration.skills.backToList', {
          defaultValue: '取消',
        })}
        style={{ width: 520 }}
        unmountOnExit
        maskClosable={!saving}
        className={styles.createModal}
        alignCenter
      >
        <div className={styles.createForm}>
          <label className={styles.field}>
            <span>
              {t('videoGeneration.skills.fields.name', { defaultValue: '标识名' })}
            </span>
            <Input
              value={draft.name}
              placeholder='my-luxury-tvc'
              onChange={(value) => setDraft((current) => ({ ...current, name: value }))}
            />
          </label>
          <label className={styles.field}>
            <span>
              {t('videoGeneration.skills.fields.displayName', {
                defaultValue: '显示名',
              })}
            </span>
            <Input
              value={draft.display_name ?? ''}
              onChange={(value) =>
                setDraft((current) => ({ ...current, display_name: value }))
              }
            />
          </label>
          <label className={styles.field}>
            <span>
              {t('videoGeneration.skills.fields.description', {
                defaultValue: '描述',
              })}
            </span>
            <Input.TextArea
              value={draft.description}
              autoSize={{ minRows: 2, maxRows: 4 }}
              onChange={(value) =>
                setDraft((current) => ({ ...current, description: value }))
              }
            />
          </label>
          <label className={styles.field}>
            <span>
              {t('videoGeneration.skills.fields.modes', {
                defaultValue: '兼容 Mode',
              })}
            </span>
            <Select
              mode='multiple'
              value={draft.compatible_modes ?? []}
              onChange={(value) =>
                setDraft((current) => ({
                  ...current,
                  compatible_modes: value as string[],
                }))
              }
              options={[
                { label: '一个想法', value: 'idea2video' },
                { label: '完整剧本', value: 'script2video' },
                { label: '小说文本', value: 'novel2video' },
              ]}
            />
          </label>
          <label className={styles.field}>
            <span>
              {t('videoGeneration.skills.fields.requirement', {
                defaultValue: '叙事 Overlay',
              })}
            </span>
            <Input.TextArea
              value={draft.requirement_overlay ?? ''}
              autoSize={{ minRows: 3, maxRows: 6 }}
              onChange={(value) =>
                setDraft((current) => ({ ...current, requirement_overlay: value }))
              }
            />
          </label>
          <label className={styles.field}>
            <span>
              {t('videoGeneration.skills.fields.style', {
                defaultValue: '视觉 Overlay',
              })}
            </span>
            <Input.TextArea
              value={draft.style_overlay ?? ''}
              autoSize={{ minRows: 2, maxRows: 4 }}
              onChange={(value) =>
                setDraft((current) => ({ ...current, style_overlay: value }))
              }
            />
          </label>
          <label className={styles.field}>
            <span>
              {t('videoGeneration.skills.fields.playbook', {
                defaultValue: '导演 Playbook',
              })}
            </span>
            <Input.TextArea
              value={draft.playbook ?? ''}
              autoSize={{ minRows: 4, maxRows: 8 }}
              onChange={(value) =>
                setDraft((current) => ({ ...current, playbook: value }))
              }
            />
          </label>
        </div>
      </Modal>
    </>
  );
};

export default VerticalSkillCreateModal;
