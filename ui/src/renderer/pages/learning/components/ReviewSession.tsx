import {
  Button,
  Dropdown,
  Input,
  Menu,
  Modal,
  Progress,
  Tag,
  Tooltip,
  Typography,
} from '@arco-design/web-react';
import { IconDelete, IconEdit, IconLock, IconMore, IconPushpin, IconQuestionCircle } from '@arco-design/web-react/icon';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import type { DueReview, QuestionEntry, ReviewAnswerResult, ReviewRating } from '../types';
import { errorMessage, isReviewUnderdeveloped } from '../utils';
import { ActivityInput } from './ActivityInput';
import { QuestionEditDialog } from './QuestionDialogs';

const { Text } = Typography;

export function ReviewCard({
  review,
  busy,
  locked,
  onAnswer,
  onForget,
  onRate,
  onSkip,
  onNext,
  onEdit,
  onMarkEdit,
  onArchive,
  onDelete,
}: {
  review: DueReview;
  busy: boolean;
  locked: boolean;
  onAnswer: (review: DueReview, response: unknown) => Promise<ReviewAnswerResult | undefined>;
  onForget: (review: DueReview) => Promise<ReviewAnswerResult | undefined>;
  onRate: (review: DueReview, rating: ReviewRating) => void;
  onSkip: (review: DueReview) => void;
  /** Leave the finished card and move to the next review item. */
  onNext: () => void;
  /** 二级操作：编辑 / 标记待编辑 / 归档 / 删除，不常用所以收进下拉菜单 */
  onEdit: (review: DueReview) => void;
  onMarkEdit: (review: DueReview) => void;
  onArchive: (review: DueReview) => void;
  onDelete: (review: DueReview) => void;
}) {
  const { t } = useTranslation();
  const [response, setResponse] = useState<unknown>();
  const [result, setResult] = useState<ReviewAnswerResult | null>(null);
  const [wasForgot, setWasForgot] = useState(false);
  // 卡片内容被编辑后，旧作答与结果不再有效，重置本轮作答状态
  useEffect(() => {
    setResponse(undefined);
    setResult(null);
    setWasForgot(false);
  }, [review.question.prompt, review.question.options, review.question.kind]);
  const question = review.question;
  const hasResponse =
    typeof response === 'string' ? response.trim().length > 0 : response !== undefined;
  const answerText = (value: unknown): string => {
    if (typeof value === 'boolean') {
      return value ? t('learning.trueLabel') : t('learning.falseLabel');
    }
    if (Array.isArray(value)) {
      return value
        .map((item) => (typeof item === 'string' ? item : ''))
        .filter((item) => item !== '')
        .join(' / ');
    }
    return typeof value === 'string' ? value : '';
  };
  const actions = (
    <Dropdown
      position='br'
      trigger='click'
      droplist={
        <div className='w-220px rounded-8px border border-solid border-[var(--color-border-1)] bg-[var(--color-bg-popup)] p-4px'>
          <Menu
            style={{ borderRadius: 8 }}
            onClickMenuItem={(key) => {
              if (key === 'edit') onEdit(review);
              if (key === 'mark-edit') onMarkEdit(review);
              if (key === 'archive') onArchive(review);
              if (key === 'delete') onDelete(review);
            }}
          >
            <Menu.Item key='edit'>
              <span className='flex items-center gap-6px'>
                <IconEdit /> {t('learning.reviewEdit')}
              </span>
            </Menu.Item>
            <Menu.Item key='mark-edit'>
              <span className='flex items-center gap-6px'>
                <IconPushpin /> {t('learning.reviewMarkEdit')}
              </span>
            </Menu.Item>
            <Menu.Item key='archive'>
              <span className='flex items-center gap-6px'>
                <IconLock /> {t('learning.reviewArchive')}
              </span>
            </Menu.Item>
            <Menu.Item key='delete' style={{ color: 'var(--color-danger-6)' }}>
              <span className='flex items-center gap-6px'>
                <IconDelete /> {t('learning.questionDelete')}
              </span>
            </Menu.Item>
          </Menu>
          <Tooltip content={t('learning.reviewManageHintFull')} position='left'>
            <div className='mx-4px my-4px flex cursor-help items-start gap-4px text-12px leading-relaxed text-t-tertiary'>
              <IconQuestionCircle className='mt-3px shrink-0' />
              <span className='min-w-0 flex-1'>{t('learning.reviewManageHint')}</span>
            </div>
          </Tooltip>
        </div>
      }
    >
      <Button
        size='mini'
        type='text'
        aria-label={t('learning.reviewActions')}
        disabled={busy || locked}
      >
        <IconMore />
      </Button>
    </Dropdown>
  );
  const editPendingBadge = review.edit_pending ? (
    <Tag size='small' color='orange'>
      <IconPushpin className='mr-3px' />
      {t('learning.reviewEditPendingLabel')}
    </Tag>
  ) : null;
  return (
    <div className='rounded-10px border border-solid border-[var(--color-border-2)] p-14px'>
      <div className='mb-12px flex flex-wrap items-center gap-x-6px gap-y-6px text-12px'>
        {review.source === 'custom' ? (
          <Tag size='small' color='purple'>
            {t('learning.reviewCustomSource')}
          </Tag>
        ) : (
          <>
            <span className='rounded-6px bg-[var(--color-fill-2)] px-8px py-2px font-500 text-t-secondary'>
              {review.course_title ?? t('learning.deletedCourse')}
            </span>
            <span className='text-t-tertiary'>›</span>
            <span className='rounded-6px bg-[var(--color-fill-2)] px-8px py-2px font-500 text-t-secondary'>
              {review.module_title ?? '—'}
            </span>
            <span className='text-t-tertiary'>›</span>
            <span className='rounded-6px bg-[var(--color-fill-2)] px-8px py-2px font-500 text-t-secondary'>
              {review.lesson_title ?? '—'}
            </span>
          </>
        )}
        {review.concept_title !== null && (
          <Tag size='small' color='arcoblue'>
            {t('learning.reviewConceptLabel')}: {review.concept_title}
          </Tag>
        )}
        {review.edit_pending &&
          (review.edit_note ? (
            <Tooltip content={review.edit_note} position='top'>
              {editPendingBadge}
            </Tooltip>
          ) : (
            editPendingBadge
          ))}
        <div className='ml-auto'>{actions}</div>
      </div>
      <div className='mb-12px text-16px font-500 leading-relaxed text-t-primary'>
        {question.prompt}
      </div>
      <ActivityInput
        kind={question.kind}
        options={question.options}
        value={response}
        disabled={result !== null || locked}
        onChange={setResponse}
      />
      {result === null && (
        <div className='mt-12px flex items-center gap-8px'>
          <Button
            type='primary'
            size='small'
            disabled={!hasResponse || locked}
            loading={busy}
            onClick={() =>
              void onAnswer(review, response).then((answerResult) => {
                if (answerResult) {
                  setResult(answerResult);
                }
              })
            }
          >
            {t('learning.reviewSubmitAnswer')}
          </Button>
          <Button
            size='small'
            disabled={locked}
            loading={busy}
            onClick={() =>
              void onForget(review).then((answerResult) => {
                if (answerResult) {
                  setWasForgot(true);
                  setResult(answerResult);
                }
              })
            }
          >
            {t('learning.reviewForgot')}
          </Button>
          <Button
            size='small'
            type='text'
            disabled={locked}
            loading={busy}
            onClick={() => onSkip(review)}
          >
            {t('learning.reviewSkip')}
          </Button>
        </div>
      )}
      {result !== null && result.correct && (
        <div className='mt-12px flex flex-col gap-10px rounded-10px border border-solid border-[var(--color-success-light-3)] bg-[var(--color-success-light-1)] p-14px'>
          <Text type='success' className='font-500'>
            {t('learning.correct')}
            {result.feedback ? ` · ${result.feedback}` : ''}
          </Text>
          <Text type='secondary' className='text-13px'>
            {t('learning.reviewRatePrompt')}
          </Text>
          <div className='grid grid-cols-3 gap-8px'>
            <Button
              status='warning'
              type='outline'
              loading={busy}
              disabled={locked && !busy}
              onClick={() => onRate(review, 'hard')}
            >
              {t('learning.reviewHard')}
            </Button>
            <Button
              type='primary'
              loading={busy}
              disabled={locked && !busy}
              onClick={() => onRate(review, 'good')}
            >
              {t('learning.reviewGood')}
            </Button>
            <Button
              status='success'
              type='outline'
              loading={busy}
              disabled={locked && !busy}
              onClick={() => onRate(review, 'easy')}
            >
              {t('learning.reviewEasy')}
            </Button>
          </div>
        </div>
      )}
      {result !== null && !result.correct && (
        <div className='mt-12px flex flex-col gap-8px rounded-10px border border-solid border-[var(--color-danger-light-3)] bg-[var(--color-danger-light-1)] p-14px'>
          <Text type='error' className='font-500'>
            {wasForgot ? t('learning.reviewForgotMarked') : t('learning.reviewWrongMarkedAgain')}
          </Text>
          {result.correct_answer !== null && (
            <Text type='secondary'>
              {t('learning.reviewCorrectAnswer')}: {answerText(result.correct_answer)}
            </Text>
          )}
          {result.feedback && <Text type='secondary'>{result.feedback}</Text>}
          <Button
            type='primary'
            status='danger'
            size='small'
            className='self-start'
            disabled={locked}
            onClick={onNext}
          >
            {t('learning.reviewNext')}
          </Button>
        </div>
      )}
    </div>
  );
}

