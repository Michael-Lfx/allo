/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useState } from 'react';
import { Button, Input, Modal } from '@arco-design/web-react';
import {
  Brain,
  Lightning,
  Magic,
  MagicWand,
  Plus,
  TagOne,
} from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import classNames from 'classnames';
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

const QUICK_TAG_KEYS = [
  { id: 'concise', labelKey: 'conversation.extractPreset.tagConcise', defaultText: '保持精炼输出' },
  { id: 'chinese', labelKey: 'conversation.extractPreset.tagChinese', defaultText: '中文详细解答' },
  { id: 'standard', labelKey: 'conversation.extractPreset.tagStandard', defaultText: '严格遵循规范' },
  { id: 'comments', labelKey: 'conversation.extractPreset.tagComments', defaultText: '包含详尽注释' },
  { id: 'stepByStep', labelKey: 'conversation.extractPreset.tagStepByStep', defaultText: '结构化分步解答' },
] as const;

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
      }, 600);
    } else {
      setExtractingStep(0);
    }
    return () => {
      if (timer) clearInterval(timer);
    };
  }, [isExtracting]);

  const handleTagClick = (tagText: string) => {
    if (isExtracting) return;
    setGuidance((prev) => {
      const trimmed = prev.trim();
      if (!trimmed) return tagText;
      if (trimmed.includes(tagText)) return trimmed;
      return `${trimmed}，${tagText}`;
    });
  };

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
      await new Promise((resolve) => setTimeout(resolve, 1400));

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
      defaultValue: '正在深度分析对话上下文与核心意图...',
    }),
    t('conversation.extractPreset.stepStructuring', {
      defaultValue: '正在构建结构化工作流与提示词约束...',
    }),
    t('conversation.extractPreset.stepMatchingSkills', {
      defaultValue: '正在匹配并装配最佳适用技能...',
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
        maxWidth: 580,
        width: '92vw',
        borderRadius: 16,
        padding: 0,
        overflow: 'hidden',
      }}
    >
      <div className='relative overflow-hidden p-24px pt-20px'>
        {/* Background Atmospheric Glow */}
        <div
          className='pointer-events-none absolute -top-100px left-1/2 -translate-x-1/2 h-180px w-360px rd-full opacity-35 blur-3xl'
          style={{
            background:
              'radial-gradient(circle, var(--color-primary-5, #3c7eff) 0%, rgba(99, 102, 241, 0.4) 50%, transparent 80%)',
          }}
          aria-hidden='true'
        />

        {/* Modal Header */}
        <div className='relative flex items-center justify-between mb-16px'>
          <div className='flex items-center gap-10px'>
            <div
              className='flex h-36px w-36px shrink-0 items-center justify-center rd-10px text-white shadow-sm'
              style={{
                background:
                  'linear-gradient(135deg, var(--color-primary-6, #165dff) 0%, #7c3aed 100%)',
              }}
            >
              <MagicWand theme='filled' size={20} fill='currentColor' />
            </div>
            <div>
              <h2 className='m-0 text-16px font-600 text-t-primary leading-tight flex items-center gap-6px'>
                <span>{t('conversation.extractPreset.title', { defaultValue: '提炼为专属设定' })}</span>
                <span className='px-6px py-1px rd-4px text-10px font-medium bg-primary-1 text-primary-6 b-1 b-solid border-primary-2'>
                  AI Presets
                </span>
              </h2>
              <p className='m-0 mt-3px text-12px text-t-secondary leading-normal'>
                {t('conversation.extractPreset.subtitle', {
                  defaultValue: '从当前对话中提纯经验与上下文，沉淀为可即刻复用的智能体设定',
                })}
              </p>
            </div>
          </div>
        </div>

        {/* 3-Pillar Distillation Value Matrix */}
        <div className='grid grid-cols-3 gap-8px mb-18px'>
          <div className='p-10px rd-8px bg-fill-1 b-1 b-solid border-arco-2 hover:border-primary-3 transition-colors group'>
            <div className='flex items-center gap-5px text-12px font-500 text-t-primary mb-3px'>
              <TagOne theme='filled' size={14} className='text-primary-6' />
              <span>{t('conversation.extractPreset.pillarRole', { defaultValue: '专属角色定位' })}</span>
            </div>
            <div className='text-11px text-t-secondary leading-tight line-clamp-2'>
              {t('conversation.extractPreset.pillarRoleDesc', {
                defaultValue: '自动归纳角色命名与职责简介',
              })}
            </div>
          </div>

          <div className='p-10px rd-8px bg-fill-1 b-1 b-solid border-arco-2 hover:border-primary-3 transition-colors group'>
            <div className='flex items-center gap-5px text-12px font-500 text-t-primary mb-3px'>
              <Brain theme='filled' size={14} className='text-primary-6' />
              <span>{t('conversation.extractPreset.pillarPrompt', { defaultValue: '工作流规范' })}</span>
            </div>
            <div className='text-11px text-t-secondary leading-tight line-clamp-2'>
              {t('conversation.extractPreset.pillarPromptDesc', {
                defaultValue: '提炼结构化 System Prompt',
              })}
            </div>
          </div>

          <div className='p-10px rd-8px bg-fill-1 b-1 b-solid border-arco-2 hover:border-primary-3 transition-colors group'>
            <div className='flex items-center gap-5px text-12px font-500 text-t-primary mb-3px'>
              <Lightning theme='filled' size={14} className='text-primary-6' />
              <span>{t('conversation.extractPreset.pillarSkills', { defaultValue: '智能技能匹配' })}</span>
            </div>
            <div className='text-11px text-t-secondary leading-tight line-clamp-2'>
              {t('conversation.extractPreset.pillarSkillsDesc', {
                defaultValue: '嗅探并推荐绑定适用工具',
              })}
            </div>
          </div>
        </div>

        {/* Human Guidance Customization Area */}
        <div className='mb-20px'>
          <div className='flex items-center justify-between mb-8px'>
            <label className='flex items-center gap-5px text-13px font-500 text-t-primary'>
              <Magic theme='filled' size={14} className='text-primary-5' />
              <span>
                {t('conversation.extractPreset.additionalGuidanceLabel', {
                  defaultValue: '补充定制要求（选填）',
                })}
              </span>
            </label>
            <span className='text-11px text-t-tertiary'>
              {t('conversation.extractPreset.quickTagsTitle', { defaultValue: '灵感快捷要求' })}
            </span>
          </div>

          {/* Quick Suggestion Chips */}
          <div className='flex flex-wrap gap-6px mb-8px'>
            {QUICK_TAG_KEYS.map((tag) => {
              const label = t(tag.labelKey, { defaultValue: tag.defaultText });
              const isSelected = guidance.includes(label);
              return (
                <button
                  key={tag.id}
                  type='button'
                  disabled={isExtracting}
                  onClick={() => handleTagClick(label)}
                  className={classNames(
                    'px-9px py-4px rd-6px text-12px transition-all cursor-pointer flex items-center gap-4px select-none',
                    isSelected
                      ? 'bg-primary-1 text-primary-6 font-medium shadow-sm b-1 b-solid border-primary-5'
                      : 'bg-fill-2 text-t-secondary hover:bg-fill-3 hover:text-t-primary b-1 b-solid border-transparent'
                  )}
                  style={{
                    transform: 'translateZ(0)',
                    transition: 'all 0.15s cubic-bezier(0.16, 1, 0.3, 1)',
                  }}
                >
                  <Plus theme='outline' size={12} className={isSelected ? 'text-primary-6' : 'opacity-60'} />
                  <span>{label}</span>
                </button>
              );
            })}
          </div>

          {/* Guidance Input Box */}
          <div className='relative'>
            <Input.TextArea
              value={guidance}
              onChange={setGuidance}
              placeholder={t('conversation.extractPreset.additionalGuidancePlaceholder', {
                defaultValue:
                  '可点击上方灵感标签，或直接输入自定义要求（例如：输出保持精炼中文、严格遵循 React 19 规范...）',
              })}
              autoSize={{ minRows: 3, maxRows: 5 }}
              maxLength={500}
              showWordLimit
              disabled={isExtracting}
              className='rd-8px !bg-fill-1 b-1 b-solid border-arco-2 focus:border-primary-5 transition-all text-13px leading-relaxed'
              style={{
                boxShadow: 'inset 0 1px 2px rgba(0,0,0,0.04)',
              }}
            />
          </div>
        </div>

        {/* Animated Forging Progress Stage (Visible when extracting) */}
        {isExtracting && (
          <div
            className='mb-18px p-12px rd-10px b-1 b-solid border-primary-3 bg-[rgba(var(--primary-6),0.08)] animate-fade-in flex items-center gap-10px'
            style={{
              backdropFilter: 'blur(8px)',
            }}
          >
            <div className='flex h-24px w-24px shrink-0 items-center justify-center animate-spin'>
              <Lightning theme='filled' size={16} fill='var(--color-primary-6)' />
            </div>
            <div className='min-w-0 flex-1'>
              <div className='text-12px font-500 text-primary-6 transition-all duration-300'>
                {stepLabels[extractingStep]}
              </div>
              <div className='h-3px w-full bg-primary-2 rd-full overflow-hidden mt-6px'>
                <div
                  className='h-full bg-primary-6 rd-full transition-all duration-500'
                  style={{ width: `${((extractingStep + 1) / stepLabels.length) * 100}%` }}
                />
              </div>
            </div>
          </div>
        )}

        {/* Modal Footer Controls */}
        <div className='flex items-center justify-between pt-12px border-t border-t-solid border-arco-1'>
          <div className='text-11px text-t-tertiary flex items-center gap-4px hidden sm:flex'>
            <Magic theme='outline' size={13} className='text-primary-5' />
            <span>
              {t('conversation.extractPreset.footerNote', {
                defaultValue: '提炼完成后将自动拉起设定抽屉，您可二次润色微调后保存入库',
              })}
            </span>
          </div>

          <div className='flex items-center gap-8px ml-auto'>
            <Button
              disabled={isExtracting}
              onClick={onCancel}
              className='rd-6px px-16px font-medium'
            >
              {t('conversation.extractPreset.cancel', { defaultValue: '取消' })}
            </Button>
            <Button
              type='primary'
              loading={isExtracting}
              onClick={handleStartExtract}
              data-testid='btn-confirm-extract-preset'
              className='rd-6px px-18px font-500 shadow-sm'
              style={{
                background:
                  'linear-gradient(135deg, var(--color-primary-6, #165dff) 0%, #7c3aed 100%)',
                border: 'none',
              }}
            >
              {isExtracting
                ? t('conversation.extractPreset.extracting', { defaultValue: '正在智能锻造中...' })
                : t('conversation.extractPreset.startExtract', { defaultValue: '开始智能提炼' })}
            </Button>
          </div>
        </div>
      </div>
    </Modal>
  );
};

export default ExtractPresetModal;
