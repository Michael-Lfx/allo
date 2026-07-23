/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Input, InputNumber } from '@arco-design/web-react';
import { BookOpen, FileText, Lightning, SettingTwo, VideoOne } from '@icon-park/react';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import type { VimaxWorkflow } from '../types';
import ModelSelectors, { type VimaxModelSelection } from './ModelSelectors';
import styles from '../index.module.css';

const TextArea = Input.TextArea;
const DRAFT_KEY = 'flowy.videoGeneration.draft.v1';

export interface VideoCreateDraft {
  workflow: VimaxWorkflow;
  sourceText: string;
  requirement: string;
  style: string;
  targetDurationSecs: number;
  models: VimaxModelSelection;
}

interface VideoCreateComposerProps {
  loading?: boolean;
  onSubmit: (draft: VideoCreateDraft) => void;
}

const EMPTY_MODELS: VimaxModelSelection = {
  llm_model: '',
  image_model: '',
  video_model: '',
};

function loadDraft(): VideoCreateDraft {
  const fallback: VideoCreateDraft = {
    workflow: 'idea2video',
    sourceText: '',
    requirement: '',
    style: '',
    targetDurationSecs: 30,
    models: EMPTY_MODELS,
  };
  try {
    const parsed = JSON.parse(window.sessionStorage.getItem(DRAFT_KEY) ?? '') as Partial<VideoCreateDraft>;
    const workflow =
      parsed.workflow === 'script2video' || parsed.workflow === 'novel2video'
        ? parsed.workflow
        : 'idea2video';
    return {
      workflow,
      sourceText: typeof parsed.sourceText === 'string' ? parsed.sourceText : '',
      requirement: typeof parsed.requirement === 'string' ? parsed.requirement : '',
      style: typeof parsed.style === 'string' ? parsed.style : '',
      targetDurationSecs:
        typeof parsed.targetDurationSecs === 'number' ? parsed.targetDurationSecs : 30,
      models: {
        llm_model: parsed.models?.llm_model ?? '',
        image_model: parsed.models?.image_model ?? '',
        video_model: parsed.models?.video_model ?? '',
      },
    };
  } catch {
    return fallback;
  }
}

export function clearVideoCreateDraft(): void {
  try {
    window.sessionStorage.removeItem(DRAFT_KEY);
  } catch {
    // Storage may be unavailable in hardened webviews.
  }
}

