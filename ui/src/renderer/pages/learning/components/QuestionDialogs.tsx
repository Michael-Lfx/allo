import {
  Button,
  Drawer,
  Input,
  Message,
  Modal,
  Radio,
  Select,
  Tag,
  Typography,
} from '@arco-design/web-react';
import { useEffect, useState } from 'react';
import { IconPushpin } from '@arco-design/web-react/icon';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import type { ConceptRef, QuestionEntry } from '../types';
import {
  errorMessage,
  formatReviewTime,
  questionStateMeta,
  validateQuestionForm,
  type Translate,
} from '../utils';

const { Paragraph, Text } = Typography;

/** 单选题选项编辑列表：勾选正确答案、增删改选项（创建 / 编辑题目共用） */
export function OptionEditor({
  options,
  answer,
  onOptionsChange,
  onAnswerChange,
  t,
}: {
  options: string[];
  answer: unknown;
  onOptionsChange: (next: string[]) => void;
  onAnswerChange: (next: unknown) => void;
  t: Translate;
}) {
  return (
    <div className='flex flex-col gap-6px'>
      {options.map((option, index) => (
        <div key={index} className='flex items-center gap-6px'>
          <Radio checked={answer === option} onChange={() => onAnswerChange(option)} />
          <Input
            value={option}
            onChange={(value) =>
              onOptionsChange(
                options.map((item, itemIndex) => (itemIndex === index ? value : item))
              )
            }
          />
          <Button
            size='mini'
            type='text'
            status='danger'
            disabled={options.length <= 2}
            onClick={() => {
              onOptionsChange(options.filter((_, itemIndex) => itemIndex !== index));
              if (answer === option) {
                onAnswerChange(undefined);
              }
            }}
          >
            {t('learning.questionOptionRemove')}
          </Button>
        </div>
      ))}
      <div>
        <Button size='small' onClick={() => onOptionsChange([...options, ''])}>
          {t('learning.questionOptionAdd')}
        </Button>
      </div>
    </div>
  );
}

/** 判断题答案选择（创建 / 编辑题目共用） */
export function TrueFalseAnswer({
  answer,
  onChange,
  t,
}: {
  answer: unknown;
  onChange: (next: unknown) => void;
  t: Translate;
}) {
  return (
    <Radio.Group
      value={answer === true ? 'true' : answer === false ? 'false' : undefined}
      onChange={(value) => onChange(value === 'true')}
    >
      <Radio value='true'>{t('learning.trueLabel')}</Radio>
      <Radio value='false'>{t('learning.falseLabel')}</Radio>
    </Radio.Group>
  );
}

/** 填空题答案与干扰项编辑：1-3 个可接受答案 + 可选近义干扰项（逗号分隔） */
export function FillBlankEditor({
  answers,
  distractors,
  onAnswersChange,
  onDistractorsChange,
  t,
}: {
  answers: string[];
  distractors: string[];
  onAnswersChange: (next: string[]) => void;
  onDistractorsChange: (next: string[]) => void;
  t: Translate;
}) {
  return (
    <div className='flex flex-col gap-12px'>
      <div className='flex flex-col gap-6px'>
        {answers.map((item, index) => (
          <div key={index} className='flex items-center gap-6px'>
            <Input
              value={item}
              onChange={(value) =>
                onAnswersChange(
                  answers.map((existing, itemIndex) => (itemIndex === index ? value : existing))
                )
              }
            />
            <Button
              size='mini'
              type='text'
              status='danger'
              disabled={answers.length <= 1}
              onClick={() =>
                onAnswersChange(answers.filter((_, itemIndex) => itemIndex !== index))
              }
            >
              {t('learning.questionOptionRemove')}
            </Button>
          </div>
        ))}
        <div>
          <Button
            size='small'
            disabled={answers.length >= 3}
            onClick={() => onAnswersChange([...answers, ''])}
          >
            {t('learning.questionAnswerAdd')}
          </Button>
        </div>
      </div>
      <div>
        <div className='mb-6px font-500'>{t('learning.questionDistractors')}</div>
        <Input
          value={distractors.join(', ')}
          placeholder={t('learning.questionDistractorsPlaceholder')}
          onChange={(value) =>
            onDistractorsChange(
              value
                .split(/[,，]/)
                .map((item) => item.trim())
                .filter((item) => item !== '')
            )
          }
        />
        <Text type='secondary' className='text-12px'>
          {t('learning.questionDistractorsHint')}
        </Text>
      </div>
    </div>
  );
}

