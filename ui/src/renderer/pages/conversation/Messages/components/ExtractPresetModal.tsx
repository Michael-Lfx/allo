/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState } from 'react';
import { Button, Input, Modal } from '@arco-design/web-react';
import { Lightning, Tips } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { ipcBridge } from '@/common';
import { buildPresetExtractionDraft, type PresetDraftData } from './extractPresetModel';

export interface ExtractPresetModalProps {
  visible: boolean;
  onCancel: () => void;
  messageText: string;
  userQuestion?: string;
  conversationTitle?: string;
}

const ExtractPresetModal: React.FC<ExtractPresetModalProps> = ({
  visible,
  onCancel,
  messageText,
  userQuestion,
  conversationTitle,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [guidance, setGuidance] = useState('');
  const [isExtracting, setIsExtracting] = useState(false);

  const handleStartExtract = async () => {
    setIsExtracting(true);
    try {
      // 1. Fetch available skills for intelligent binding recommendation
      let availableSkills: Array<{ id: string; name?: string }> = [];
      try {
        const catalog = await ipcBridge.fs.listSkillCatalog.invoke();
        availableSkills = catalog.skills;
      } catch (err) {
        console.warn('[ExtractPreset] Failed to fetch skill catalog:', err);
      }

      // 2. Build structured extraction draft
      const draft: PresetDraftData = buildPresetExtractionDraft({
        messageText,
        userQuestion,
        conversationTitle,
        userGuidance: guidance,
        availableSkills,
      });

      // 3. Close modal and navigate to preset editor drawer
      onCancel();
      setGuidance('');
      Message.success(
        t('conversation.extractPreset.success', {
          defaultValue: '已成功提炼设定，您可继续调整并保存',
        })
      );
      navigate('/presets', {
        state: {
          openPresetEditor: true,
          createPresetDraft: draft,
        },
      });
    } catch (error) {
      console.error('[ExtractPreset] Extraction failed:', error);
      Message.error(t('common.unknownError', { defaultValue: '提炼失败，请重试' }));
    } finally {
      setIsExtracting(false);
    }
  };

  return (
    <Modal
      title={
        <div className='flex items-center gap-6px font-500'>
          <Lightning theme='filled' size={18} fill='var(--color-primary-6)' />
          <span>{t('conversation.extractPreset.title', { defaultValue: '提炼为设定' })}</span>
        </div>
      }
      visible={visible}
      onCancel={() => {
        if (!isExtracting) {
          setGuidance('');
          onCancel();
        }
      }}
      footer={
        <div className='flex justify-end gap-8px'>
          <Button disabled={isExtracting} onClick={onCancel}>
            {t('conversation.extractPreset.cancel', { defaultValue: '取消' })}
          </Button>
          <Button
            type='primary'
            loading={isExtracting}
            onClick={handleStartExtract}
            data-testid='btn-confirm-extract-preset'
          >
            {t('conversation.extractPreset.startExtract', { defaultValue: '开始提炼' })}
          </Button>
        </div>
      }
      className='flowy-modal'
      style={{ maxWidth: 520, width: '90vw' }}
    >
      <div className='flex flex-col gap-12px'>
        <div className='p-12px rd-8px bg-fill-2 flex items-start gap-8px text-13px text-t-secondary leading-normal'>
          <Tips theme='outline' size={16} className='mt-2px shrink-0 text-primary-6' />
          <span>
            {t('conversation.extractPreset.description', {
              defaultValue:
                'AI 将根据当前对话上下文，自动提取并提炼出专属角色名称、系统提示词与推荐绑定的技能。',
            })}
          </span>
        </div>

        <div>
          <label className='block text-13px font-500 text-t-primary mb-6px'>
            {t('conversation.extractPreset.additionalGuidanceLabel', {
              defaultValue: '补充要求（选填）',
            })}
          </label>
          <Input.TextArea
            value={guidance}
            onChange={setGuidance}
            placeholder={t('conversation.extractPreset.additionalGuidancePlaceholder', {
              defaultValue: '例如：输出保持精炼中文、严格遵循 React 19 规范...',
            })}
            autoSize={{ minRows: 3, maxRows: 6 }}
            maxLength={500}
            showWordLimit
            disabled={isExtracting}
            className='rd-6px'
          />
        </div>
      </div>
    </Modal>
  );
};

export default ExtractPresetModal;
