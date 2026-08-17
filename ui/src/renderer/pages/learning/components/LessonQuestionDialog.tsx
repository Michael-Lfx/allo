import {
  Alert,
  Button,
  Input,
  Message,
  Modal,
  Radio,
  Select,
  Tabs,
  Typography,
} from '@arco-design/web-react';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import type {
  ActivityKind,
  ConceptRef,
  CreateLessonActivityRequest,
  GeneratedLessonActivity,
  Lesson,
} from '../types';
import { errorMessage } from '../utils';
import { useLearningAutogenModel } from './LearningModelSelector';
import { FillBlankEditor, OptionEditor, TrueFalseAnswer } from './QuestionDialogs';

const { Text } = Typography;

const KIND_OPTIONS: { kind: ActivityKind; labelKey: string }[] = [
  { kind: 'single_choice', labelKey: 'learning.kindSingleChoice' },
  { kind: 'true_false', labelKey: 'learning.kindTrueFalse' },
  { kind: 'fill_in_blank', labelKey: 'learning.kindFillInBlank' },
  { kind: 'reflection', labelKey: 'learning.kindReflection' },
];

/** 向课时追加练习：手动创建任意 4 种题型，或 AI 参考课时内容与已存在问题生成新题草案 */
export function LessonQuestionDialog({
  lesson,
  onClose,
  onSaved,
}: {
  lesson: Lesson;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  // 生成使用学习页统一的模型偏好；未选择时后端回落默认模型
  const { choice: modelChoice } = useLearningAutogenModel();
  const [tab, setTab] = useState<'manual' | 'ai'>('manual');
  const [kind, setKind] = useState<ActivityKind>('single_choice');
  const [prompt, setPrompt] = useState('');
  const [options, setOptions] = useState<string[]>(['', '']);
  const [answer, setAnswer] = useState<unknown>(undefined);
  const [answers, setAnswers] = useState<string[]>(['']);
  const [distractors, setDistractors] = useState<string[]>([]);
  const [explanation, setExplanation] = useState('');
  const [conceptIds, setConceptIds] = useState<string[]>([]);
  const [conceptRefs, setConceptRefs] = useState<ConceptRef[]>([]);
  const [conceptsLoading, setConceptsLoading] = useState(false);
  const [focus, setFocus] = useState('');
  const [generating, setGenerating] = useState(false);
  const [generated, setGenerated] = useState(false);
  const [generateError, setGenerateError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const isSingleChoice = kind === 'single_choice';
  const isTrueFalse = kind === 'true_false';
  const isFillInBlank = kind === 'fill_in_blank';
  const isReflection = kind === 'reflection';
  const locked = busy || generating;

  useEffect(() => {
    let cancelled = false;
    setConceptsLoading(true);
    learningApi
      .listConceptRefs()
      .then((refs) => {
        if (!cancelled) setConceptRefs(refs);
      })
      .catch(() => {
        // 概念绑定可选；列表加载失败不阻塞对话框
      })
      .finally(() => {
        if (!cancelled) setConceptsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const switchKind = (next: ActivityKind) => {
    if (next === kind) return;
    setKind(next);
    setAnswer(undefined);
    setAnswers(['']);
    setDistractors([]);
  };

  /** 表单校验；返回 i18n 键，通过返回 null。反思题仅要求题干非空 */
  const validate = (): string | null => {
    if (prompt.trim().length === 0) {
      return 'learning.questionPromptRequired';
    }
    if (isSingleChoice) {
      const cleanedOptions = options.map((option) => option.trim()).filter((option) => option !== '');
      if (cleanedOptions.length < 2) {
        return 'learning.questionOptionsRequired';
      }
      if (typeof answer !== 'string' || !cleanedOptions.includes(answer)) {
        return 'learning.questionAnswerInvalid';
      }
    } else if (isFillInBlank) {
      if (!prompt.includes('___')) {
        return 'learning.questionFillBlankHintRequired';
      }
      const cleanedAnswers = answers.map((item) => item.trim()).filter((item) => item !== '');
      if (cleanedAnswers.length === 0 || cleanedAnswers.length > 3) {
        return 'learning.questionAnswerInvalid';
      }
    } else if (isTrueFalse && typeof answer !== 'boolean') {
      return 'learning.questionAnswerInvalid';
    }
    return null;
  };

  /** 组装提交体：反思题 answer 为 null；concept_ids 留空时后端绑定课时全部概念 */
  const buildRequest = (): CreateLessonActivityRequest | null => {
    const errorKey = validate();
    if (errorKey !== null) {
      Message.error(t(errorKey));
      return null;
    }
    const cleanedOptions = options.map((option) => option.trim()).filter((option) => option !== '');
    const cleanedAnswers = answers.map((item) => item.trim()).filter((item) => item !== '');
    return {
      kind,
      prompt: prompt.trim(),
      options: isSingleChoice ? cleanedOptions : [],
      answer: isFillInBlank ? cleanedAnswers : isReflection ? null : answer,
      explanation: explanation.trim(),
      distractors: isFillInBlank ? distractors : undefined,
      concept_ids: conceptIds,
    };
  };

  const submit = async () => {
    const request = buildRequest();
    if (request === null) return;
    setBusy(true);
    try {
      await learningApi.createLessonActivity(lesson.id, request);
      Message.success(t('learning.questionAddedToLesson'));
      onSaved();
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    } finally {
      setBusy(false);
    }
  };

  /** 生成失败时保留表单与错误提示，用户可直接重试（后端内部已带一次校正重试） */
  const generate = async () => {
    setGenerating(true);
    setGenerateError(null);
    try {
      const draft = await learningApi.generateLessonActivity(lesson.id, {
        kind,
        provider_id: modelChoice?.provider_id,
        model: modelChoice?.model,
        focus: focus.trim() || undefined,
      });
      applyDraft(draft);
    } catch (actionError) {
      const message = errorMessage(t, actionError);
      setGenerateError(message);
      Message.error(message);
    } finally {
      setGenerating(false);
    }
  };

  /** 用生成结果填充共享编辑区，用户可继续修改后确认添加 */
  const applyDraft = (draft: GeneratedLessonActivity) => {
    setKind(draft.kind);
    setPrompt(draft.prompt);
    setOptions(draft.options.length >= 2 ? draft.options : [...draft.options, '']);
    setAnswer(
      typeof draft.answer === 'string' || typeof draft.answer === 'boolean'
        ? draft.answer
        : undefined
    );
    setAnswers(Array.isArray(draft.answer) ? (draft.answer as string[]) : ['']);
    setDistractors(draft.distractors);
    setExplanation(draft.explanation);
    setConceptIds(draft.concept_ids);
    setGenerated(true);
  };

  const kindRadios = (
    <Radio.Group value={kind} onChange={(value) => switchKind(value as ActivityKind)}>
      {KIND_OPTIONS.map((option) => (
        <Radio key={option.kind} value={option.kind}>
          {t(option.labelKey)}
        </Radio>
      ))}
    </Radio.Group>
  );

  /** 题干 + 各题型专属编辑区 + 解析（手动与 AI 草案共用） */
  const formFields = (
    <div className='flex flex-col gap-14px'>
      <div>
        <div className='mb-6px font-500'>{t('learning.questionPromptLabel')}</div>
        <Input.TextArea
          value={prompt}
          onChange={setPrompt}
          autoSize={{ minRows: 2 }}
        />
        {isFillInBlank && (
          <Text type='secondary' className='text-12px'>
            {t('learning.questionFillBlankHint')}
          </Text>
        )}
      </div>
      {isSingleChoice && (
        <div>
          <div className='mb-6px font-500'>{t('learning.questionOptions')}</div>
          <OptionEditor
            options={options}
            answer={answer}
            onOptionsChange={setOptions}
            onAnswerChange={setAnswer}
            t={t}
          />
          <Text type='secondary' className='text-12px'>
            {t('learning.questionAnswerHint')}
          </Text>
        </div>
      )}
      {isTrueFalse && (
        <div>
          <div className='mb-6px font-500'>{t('learning.questionAnswer')}</div>
          <TrueFalseAnswer answer={answer} onChange={setAnswer} t={t} />
        </div>
      )}
      {isFillInBlank && (
        <div>
          <div className='mb-6px font-500'>{t('learning.questionAcceptedAnswers')}</div>
          <FillBlankEditor
            answers={answers}
            distractors={distractors}
            onAnswersChange={setAnswers}
            onDistractorsChange={setDistractors}
            t={t}
          />
        </div>
      )}
      <div>
        <div className='mb-6px font-500'>{t('learning.questionExplanation')}</div>
        <Input.TextArea
          value={explanation}
          onChange={setExplanation}
          autoSize={{ minRows: 2 }}
        />
      </div>
    </div>
  );

  const conceptPicker = (
    <div>
      <div className='mb-6px font-500'>{t('learning.questionConceptBind')}</div>
      <Select
        className='w-full'
        mode='multiple'
        allowClear
        loading={conceptsLoading}
        value={conceptIds}
        placeholder={t('learning.questionConceptBindPlaceholder')}
        onChange={(value: string[]) => setConceptIds(value)}
      >
        {conceptRefs.map((concept) => (
          <Select.Option key={concept.concept_id} value={concept.concept_id}>
            {concept.title}
            {concept.course_title !== null ? ` · ${concept.course_title}` : ''}
          </Select.Option>
        ))}
      </Select>
      <Text type='secondary' className='text-12px'>
        {t('learning.questionConceptBindLessonHint')}
      </Text>
    </div>
  );

  return (
    <Modal
      title={t('learning.lessonAddQuestion')}
      visible
      style={{ width: 640 }}
      footer={null}
      closable={!locked}
      maskClosable={!locked}
      onCancel={onClose}
    >
      <Tabs activeTab={tab} onChange={(key) => setTab(key as 'manual' | 'ai')}>
        <Tabs.TabPane key='manual' title={t('learning.addQuestionManual')}>
          {/* 内联限高 + 滚动：AI 生成的长表单（长题干/多选项/长解析）超出弹窗范围时内部滚动，
              避免撑破弹窗；不用 UnoCSS 任意值类以保证限高在任何构建下都生效 */}
          <div
            style={{
              maxHeight: 'calc(100vh - 260px)',
              overflowY: 'auto',
              scrollbarWidth: 'thin',
              scrollbarColor: 'color-mix(in srgb, var(--color-text-3) 52%, transparent) transparent',
            }}
          >
            <div className='flex flex-col gap-14px'>
              <div>
                <div className='mb-6px font-500'>{t('learning.questionKind')}</div>
                {kindRadios}
              </div>
              {formFields}
              {conceptPicker}
              <div className='flex justify-end'>
                <Button type='primary' loading={busy} disabled={generating} onClick={() => void submit()}>
                  {t('learning.questionAddConfirm')}
                </Button>
              </div>
            </div>
          </div>
        </Tabs.TabPane>
        <Tabs.TabPane key='ai' title={t('learning.addQuestionAi')}>
          <div
            style={{
              maxHeight: 'calc(100vh - 260px)',
              overflowY: 'auto',
              scrollbarWidth: 'thin',
              scrollbarColor: 'color-mix(in srgb, var(--color-text-3) 52%, transparent) transparent',
            }}
          >
            <div className='flex flex-col gap-14px'>
              <div>
                <div className='mb-6px font-500'>{t('learning.questionKind')}</div>
                {kindRadios}
              </div>
              <div>
                <div className='mb-6px font-500'>{t('learning.questionFocusLabel')}</div>
                <Input
                  value={focus}
                  onChange={setFocus}
                  placeholder={t('learning.questionFocusPlaceholder')}
                />
              </div>
              <div className='flex items-center gap-10px'>
                <Button
                  type='primary'
                  loading={generating}
                  disabled={busy}
                  onClick={() => void generate()}
                >
                  {generated
                    ? t('learning.regenerateQuestion')
                    : t('learning.generateQuestion')}
                </Button>
                <Text type='secondary' className='text-12px'>
                  {t('learning.questionNoveltyHint')}
                </Text>
              </div>
              <Text type='secondary' className='text-12px'>
                {t('learning.generateQuestionHint')}
              </Text>
              {generateError && <Alert type='error' content={generateError} />}
              {generated && (
                <>
                  <Alert type='success' content={t('learning.questionGenerated')} />
                  {formFields}
                  {conceptPicker}
                  <div className='flex justify-end'>
                    <Button
                      type='primary'
                      loading={busy}
                      disabled={generating}
                      onClick={() => void submit()}
                    >
                      {t('learning.questionAddConfirm')}
                    </Button>
                  </div>
                </>
              )}
            </div>
          </div>
        </Tabs.TabPane>
      </Tabs>
    </Modal>
  );
}
