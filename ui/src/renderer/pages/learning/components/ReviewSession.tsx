import { Button, Modal, Progress, Tag, Typography } from '@arco-design/web-react';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { DueReview, ReviewAnswerResult, ReviewRating } from '../types';
import { ActivityInput } from './ActivityInput';

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
}) {
  const { t } = useTranslation();
  const [response, setResponse] = useState<unknown>();
  const [result, setResult] = useState<ReviewAnswerResult | null>(null);
  const [wasForgot, setWasForgot] = useState(false);
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
  onClose,
}: {
  open: boolean;
  queue: DueReview[];
  busyId: string | null;
  onAnswer: (review: DueReview, response: unknown) => Promise<ReviewAnswerResult | undefined>;
  onForget: (review: DueReview) => Promise<ReviewAnswerResult | undefined>;
  onRate: (review: DueReview, rating: ReviewRating) => Promise<boolean>;
  onSkip: (review: DueReview) => Promise<boolean>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [index, setIndex] = useState(0);
  useEffect(() => {
    if (open) setIndex(0);
  }, [open]);
  const current = queue[index];
  // Each queue card is its own review item with its own schedule, so rating,
  // answering wrong or skipping all move straight to the next card.
  const advance = () => setIndex((value) => value + 1);
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
          />
        </div>
      )}
    </Modal>
  );
}