function QuestionEditDialog({
  entry,
  onClose,
  onSaved,
}: {
  entry: QuestionEntry;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [prompt, setPrompt] = useState(entry.prompt ?? '');
  const [options, setOptions] = useState<string[]>(entry.options);
  const [answer, setAnswer] = useState<unknown>(entry.answer ?? undefined);
  const [answers, setAnswers] = useState<string[]>(
    Array.isArray(entry.answer) ? (entry.answer as string[]) : ['']
  );
  const [distractors, setDistractors] = useState<string[]>(entry.distractors);
  const [explanation, setExplanation] = useState(entry.explanation ?? '');
  const [busy, setBusy] = useState(false);
  const isSingleChoice = entry.question_kind === 'single_choice';
  const isFillInBlank = entry.question_kind === 'fill_in_blank';
  const save = async () => {
    const errorKey = validateQuestionForm(prompt, options, answer, isSingleChoice, isFillInBlank);
    if (errorKey !== null) {
      Message.error(t(errorKey));
      return;
    }
    const cleanedOptions = options.map((option) => option.trim()).filter((option) => option !== '');
    const cleanedAnswers = answers.map((item) => item.trim()).filter((item) => item !== '');
    setBusy(true);
    try {
      await learningApi.updateQuestion(entry, {
        prompt: prompt.trim(),
        options: isSingleChoice ? cleanedOptions : [],
        answer: isFillInBlank ? cleanedAnswers : answer,
        explanation: explanation.trim(),
        distractors: isFillInBlank ? distractors : undefined,
      });
      Message.success(t('learning.questionSaved'));
      onSaved();
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      title={t('learning.questionEditTitle')}
      visible
      style={{ width: 560 }}
      confirmLoading={busy}
      onCancel={() => {
        if (!busy) onClose();
      }}
      onOk={() => void save()}
    >
      <div className='flex flex-col gap-14px'>
        {entry.edit_pending && entry.edit_note ? (
          <div className='flex items-start gap-6px rounded-6px bg-[var(--color-fill-2)] px-10px py-8px'>
            <IconPushpin className='mt-2px shrink-0 text-t-tertiary' />
            <span className='min-w-0 flex-1 text-12px leading-relaxed text-t-secondary'>
              <span className='font-500 text-t-primary'>{t('learning.questionEditNoteLabel')}</span>
              ：{entry.edit_note}
            </span>
          </div>
        ) : null}
        <div>
          <div className='mb-6px font-500'>{t('learning.questionPromptLabel')}</div>
          <Input.TextArea
            value={prompt}
            onChange={setPrompt}
            autoSize={{ minRows: 2 }}
          />
        </div>
        {isSingleChoice ? (
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
        ) : isFillInBlank ? (
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
        ) : (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionAnswer')}</div>
            <TrueFalseAnswer answer={answer} onChange={setAnswer} t={t} />
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
    </Modal>
  );
}

function QuestionCreateDialog({
  onClose,
  onSaved,
}: {
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [kind, setKind] = useState<'true_false' | 'single_choice' | 'fill_in_blank'>('true_false');
  const [prompt, setPrompt] = useState('');
  const [options, setOptions] = useState<string[]>(['', '']);
  const [answer, setAnswer] = useState<unknown>(undefined);
  const [answers, setAnswers] = useState<string[]>(['']);
  const [distractors, setDistractors] = useState<string[]>([]);
  const [explanation, setExplanation] = useState('');
  const [conceptId, setConceptId] = useState<string | undefined>(undefined);
  const [conceptRefs, setConceptRefs] = useState<ConceptRef[]>([]);
  const [conceptsLoading, setConceptsLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const isSingleChoice = kind === 'single_choice';
  const isFillInBlank = kind === 'fill_in_blank';

  useEffect(() => {
    let cancelled = false;
    setConceptsLoading(true);
    learningApi
      .listConceptRefs()
      .then((refs) => {
        if (!cancelled) setConceptRefs(refs);
      })
      .catch(() => {
        // Concept binding is optional; keep the dialog usable if listing fails.
      })
      .finally(() => {
        if (!cancelled) setConceptsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const save = async () => {
    const errorKey = validateQuestionForm(prompt, options, answer, isSingleChoice, isFillInBlank);
    if (errorKey !== null) {
      Message.error(t(errorKey));
      return;
    }
    const cleanedOptions = options.map((option) => option.trim()).filter((option) => option !== '');
    const cleanedAnswers = answers.map((item) => item.trim()).filter((item) => item !== '');
    setBusy(true);
    try {
      await learningApi.createCustomQuestion({
        kind,
        prompt: prompt.trim(),
        options: isSingleChoice ? cleanedOptions : [],
        answer: isFillInBlank ? cleanedAnswers : answer,
        explanation: explanation.trim(),
        concept_id: conceptId ?? null,
        distractors: isFillInBlank ? distractors : undefined,
      });
      Message.success(t('learning.questionCreated'));
      onSaved();
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    } finally {
      setBusy(false);
    }
  };

  const switchKind = (next: 'true_false' | 'single_choice' | 'fill_in_blank') => {
    if (next === kind) return;
    setKind(next);
    setAnswer(undefined);
    setAnswers(['']);
    setDistractors([]);
  };

  return (
    <Modal
      title={t('learning.questionCreateTitle')}
      visible
      style={{ width: 560 }}
      confirmLoading={busy}
      onCancel={() => {
        if (!busy) onClose();
      }}
      onOk={() => void save()}
    >
      <div className='flex flex-col gap-14px'>
        <div>
          <div className='mb-6px font-500'>{t('learning.questionKind')}</div>
          <Radio.Group
            value={kind}
            onChange={(value) =>
              switchKind(value as 'true_false' | 'single_choice' | 'fill_in_blank')
            }
          >
            <Radio value='true_false'>{t('learning.kindTrueFalse')}</Radio>
            <Radio value='single_choice'>{t('learning.kindSingleChoice')}</Radio>
            <Radio value='fill_in_blank'>{t('learning.kindFillInBlank')}</Radio>
          </Radio.Group>
        </div>
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
        {isSingleChoice ? (
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
        ) : isFillInBlank ? (
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
        ) : (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionAnswer')}</div>
            <TrueFalseAnswer answer={answer} onChange={setAnswer} t={t} />
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
        <div>
          <div className='mb-6px font-500'>{t('learning.questionConceptBind')}</div>
          <Select
            className='w-full'
            allowClear
            loading={conceptsLoading}
            value={conceptId}
            placeholder={t('learning.questionConceptBindPlaceholder')}
            onChange={(value: string | undefined) => setConceptId(value)}
          >
            {conceptRefs.map((concept) => (
              <Select.Option key={concept.concept_id} value={concept.concept_id}>
                {concept.title}
                {concept.course_title !== null ? ` · ${concept.course_title}` : ''}
              </Select.Option>
            ))}
          </Select>
          <Text type='secondary' className='text-12px'>
            {t('learning.questionConceptBindHint')}
          </Text>
        </div>
      </div>
    </Modal>
  );
}

function QuestionDetailDrawer({
  entry,
  onClose,
  onEdit,
  onDelete,
}: {
  entry: QuestionEntry;
  onClose: () => void;
  onEdit: (entry: QuestionEntry) => void;
  onDelete: (entry: QuestionEntry) => void;
}) {
  const { t } = useTranslation();
  const state = questionStateMeta(entry, t);
  const isSingleChoice = entry.question_kind === 'single_choice';
  const isFillInBlank = entry.question_kind === 'fill_in_blank';
  const deletable = entry.source === 'custom' || entry.review_item_id !== null;
  const inQueue = entry.source === 'course' && entry.state !== 'unlearned';
  const metrics = [
    {
      label: t('learning.questionLastReviewed'),
      value: formatReviewTime(entry.last_reviewed_at),
    },
    { label: t('learning.questionReviewCount'), value: String(entry.review_count) },
    { label: t('learning.questionLapseCount'), value: String(entry.lapse_count) },
    { label: t('learning.questionStability'), value: entry.stability_days.toFixed(1) },
    { label: t('learning.questionDifficulty'), value: entry.difficulty.toFixed(1) },
  ];
  return (
    <Drawer
      title={t('learning.questionDetailTitle')}
      visible
      width={480}
      onCancel={onClose}
      footer={
        <div className='flex justify-end gap-8px'>
          <Button status='danger' disabled={!deletable} onClick={() => onDelete(entry)}>
            {t('learning.questionDelete')}
          </Button>
          <Button type='primary' onClick={() => onEdit(entry)}>
            {t('learning.questionEdit')}
          </Button>
        </div>
      }
    >
      <div className='flex flex-col gap-16px'>
        <div>
          <div className='mb-8px flex flex-wrap items-center gap-6px'>
            <Tag color={state.color}>{state.label}</Tag>
            {entry.question_kind !== null && (
              <Tag>
                {entry.question_kind === 'single_choice'
                  ? t('learning.kindSingleChoice')
                  : entry.question_kind === 'fill_in_blank'
                    ? t('learning.kindFillInBlank')
                    : t('learning.kindTrueFalse')}
              </Tag>
            )}
          </div>
          <div className='text-16px font-600 leading-relaxed'>{entry.prompt ?? '—'}</div>
        </div>
        <div className='text-12px text-t-tertiary'>
          {entry.source === 'custom'
            ? t('learning.questionCustomSource')
            : [entry.course_title ?? t('learning.deletedCourse'), entry.concept_title]
                .filter((part) => part !== null && part !== undefined)
                .join(' › ')}
        </div>
        <div>
          <div className='mb-6px font-500'>{t('learning.questionQueueSection')}</div>
          <div className='flex flex-col gap-8px'>
            <div className='flex flex-wrap items-center gap-8px'>
              {entry.source === 'custom' ? (
                <Tag color='purple'>{t('learning.questionQueueCustom')}</Tag>
              ) : inQueue ? (
                <Tag color='green'>{t('learning.questionQueueInQueue')}</Tag>
              ) : (
                <Tag color='gray'>{t('learning.questionQueueNotInQueue')}</Tag>
              )}
              {inQueue && entry.due_at !== null && (
                <Text type='secondary'>
                  {t('learning.questionDueAt')}: {formatReviewTime(entry.due_at)}
                </Text>
              )}
            </div>
            <div className='text-13px text-t-secondary'>
              {entry.source === 'custom'
                ? t('learning.questionQueueHintCustom')
                : inQueue
                  ? t('learning.questionQueueHintIn')
                  : t('learning.questionQueueHintOut')}
            </div>
          </div>
        </div>
        {isSingleChoice && entry.options.length > 0 && (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionOptions')}</div>
            <div className='flex flex-col gap-6px'>
              {entry.options.map((option, index) => {
                const isAnswer = entry.answer === option;
                return (
                  <div
                    key={index}
                    className={`flex items-center rounded-8px border px-12px py-8px text-13px ${
                      isAnswer
                        ? 'border-[var(--color-success-light-3)] bg-[var(--color-success-light-1)]'
                        : 'border-[var(--color-border-2)]'
                    }`}
                  >
                    <span>{option}</span>
                    {isAnswer && (
                      <Tag size='small' color='green' className='ml-8px'>
                        {t('learning.questionDetailCorrectAnswer')}
                      </Tag>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}
        {!isSingleChoice && !isFillInBlank && typeof entry.answer === 'boolean' && (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionAnswer')}</div>
            <Tag color='green'>
              {entry.answer ? t('learning.trueLabel') : t('learning.falseLabel')}
            </Tag>
          </div>
        )}
        {isFillInBlank && Array.isArray(entry.answer) && (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionAcceptedAnswers')}</div>
            <div className='flex flex-wrap gap-6px'>
              {(entry.answer as string[])
                .filter((item) => typeof item === 'string' && item.trim() !== '')
                .map((item, index) => (
                  <Tag key={index} color='green'>
                    {item}
                  </Tag>
                ))}
            </div>
          </div>
        )}
        {isFillInBlank && entry.distractors.length > 0 && (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionDistractors')}</div>
            <div className='flex flex-wrap gap-6px'>
              {entry.distractors.map((item, index) => (
                <Tag key={index}>{item}</Tag>
              ))}
            </div>
          </div>
        )}
        {entry.explanation !== null && entry.explanation.trim() !== '' && (
          <div>
            <div className='mb-6px font-500'>{t('learning.questionExplanation')}</div>
            <div className='text-13px text-t-secondary'>{entry.explanation}</div>
          </div>
        )}
        <div>
          <div className='mb-6px font-500'>{t('learning.questionDetailMetrics')}</div>
          <div className='grid grid-cols-2 gap-8px'>
            {metrics.map((metric) => (
              <div
                key={metric.label}
                className='rounded-8px bg-[var(--color-fill-2)] px-12px py-8px'
              >
                <div className='text-12px text-t-tertiary'>{metric.label}</div>
                <div className='mt-2px text-14px font-500'>{metric.value}</div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </Drawer>
  );
}

export { QuestionCreateDialog, QuestionDetailDrawer, QuestionEditDialog };
