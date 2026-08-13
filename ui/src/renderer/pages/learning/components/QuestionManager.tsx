import {
  Alert,
  Button,
  Checkbox,
  Dropdown,
  Empty,
  Input,
  Message,
  Modal,
  Select,
  Table,
  Tag,
  Tooltip,
  Typography,
} from '@arco-design/web-react';
import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import { QUESTION_COLUMNS_STORAGE_KEY, QUESTION_SELECTABLE_COLUMNS } from '../constants';
import type { CourseSummary, QuestionEntry } from '../types';
import {
  errorMessage,
  formatReviewTime,
  loadVisibleQuestionColumns,
  questionStateMeta,
} from '../utils';
import {
  QuestionCreateDialog,
  QuestionDetailDrawer,
  QuestionEditDialog,
} from './QuestionDialogs';

const { Text } = Typography;

/** 题目管理：筛选、列显隐、增删改查与详情抽屉 */
export function QuestionManager({
  onMutated,
  onEditTags,
}: {
  onMutated: () => void;
  onEditTags: (entry: QuestionEntry) => void;
}) {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<QuestionEntry[]>([]);
  const [courses, setCourses] = useState<CourseSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [courseFilter, setCourseFilter] = useState<string | undefined>(undefined);
  const [stateFilter, setStateFilter] = useState<string | undefined>(undefined);
  const [search, setSearch] = useState('');
  const [editing, setEditing] = useState<QuestionEntry | null>(null);
  const [creating, setCreating] = useState(false);
  const [visibleColumns, setVisibleColumns] = useState<string[]>(loadVisibleQuestionColumns);
  const [detailEntry, setDetailEntry] = useState<QuestionEntry | null>(null);
  const persistVisibleColumns = (next: string[]) => {
    setVisibleColumns(next);
    try {
      localStorage.setItem(QUESTION_COLUMNS_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // Storage is unavailable; keep the in-memory selection only.
    }
  };
  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [questionRows, courseRows] = await Promise.all([
        learningApi.listQuestions({
          course_id: courseFilter,
          state: stateFilter,
          search: search.trim() === '' ? undefined : search.trim(),
        }),
        learningApi.listCourses(),
      ]);
      setEntries(questionRows);
      setCourses(courseRows);
    } catch (actionError) {
      Message.error(errorMessage(t, actionError));
    } finally {
      setLoading(false);
    }
  }, [courseFilter, stateFilter, search, t]);
  useEffect(() => {
    void load();
  }, [load]);
  const confirmDelete = (entry: QuestionEntry) => {
    const isCustom = entry.source === 'custom';
    if (!isCustom && entry.review_item_id === null) {
      return;
    }
    Modal.confirm({
      title: isCustom
        ? t('learning.questionDeleteCustomTitle')
        : t('learning.questionDeleteTitle'),
      content: isCustom
        ? t('learning.questionDeleteCustomHint')
        : t('learning.questionDeleteHint'),
      okButtonProps: { status: 'danger' },
      onOk: async () => {
        try {
          if (isCustom) {
            await learningApi.deleteCustomQuestion(entry.question_id);
          } else if (entry.review_item_id !== null) {
            await learningApi.deleteReviewItem(entry.review_item_id);
          }
          Message.success(t('learning.questionDeleted'));
          await load();
          onMutated();
        } catch (actionError) {
          Message.error(errorMessage(t, actionError));
        }
      },
    });
  };
  const promptColumn = {
    title: t('learning.questionPrompt'),
    render: (_value: unknown, entry: QuestionEntry) => {
      const text = entry.prompt ?? '—';
      return (
        <Tooltip content={text} position='tl'>
          <span className='line-clamp-2 block'>{text}</span>
        </Tooltip>
      );
    },
  };
  const sourceColumn = {
    title: t('learning.questionSource'),
    dataIndex: 'source',
    width: 220,
    render: (_value: unknown, entry: QuestionEntry) =>
      entry.source === 'custom' ? (
        <Tag color='purple'>{t('learning.questionCustomSource')}</Tag>
      ) : (
        <div className='flex flex-col gap-2px'>
          <span className='truncate'>{entry.concept_title ?? '—'}</span>
          <span className='truncate text-12px text-t-tertiary'>
            {entry.course_title ?? t('learning.deletedCourse')}
          </span>
        </div>
      ),
  };
  const stateColumn = {
    title: t('learning.questionState'),
    dataIndex: 'state',
    width: 130,
    render: (_value: unknown, entry: QuestionEntry) => {
      const state = questionStateMeta(entry, t);
      const hint =
        entry.state === 'unlearned'
          ? t('learning.questionStateUnlearnedHint')
          : entry.state === 'new'
            ? t('learning.questionStateNewHint')
            : entry.state === 'due'
              ? t('learning.questionStateDueHint')
              : t('learning.questionStateScheduledHint');
      return (
        <Tooltip content={hint} position='tl'>
          <Tag color={state.color}>{state.label}</Tag>
        </Tooltip>
      );
    },
  };
  const dueColumn = {
    title: t('learning.questionDueAt'),
    dataIndex: 'due_at',
    width: 170,
    sorter: (a: QuestionEntry, b: QuestionEntry) => (a.due_at ?? 0) - (b.due_at ?? 0),
    render: (value: number | null) => formatReviewTime(value),
  };
  const tagsColumn = {
    title: t('learning.questionTags'),
    dataIndex: 'tags',
    width: 200,
    render: (_value: unknown, entry: QuestionEntry) => (
      <div
        className='flex flex-wrap items-center gap-4px'
        onClick={(event) => event.stopPropagation()}
      >
        {entry.tags.length === 0 ? (
          <Text type='secondary' className='text-12px'>
            {t('learning.tagsEmpty')}
          </Text>
        ) : (
          entry.tags.map((tag) => (
            <Tag key={tag} size='small'>
              {tag}
            </Tag>
          ))
        )}
        <Button size='mini' type='text' onClick={() => onEditTags(entry)}>
          {t('learning.questionTagsEdit')}
        </Button>
      </div>
    ),
  };
  const actionsColumn = {
    title: t('learning.questionActions'),
    width: 130,
    render: (_value: unknown, entry: QuestionEntry) => (
      <div className='flex gap-6px'>
        <Button
          size='mini'
          type='text'
          onClick={(event) => {
            event.stopPropagation();
            setEditing(entry);
          }}
        >
          {t('learning.questionEdit')}
        </Button>
        <Button
          size='mini'
          type='text'
          status='danger'
          disabled={entry.source === 'course' && entry.review_item_id === null}
          onClick={(event) => {
            event.stopPropagation();
            confirmDelete(entry);
          }}
        >
          {t('learning.questionDelete')}
        </Button>
      </div>
    ),
  };
  const columns = [
    promptColumn,
    ...(visibleColumns.includes('source') ? [sourceColumn] : []),
    ...(visibleColumns.includes('state') ? [stateColumn] : []),
    ...(visibleColumns.includes('due_at') ? [dueColumn] : []),
    ...(visibleColumns.includes('tags') ? [tagsColumn] : []),
    actionsColumn,
  ];
  return (
    <div className='flex flex-col gap-12px'>
      <div className='flex flex-wrap items-center gap-8px'>
        <Select
          className='w-240px'
          allowClear
          placeholder={t('learning.questionFilterCourse')}
          value={courseFilter}
          onChange={(value: string | undefined) => setCourseFilter(value)}
        >
          {courses.map((course) => (
            <Select.Option key={course.id} value={course.id}>
              {course.title}
            </Select.Option>
          ))}
        </Select>
        <Select
          className='w-160px'
          allowClear
          placeholder={t('learning.questionFilterState')}
          value={stateFilter}
          onChange={(value: string | undefined) => setStateFilter(value)}
        >
          <Select.Option value='unlearned'>{t('learning.questionStateUnlearned')}</Select.Option>
          <Select.Option value='new'>{t('learning.questionStateNew')}</Select.Option>
          <Select.Option value='due'>{t('learning.questionStateDue')}</Select.Option>
          <Select.Option value='scheduled'>{t('learning.questionStateScheduled')}</Select.Option>
        </Select>
        <Input.Search
          className='w-240px'
          allowClear
          placeholder={t('learning.questionSearchPlaceholder')}
          onSearch={(value) => setSearch(value)}
        />
        <div className='ml-auto flex items-center gap-8px'>
          <Dropdown
            position='br'
            trigger='click'
            droplist={
              <div className='rounded-8px border border-[var(--color-border-2)] bg-[var(--color-bg-popup)] p-10px'>
                <Checkbox.Group
                  value={visibleColumns}
                  onChange={(value) => persistVisibleColumns(value as string[])}
                >
                  <div className='flex flex-col gap-6px'>
                    {QUESTION_SELECTABLE_COLUMNS.map((key) => (
                      <Checkbox key={key} value={key}>
                        {key === 'source'
                          ? t('learning.questionSource')
                          : key === 'state'
                            ? t('learning.questionState')
                            : key === 'due_at'
                              ? t('learning.questionDueAt')
                              : t('learning.questionTags')}
                      </Checkbox>
                    ))}
                  </div>
                </Checkbox.Group>
              </div>
            }
          >
            <Button>{t('learning.questionColumnsConfig')}</Button>
          </Dropdown>
          <Button type='primary' onClick={() => setCreating(true)}>
            {t('learning.questionCreate')}
          </Button>
        </div>
      </div>
      <Alert type='info' content={t('learning.questionQueueLegend')} />
      <Table
        rowKey={(entry: QuestionEntry) =>
          `${entry.source}:${entry.question_id}:${entry.concept_id ?? '-'}`
        }
        loading={loading}
        data={entries}
        columns={columns}
        pagination={{ pageSize: 20, showTotal: true }}
        onRow={(record: QuestionEntry) => ({
          onClick: () => setDetailEntry(record),
        })}
        noDataElement={<Empty description={t('learning.questionEmpty')} />}
      />
      {detailEntry !== null && (
        <QuestionDetailDrawer
          entry={detailEntry}
          onClose={() => setDetailEntry(null)}
          onEdit={(entry) => {
            setDetailEntry(null);
            setEditing(entry);
          }}
          onDelete={(entry) => {
            setDetailEntry(null);
            confirmDelete(entry);
          }}
        />
      )}
      {editing !== null && (
        <QuestionEditDialog
          entry={editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            void load();
            onMutated();
          }}
        />
      )}
      {creating && (
        <QuestionCreateDialog
          onClose={() => setCreating(false)}
          onSaved={() => {
            setCreating(false);
            void load();
            onMutated();
          }}
        />
      )}
    </div>
  );
}
