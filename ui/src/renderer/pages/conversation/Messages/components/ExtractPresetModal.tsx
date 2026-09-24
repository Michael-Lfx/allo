/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useState } from 'react';
import { Button, Input, Modal } from '@arco-design/web-react';
import {
  Lightning,
  MagicWand,
} from '@icon-park/react';
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
  const [extractingStep, setExtractingStep] = useState(0);

  useEffect(() => {
    let timer: ReturnType<typeof setInterval> | null = null;
    if (isExtracting) {
      setExtractingStep(0);
      timer = setInterval(() => {
        setExtractingStep((prev) => (prev < 2 ? prev + 1 : prev));
      }, 550);
    } else {
      setExtractingStep(0);
    }
    return () => {
      if (timer) clearInterval(timer);
    };
  }, [isExtracting]);

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

      // 2. Small visual breathing delay for forging animation
      await new Promise((resolve) => setTimeout(resolve, 1300));

      // 3. Build structured extraction draft
      const draft: PresetDraftData = buildPresetExtractionDraft({
        messageText,
        userQuestion,
        conversationTitle,
        userGuidance: guidance,
        availableSkills,
      });

      // 4. Close modal and navigate to preset editor drawer
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

  const stepLabels = [
    t('conversation.extractPreset.stepAnalyzing', {
      defaultValue: '正在分析对话上下文与核心意图...',
    }),
    t('conversation.extractPreset.stepStructuring', {
      defaultValue: '正在构建工作流与系统提示词...',
    }),
    t('conversation.extractPreset.stepMatchingSkills', {
      defaultValue: '正在匹配并装配适用技能...',
    }),
  ];

  return (
    <Modal
      title={null}
      visible={visible}
      onCancel={() => {
        if (!isExtracting) {
          setGuidance('');
          onCancel();
        }
      }}
      footer={null}
      className='flowy-modal flowy-extract-preset-modal'
      style={{
        maxWidth: 480,
        width: '90vw',
        borderRadius: 16,
        padding: 0,
        overflow: 'hidden',
      }}
    >
      <div className='relative overflow-hidden p-24px'>
        {/* Background Atmospheric Glow */}
        <div
          className='pointer-events-none absolute -top-80px left-1/2 -translate-x-1/2 h-160px w-320px rd-full opacity-25 blur-3xl'
          style={{
            background:
              'radial-gradient(circle, var(--color-primary-5, #3c7eff) 0%, rgba(99, 102, 241, 0.3) 50%, transparent 80%)',
          }}
          aria-hidden='true'
        />

        {/* Modal Header */}
        <div className='relative flex items-start gap-12px mb-20px'>
          <div
            className='flex h-38px w-38px shrink-0 items-center justify-center rd-10px text-white shadow-sm mt-2px'
            style={{
              background:
                'linear-gradient(135deg, var(--color-primary-6, #165dff) 0%, #7c3aed 100%)',
            }}
          >
            <MagicWand theme='filled' size={20} fill='currentColor' />
          </div>
          <div className='min-w-0 flex-1 pr-16px'>
            <h2 className='m-0 text-16px font-600 text-t-primary leading-snug'>
              {t('conversation.extractPreset.title', { defaultValue: '提炼为专属设定' })}
            </h2>
            <p className='m-0 mt-4px text-12px text-t-secondary leading-relaxed'>
              {t('conversation.extractPreset.subtitle', {
                defaultValue: '从当前对话成果中智能提纯经验，生成包含提示词与技能绑定的独立设定。',
              })}
            </p>
          </div>
        </div>

        {/* Dynamic Body: Edit vs Extracting Motion State */}
        {isExtracting ? (
          <div className='py-28px flex flex-col items-center justify-center text-center animate-fade-in'>
            <div className='relative flex h-52px w-52px items-center justify-center mb-16px'>
              {/* Outer Pulsing Glow Ring */}
              <div
                className='absolute inset-0 rd-full animate-ping opacity-25'
                style={{ background: 'var(--color-primary-6, #165dff)' }}
              />
              <div
                className='relative flex h-44px w-44px items-center justify-center rd-full shadow-md text-white'
                style={{
                  background:
                    'linear-gradient(135deg, var(--color-primary-6, #165dff) 0%, #7c3aed 100%)',
                }}
              >
                <Lightning theme='filled' size={22} fill='currentColor' className='animate-pulse' />
              </div>
            </div>

            <div className='text-13px font-500 text-t-primary transition-all duration-300 mb-12px'>
              {stepLabels[extractingStep]}
            </div>

            {/* Linear Animated Progress */}
            <div className='h-3px w-200px bg-fill-3 rd-full overflow-hidden'>
              <div
                className='h-full bg-primary-6 rd-full transition-all duration-500'
                style={{ width: `${((extractingStep + 1) / stepLabels.length) * 100}%` }}
              />
            </div>
          </div>
        ) : (
          <div className='mb-20px'>
            <div className='mb-8px'>
              <label className='text-12px font-500 text-t-primary'>
                {t('conversation.extractPreset.additionalGuidanceLabel', {
                  defaultValue: '补充定制要求（选填）',
                })}
              </label>
            </div>
            <Input.TextArea
              value={guidance}
              onChange={setGuidance}
              placeholder={t('conversation.extractPreset.additionalGuidancePlaceholder', {
                defaultValue:
                  '可在此补充对设定角色的要求或偏好（例如：输出保持中文、遵循特定代码规范等），留空将自动智能提炼',
              })}
              autoSize={{ minRows: 3, maxRows: 5 }}
              maxLength={500}
              showWordLimit
              className='rd-8px !bg-fill-1 b-1 b-solid border-arco-2 focus:border-primary-5 transition-all text-13px leading-relaxed'
              style={{
                boxShadow: 'inset 0 1px 2px rgba(0,0,0,0.03)',
              }}
            />
          </div>
        )}

        {/* Modal Footer */}
        {!isExtracting && (
          <div className='flex items-center justify-end gap-8px pt-14px border-t border-t-solid border-arco-1'>
            <Button
              onClick={onCancel}
              className='rd-6px px-16px font-medium'
            >
              {t('conversation.extractPreset.cancel', { defaultValue: '取消' })}
            </Button>
            <Button
              type='primary'
              onClick={handleStartExtract}
              data-testid='btn-confirm-extract-preset'
              className='rd-6px px-18px font-500 shadow-sm'
              style={{
                background:
                  'linear-gradient(135deg, var(--color-primary-6, #165dff) 0%, #7c3aed 100%)',
                border: 'none',
              }}
            >
              {t('conversation.extractPreset.startExtract', { defaultValue: '开始智能提炼' })}
            </Button>
          </div>
        )}
      </div>
    </Modal>
  );
};

export default ExtractPresetModal;