export function ReviewSessionModal({
  open,
  queue,
  busyId,
  onAnswer,
  onForget,
  onRate,
  onSkip,
  onArchive,
  onRemove,
  onMarkEdit,
  onEdited,
  onClose,
}: {
  open: boolean;
  queue: DueReview[];
  busyId: string | null;
  onAnswer: (review: DueReview, response: unknown) => Promise<ReviewAnswerResult | undefined>;
  onForget: (review: DueReview) => Promise<ReviewAnswerResult | undefined>;
  onRate: (review: DueReview, rating: ReviewRating) => Promise<boolean>;
  onSkip: (review: DueReview) => Promise<boolean>;
  /** 归档动作：成功后由调用方推进队列 */
  onArchive: (review: DueReview) => Promise<boolean>;
  /** 删除动作：成功后由调用方推进队列 */
  onRemove: (review: DueReview) => Promise<boolean>;
  /** 标记待编辑：记录选填描述，不推进队列 */
  onMarkEdit: (review: DueReview, note: string) => Promise<void>;
  /** 编辑保存后原地更新队列中的卡片，不推进 */
  onEdited: (updated: DueReview) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [index, setIndex] = useState(0);
  const [editing, setEditing] = useState<QuestionEntry | null>(null);
  const [editingReview, setEditingReview] = useState<DueReview | null>(null);
  useEffect(() => {
    if (open) setIndex(0);
  }, [open]);
  const current = queue[index];
  // Each queue card is its own review item with its own schedule, so rating,
  // answering wrong or skipping all move straight to the next card.
  const advance = () => setIndex((value) => value + 1);
  // 编辑：先拉取含答案的完整条目，再打开编辑对话框
  const openEdit = async (review: DueReview) => {
    try {
      const entry =
        review.source === 'custom'
          ? await learningApi.getCustomQuestion(review.id)
          : await learningApi.getReviewQuestion(review.id);
      setEditingReview(review);
      setEditing(entry);
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    }
  };
  // 编辑保存后原地更新当前卡片；排期与作答状态保持不变
  const applyEdit = (entry: QuestionEntry) => {
    const source = editingReview;
    setEditing(null);
    setEditingReview(null);
    const target = queue.find((item) => item.id === source?.id);
    if (target === undefined) return;
    onEdited({
      ...target,
      question: {
        activity_id: target.question.activity_id,
        kind: entry.question_kind ?? target.question.kind,
        prompt: entry.prompt ?? target.question.prompt,
        options: entry.options,
      },
      // 编辑已保存，待编辑标记随之清除（后端同步清除）
      edit_pending: false,
      edit_note: null,
    });
  };
  // 归档：学习算法数据未达标时追问；达标直接归档
  const confirmArchive = (review: DueReview) => {
    const run = () =>
      void onArchive(review).then((handled) => {
        if (handled) advance();
      });
    if (isReviewUnderdeveloped(review)) {
      Modal.confirm({
        title: t('learning.reviewArchiveConfirmTitle'),
        content: t('learning.reviewArchiveConfirmHint', {
          count: review.review_count,
          days: review.stability_days.toFixed(1),
        }),
        okText: t('learning.reviewArchiveConfirmOk'),
        okButtonProps: { status: 'warning' },
        onOk: () => run(),
      });
    } else {
      run();
    }
  };
  // 删除不可逆，二次确认防止误触；确认文案区分课程题与自建题
  const confirmDelete = (review: DueReview) => {
    const run = () =>
      void onRemove(review).then((handled) => {
        if (handled) advance();
      });
    const isCustom = review.source === 'custom';
    Modal.confirm({
      title: isCustom
        ? t('learning.questionDeleteCustomTitle')
        : t('learning.questionDeleteTitle'),
      content: isCustom
        ? t('learning.questionDeleteCustomHint')
        : t('learning.questionDeleteHint'),
      okText: t('learning.questionDelete'),
      okButtonProps: { status: 'danger' },
      onOk: () => run(),
    });
  };
  // 标记待编辑：选填一句描述，将来编辑时找回思路；不打断复习心流
  const openMarkEdit = (review: DueReview) => {
    let note = '';
    Modal.confirm({
      title: t('learning.reviewMarkEditTitle'),
      content: (
        <div className='flex flex-col gap-8px pt-8px'>
          <div className='line-clamp-2 text-12px leading-relaxed text-t-tertiary'>
            {review.question.prompt}
          </div>
          <Input.TextArea
            placeholder={t('learning.reviewMarkEditPlaceholder')}
            autoSize={{ minRows: 2, maxRows: 5 }}
            maxLength={200}
            onChange={(value) => {
              note = value;
            }}
          />
        </div>
      ),
      okText: t('learning.reviewMarkEditOk'),
      onOk: () => onMarkEdit(review, note),
    });
  };
  return (
    <Modal
      title={t('learning.reviewSessionTitle')}
      visible={open}
      footer={null}
      style={{ width: 780 }}
      maskClosable={false}
      onCancel={() => {
        if (busyId === null) onClose();
      }}
    >
      {current === undefined ? (
        <div className='flex flex-col items-center gap-14px py-32px'>
          <Text type='secondary'>{t('learning.reviewSessionDone')}</Text>
          <Button type='primary' onClick={onClose}>
            {t('learning.reviewSessionClose')}
          </Button>
        </div>
      ) : (
        <div className='flex flex-col gap-12px'>
          <div className='flex items-center gap-12px'>
            <span className='shrink-0 rounded-full bg-[var(--color-primary-light-1)] px-12px py-2px text-13px font-600 text-[var(--color-primary-6)]'>
              {Math.min(index + 1, queue.length)} / {queue.length}
            </span>
            <div className='flex-1'>
              <Progress
                percent={Math.round((Math.min(index, queue.length) / queue.length) * 100)}
                showText={false}
                size='small'
                className='!my-0'
              />
            </div>
          </div>
          <ReviewCard
            key={current.id}
            review={current}
            busy={busyId === current.id}
            locked={busyId !== null && busyId !== current.id}
            onAnswer={onAnswer}
            onForget={onForget}
            onRate={(review, rating) =>
              void onRate(review, rating).then((handled) => {
                if (handled) advance();
              })
            }
            onSkip={(review) =>
              void onSkip(review).then((handled) => {
                if (handled) advance();
              })
            }
            onNext={advance}
            onEdit={(review) => void openEdit(review)}
            onMarkEdit={openMarkEdit}
            onArchive={confirmArchive}
            onDelete={confirmDelete}
          />
        </div>
      )}
      {editing !== null && (
        <QuestionEditDialog
          entry={editing}
          onClose={() => {
            setEditing(null);
            setEditingReview(null);
          }}
          onSaved={() => applyEdit(editing)}
        />
      )}
    </Modal>
  );
}
