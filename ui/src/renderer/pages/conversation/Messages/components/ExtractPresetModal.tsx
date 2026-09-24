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
      }, 500);
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
      let availableSkills: Array<{ skill_id?: string; id?: string; name?: string; description?: string }> = [];
      try {
        const catalog = await ipcBridge.fs.listSkillCatalog.invoke();
        if (Array.isArray(catalog?.skills)) {
          availableSkills = catalog.skills;
        }
      } catch (err) {
        console.warn('[ExtractPreset] Failed to fetch skill catalog:', err);
      }

      // 2. Small visual breathing delay for forging animation
      await new Promise((resolve) => setTimeout(resolve, 1000));

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
        maxWidth: 680,
        width: '92vw',
        borderRadius: 14,
        overflow: 'hidden',
      }}
    >
      <div className='flex flex-col gap-20px py-6px'>
        {/* Modal Header */}
        <div className='flex items-center gap-14px'>
          <div
            className='flex h-40px w-40px shrink-0 items-center justify-center rd-10px bg-primary-1 text-primary-6 [&>span]:flex [&>span]:items-center [&>span]:justify-center'
            style={{ lineHeight: 0 }}
          >
            <MagicWand theme='filled' size={20} fill='currentColor' />
          </div>
          <div className='min-w-0 flex-1 pr-16px'>
            <div className='flex items-center gap-8px'>
              <h2 className='m-0 text-16px font-600 text-t-primary leading-tight'>
                {t('conversation.extractPreset.title', { defaultValue: '提炼为专属设定' })}
              </h2>
              <span className='px-6px py-1px rd-4px text-10px font-medium bg-primary-1 text-primary-6 b-1 b-solid border-primary-2 leading-none'>
                AI Preset
              </span>
            </div>
            <p className='m-0 mt-6px text-13px text-t-secondary leading-relaxed'>
              {t('conversation.extractPreset.subtitle', {
                defaultValue: '从当前对话成果中智能提纯经验，生成包含提示词与技能绑定的独立设定。',
              })}
            </p>
          </div>
        </div>

        {/* Dynamic Body: Edit vs Extracting Motion State */}
        {isExtracting ? (
          <div className='py-32px flex flex-col items-center justify-center text-center animate-fade-in'>
            <div className='relative flex h-48px w-48px items-center justify-center mb-16px'>
              <div className='absolute inset-0 rd-full animate-ping opacity-20 bg-primary-6' />
              <div
                className='relative flex h-42px w-42px items-center justify-center rd-full bg-primary-1 text-primary-6 shadow-sm [&>span]:flex [&>span]:items-center [&>span]:justify-center'
                style={{ lineHeight: 0 }}
              >
                <Lightning theme='filled' size={22} fill='currentColor' className='animate-pulse' />
              </div>
            </div>

            <div className='text-13px font-500 text-t-primary transition-all duration-300 mb-14px'>
              {stepLabels[extractingStep]}
            </div>

            {/* Linear Animated Progress */}
            <div className='h-3px w-260px bg-fill-3 rd-full overflow-hidden'>
              <div
                className='h-full bg-primary-6 rd-full transition-all duration-500'
                style={{ width: `${((extractingStep + 1) / stepLabels.length) * 100}%` }}
              />
            </div>
          </div>
        ) : (
          <>
            {/* Workflow Process Cards */}
            <div className='grid grid-cols-3 gap-10px'>
              <div className='p-12px rd-10px bg-fill-1 b-1 b-solid border-arco-2 hover:border-primary-3 transition-colors'>
                <div className='flex items-center gap-6px text-13px font-600 text-t-primary mb-6px'>
                  <span
                    className='flex h-18px w-18px shrink-0 items-center justify-center rd-full bg-primary-1 text-primary-6 text-11px font-600 leading-none select-none text-center'
                    style={{ lineHeight: 1 }}
                  >
                    1
                  </span>
                  <span>{t('conversation.extractPreset.flowStep1Title', { defaultValue: '角色定位归纳' })}</span>
                </div>
                <p className='m-0 text-12px text-t-secondary leading-relaxed whitespace-nowrap overflow-hidden text-ellipsis'>
                  {t('conversation.extractPreset.flowStep1Desc', {
                    defaultValue: '提炼角色定位与核心职责',
                  })}
                </p>
              </div>

              <div className='p-12px rd-10px bg-fill-1 b-1 b-solid border-arco-2 hover:border-primary-3 transition-colors'>
                <div className='flex items-center gap-6px text-13px font-600 text-t-primary mb-6px'>
                  <span
                    className='flex h-18px w-18px shrink-0 items-center justify-center rd-full bg-primary-1 text-primary-6 text-11px font-600 leading-none select-none text-center'
                    style={{ lineHeight: 1 }}
                  >
                    2
                  </span>
                  <span>{t('conversation.extractPreset.flowStep2Title', { defaultValue: '工作流沉淀' })}</span>
                </div>
                <p className='m-0 text-12px text-t-secondary leading-relaxed whitespace-nowrap overflow-hidden text-ellipsis'>
                  {t('conversation.extractPreset.flowStep2Desc', {
                    defaultValue: '沉淀系统提示词与交互规范',
                  })}
                </p>
              </div>

              <div className='p-12px rd-10px bg-fill-1 b-1 b-solid border-arco-2 hover:border-primary-3 transition-colors'>
                <div className='flex items-center gap-6px text-13px font-600 text-t-primary mb-6px'>
                  <span
                    className='flex h-18px w-18px shrink-0 items-center justify-center rd-full bg-primary-1 text-primary-6 text-11px font-600 leading-none select-none text-center'
                    style={{ lineHeight: 1 }}
                  >
                    3
                  </span>
                  <span>{t('conversation.extractPreset.flowStep3Title', { defaultValue: '适用技能装配' })}</span>
                </div>
                <p className='m-0 text-12px text-t-secondary leading-relaxed whitespace-nowrap overflow-hidden text-ellipsis'>
                  {t('conversation.extractPreset.flowStep3Desc', {
                    defaultValue: '智能匹配并装配适用技能',
                  })}
                </p>
              </div>
            </div>

            {/* Customization Guidance */}
            <div>
              <label className='block text-13px font-500 text-t-primary mb-8px'>
                {t('conversation.extractPreset.additionalGuidanceLabel', {
                  defaultValue: '补充定制要求（选填）',
                })}
              </label>
              <Input.TextArea
                value={guidance}
                onChange={setGuidance}
                placeholder={t('conversation.extractPreset.additionalGuidancePlaceholder', {
                  defaultValue:
                    '可在此补充角色偏好或输出约束（如保持中文、特定代码规范等），留空将全自动提炼',
                })}
                autoSize={{ minRows: 3, maxRows: 4 }}
                maxLength={500}
                showWordLimit
                className='rd-8px !bg-fill-1 b-1 b-solid border-arco-2 focus:border-primary-5 transition-all text-13px leading-relaxed'
                style={{
                  boxShadow: 'inset 0 1px 2px rgba(0,0,0,0.02)',
                }}
              />
            </div>
          </>
        )}

        {/* Modal Footer */}
        {!isExtracting && (
          <div className='flex items-center justify-end gap-10px pt-16px mt-2px border-t border-t-solid border-arco-1'>
            <Button
              onClick={onCancel}
              className='rd-6px px-16px'
            >
              {t('conversation.extractPreset.cancel', { defaultValue: '取消' })}
            </Button>
            <Button
              type='primary'
              onClick={handleStartExtract}
              data-testid='btn-confirm-extract-preset'
              className='rd-6px px-18px font-500 shadow-sm'
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