const VideoCreateComposer: React.FC<VideoCreateComposerProps> = ({ loading, onSubmit }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [draft, setDraft] = useState<VideoCreateDraft>(loadDraft);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [modelMissing, setModelMissing] = useState(false);
  const draftedTracked = useRef(false);

  useEffect(() => {
    try {
      window.sessionStorage.setItem(DRAFT_KEY, JSON.stringify(draft));
    } catch {
      // Storage may be unavailable in hardened webviews.
    }
  }, [draft]);

  useEffect(() => {
    if (draft.models.llm_model) setModelMissing(false);
  }, [draft.models.llm_model]);

  useEffect(() => {
    if (!draft.sourceText.trim() || draftedTracked.current) return;
    draftedTracked.current = true;
    trackFunnelEvent('task_drafted', {
      feature: 'video_generation',
      workflow: draft.workflow,
    });
  }, [draft.sourceText, draft.workflow]);

  const modes = useMemo(
    () => [
      {
        id: 'idea2video' as const,
        icon: <VideoOne theme='outline' size={14} />,
        label: t('videoGeneration.create.modes.idea', { defaultValue: '一个想法' }),
      },
      {
        id: 'script2video' as const,
        icon: <FileText theme='outline' size={14} />,
        label: t('videoGeneration.create.modes.script', { defaultValue: '完整剧本' }),
      },
      {
        id: 'novel2video' as const,
        icon: <BookOpen theme='outline' size={14} />,
        label: t('videoGeneration.create.modes.novel', { defaultValue: '小说文本' }),
      },
    ],
    [t]
  );

  const examples = useMemo(
    () => [
      t('videoGeneration.create.examples.product', {
        defaultValue: '为一款极简咖啡机制作 30 秒发布短片',
      }),
      t('videoGeneration.create.examples.story', {
        defaultValue: '雨夜里，最后一班列车驶入无人车站',
      }),
      t('videoGeneration.create.examples.brand', {
        defaultValue: '用电影感镜头讲述一双跑鞋的一天',
      }),
    ],
    [t]
  );

  const placeholder =
    draft.workflow === 'script2video'
      ? t('videoGeneration.create.composer.scriptPlaceholder', {
          defaultValue: '粘贴剧本，Nomi 会自动拆成可编辑镜头…',
        })
      : draft.workflow === 'novel2video'
        ? t('videoGeneration.create.composer.novelPlaceholder', {
            defaultValue: '粘贴小说片段，Nomi 会提炼剧情并设计分镜…',
          })
        : t('videoGeneration.create.composer.ideaPlaceholder', {
            defaultValue: '描述你想看到的故事、情绪或产品画面…',
          });

  const submit = () => {
    if (!draft.sourceText.trim()) return;
    if (!draft.models.llm_model) {
      setAdvancedOpen(true);
      setModelMissing(true);
      return;
    }
    onSubmit({ ...draft, sourceText: draft.sourceText.trim() });
  };

  return (
    <section className={`${styles.launchpad} px-18px py-22px md:px-32px md:py-30px`}>
      <div className='relative z-1 mx-auto flex max-w-820px flex-col items-center text-center'>
        <span className='mb-10px inline-flex items-center gap-6px rd-full bg-[rgba(var(--primary-6),0.1)] px-10px py-5px text-12px font-600 text-[rgb(var(--primary-6))]'>
          <Lightning theme='filled' size={13} fill='currentColor' />
          {t('videoGeneration.create.eyebrow', { defaultValue: 'AI 叙事导演' })}
        </span>
        <h1 className='m-0 max-w-680px text-26px font-750 leading-34px tracking-[-0.02em] text-[var(--color-text-1)] md:text-34px md:leading-43px'>
          {t('videoGeneration.create.heroTitle', { defaultValue: '一个想法，变成一支完整影片' })}
        </h1>
        <p className='mb-20px mt-8px max-w-620px text-13px leading-20px text-[var(--color-text-3)] md:text-14px'>
          {t('videoGeneration.create.heroSubtitle', {
            defaultValue: '先生成可编辑分镜，确认故事与画面后再渲染成片。',
          })}
        </p>

        <div className='mb-9px flex flex-wrap justify-center gap-4px rd-full bg-[var(--color-fill-1)] p-3px'>
          {modes.map((mode) => (
            <button
              key={mode.id}
              type='button'
              className={`${styles.modeButton} ${
                draft.workflow === mode.id ? styles.modeButtonActive : ''
              }`}
              aria-pressed={draft.workflow === mode.id}
              onClick={() => setDraft((current) => ({ ...current, workflow: mode.id }))}
            >
              {mode.icon}
              {mode.label}
            </button>
          ))}
        </div>

        <div className={`${styles.composer} w-full box-border p-12px text-left`}>
          <TextArea
            value={draft.sourceText}
            onChange={(sourceText) => setDraft((current) => ({ ...current, sourceText }))}
            placeholder={placeholder}
            autoSize={{ minRows: 4, maxRows: 10 }}
            disabled={loading}
            className='!border-none !bg-transparent !px-4px !text-15px !leading-24px !shadow-none'
            onKeyDown={(event) => {
              if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
                event.preventDefault();
                submit();
              }
            }}
          />
          <div className='mt-10px flex flex-wrap items-center justify-between gap-10px border-t border-b-0 border-l-0 border-r-0 border-solid border-[var(--color-border-2)] pt-10px'>
            <Button
              type='text'
              size='small'
              onClick={() => setAdvancedOpen((open) => !open)}
              aria-expanded={advancedOpen}
            >
              <span className='inline-flex items-center gap-5px text-[var(--color-text-2)]'>
                <SettingTwo theme='outline' size={14} />
                {t('videoGeneration.create.advanced', { defaultValue: '风格与模型' })}
                <span className='text-11px text-[var(--color-text-3)]'>
                  · {draft.targetDurationSecs}s
                </span>
              </span>
            </Button>
            <Button
              type='primary'
              size='large'
              loading={loading}
              disabled={!draft.sourceText.trim()}
              onClick={submit}
            >
              <span className='inline-flex items-center gap-7px font-600'>
                <Lightning theme='filled' size={15} fill='currentColor' />
                {t('videoGeneration.create.generateStoryboard', { defaultValue: '生成分镜' })}
              </span>
            </Button>
          </div>

          {advancedOpen ? (
            <div className='mt-12px flex flex-col gap-12px rd-12px bg-[var(--color-fill-1)] p-12px'>
              <div className='grid grid-cols-1 gap-10px md:grid-cols-[140px_1fr_1fr]'>
                <label className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
                  {t('videoGeneration.workspace.source.durationLabel', {
                    defaultValue: '目标时长（秒）',
                  })}
                  <InputNumber
                    value={draft.targetDurationSecs}
                    onChange={(value) =>
                      setDraft((current) => ({
                        ...current,
                        targetDurationSecs: typeof value === 'number' ? value : 30,
                      }))
                    }
                    min={5}
                    max={180}
                    step={5}
                    suffix='s'
                    disabled={loading}
                  />
                </label>
                <label className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
                  {t('videoGeneration.workspace.source.styleLabel', { defaultValue: '视觉风格' })}
                  <Input
                    value={draft.style}
                    onChange={(style) => setDraft((current) => ({ ...current, style }))}
                    placeholder={t('videoGeneration.workspace.source.stylePlaceholder', {
                      defaultValue: '如：电影写实、复古胶片',
                    })}
                    disabled={loading}
                  />
                </label>
                <label className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
                  {t('videoGeneration.workspace.source.requirementLabel', {
                    defaultValue: '额外要求',
                  })}
                  <Input
                    value={draft.requirement}
                    onChange={(requirement) =>
                      setDraft((current) => ({ ...current, requirement }))
                    }
                    placeholder={t('videoGeneration.workspace.source.requirementPlaceholder', {
                      defaultValue: '节奏、受众、画幅等',
                    })}
                    disabled={loading}
                  />
                </label>
              </div>
              <ModelSelectors
                value={draft.models}
                onChange={(models) => setDraft((current) => ({ ...current, models }))}
                disabled={loading}
              />
              {modelMissing ? (
                <div className='flex flex-wrap items-center justify-between gap-8px rd-8px bg-[rgba(var(--warning-6),0.1)] px-10px py-8px text-12px text-[var(--color-text-2)]'>
                  <span>
                    {t('videoGeneration.create.modelRequired', {
                      defaultValue: '生成分镜前需要一个可用的规划模型。',
                    })}
                  </span>
                  <Button type='text' size='mini' onClick={() => navigate('/models')}>
                    {t('videoGeneration.create.configureModels', { defaultValue: '前往模型中心' })}
                  </Button>
                </div>
              ) : null}
            </div>
          ) : null}
        </div>

        <div className='mt-12px flex w-full flex-wrap items-center justify-center gap-7px'>
          <span className='text-11px text-[var(--color-text-3)]'>
            {t('videoGeneration.create.tryExample', { defaultValue: '试试：' })}
          </span>
          {examples.map((example) => (
            <button
              key={example}
              type='button'
              className={styles.exampleChip}
              onClick={() => setDraft((current) => ({ ...current, sourceText: example }))}
            >
              {example}
            </button>
          ))}
        </div>
      </div>
    </section>
  );
};

export default VideoCreateComposer;
