import type { ConversationCommandQueueItem } from '@/renderer/pages/conversation/platforms/useConversationCommandQueue';
import {
  type Modifier,
  closestCenter,
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import { SortableContext, useSortable, verticalListSortingStrategy } from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { Dropdown, Menu } from '@arco-design/web-react';
import { CornerDownLeft, Delete, Down, Drag, Edit } from '@icon-park/react';
import React, { useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './commandQueuePanel.module.css';

const getCommandPreview = (input: string): string => input.replace(/\s+/g, ' ').trim();

const restrictQueueDragToVerticalAxis: Modifier = ({ transform }) => ({
  ...transform,
  x: 0,
});

const createRestrictToQueueContainerModifier = (
  queueContainerRef: React.RefObject<HTMLDivElement | null>
): Modifier => {
  return ({ draggingNodeRect, overlayNodeRect, transform }) => {
    const queueContainerRect = queueContainerRef.current?.getBoundingClientRect();
    const activeRect = overlayNodeRect ?? draggingNodeRect;

    if (!queueContainerRect || !activeRect) {
      return transform;
    }

    const minY = queueContainerRect.top - activeRect.top;
    const maxY = queueContainerRect.bottom - (activeRect.top + activeRect.height);

    return {
      ...transform,
      y: Math.min(Math.max(transform.y, minY), maxY),
    };
  };
};

type CommandQueuePanelProps = {
  items: ConversationCommandQueueItem[];
  paused: boolean;
  interactionLocked: boolean;
  onPause: () => void;
  onResume: () => void;
  onInteractionLock: () => void;
  onInteractionUnlock: () => void;
  onEdit?: (item: ConversationCommandQueueItem) => void;
  onSendNow: (commandId: string) => void;
  onReorder: (activeCommandId: string, overCommandId: string) => void;
  onRemove: (commandId: string) => void;
  onClear: () => void;
};

type SortableQueueItemProps = {
  item: ConversationCommandQueueItem;
  dragDisabled: boolean;
  dragHandleLabel: string;
  preview: string;
  fileCountLabel: string | null;
  t: (key: string, options?: Record<string, unknown>) => string;
  onEdit?: (item: ConversationCommandQueueItem) => void;
  onSendNow: (commandId: string) => void;
  onRemove: (commandId: string) => void;
  onDragHandlePointerDown: (event: React.PointerEvent<HTMLButtonElement>) => void;
};

type QueueItemCardProps = {
  item: ConversationCommandQueueItem;
  isDragging: boolean;
  dragDisabled: boolean;
  dragHandleLabel: string;
  preview: string;
  fileCountLabel: string | null;
  t: (key: string, options?: Record<string, unknown>) => string;
  onEdit?: (item: ConversationCommandQueueItem) => void;
  onSendNow: (commandId: string) => void;
  onRemove: (commandId: string) => void;
  onDragHandlePointerDown: (event: React.PointerEvent<HTMLButtonElement>) => void;
  dragHandleButtonProps: React.ButtonHTMLAttributes<HTMLButtonElement>;
  dragHandleRef: (element: HTMLButtonElement | null) => void;
};

const QueueItemCard: React.FC<QueueItemCardProps> = ({
  item,
  isDragging,
  dragDisabled,
  dragHandleLabel,
  preview,
  fileCountLabel,
  t,
  onEdit,
  onSendNow,
  onRemove,
  onDragHandlePointerDown,
  dragHandleButtonProps,
  dragHandleRef,
}) => {
  const { onPointerDown: onSortableDragHandlePointerDown, ...restDragHandleButtonProps } = dragHandleButtonProps ?? {};
  return (
    <div
      className={isDragging ? `${styles.item} ${styles.itemDragging}` : styles.item}
      data-command-id={item.id}
      data-sortable={dragDisabled ? 'disabled' : 'enabled'}
      aria-grabbed={isDragging}
      aria-label={preview}
    >
      <div className={styles.body}>
        <div className={styles.dragSlot}>
          <button
            {...restDragHandleButtonProps}
            ref={dragHandleRef}
            type='button'
            aria-label={dragHandleLabel}
            disabled={dragDisabled}
            data-drag-handle={dragDisabled ? 'disabled' : 'enabled'}
            data-dragging={isDragging ? 'true' : 'false'}
            className={styles.dragHandle}
            style={{ touchAction: dragDisabled ? undefined : 'none' }}
            onPointerDown={(event) => {
              onDragHandlePointerDown(event);
              onSortableDragHandlePointerDown?.(event);
            }}
          >
            <Drag theme='outline' size='11' strokeWidth={1.5} aria-hidden='true' />
          </button>
        </div>
        <div className={styles.preview}>
          <span className={styles.previewText} title={preview}>
            {preview}
          </span>
          {fileCountLabel ? <span className={styles.fileCount}>{fileCountLabel}</span> : null}
        </div>
      </div>
      <div className={styles.actions}>
        <button
          type='button'
          className={styles.sendNow}
          data-testid='command-queue-send-now'
          onClick={() => onSendNow(item.id)}
        >
          <span>{t('conversation.commandQueue.sendNow', { defaultValue: 'Send now' })}</span>
          <CornerDownLeft theme='outline' size='11' strokeWidth={1.5} aria-hidden='true' />
        </button>
        {onEdit ? (
          <button
            type='button'
            className={styles.iconBtn}
            aria-label={t('conversation.commandQueue.edit', { defaultValue: 'Edit' })}
            onClick={() => onEdit(item)}
          >
            <Edit theme='outline' size='12' strokeWidth={1.5} aria-hidden='true' />
          </button>
        ) : null}
        <button
          type='button'
          className={`${styles.iconBtn} ${styles.iconBtnDanger}`}
          aria-label={t('conversation.commandQueue.remove', { defaultValue: 'Remove' })}
          onClick={() => onRemove(item.id)}
        >
          <Delete theme='outline' size='12' strokeWidth={1.5} aria-hidden='true' />
        </button>
      </div>
    </div>
  );
};

const SortableQueueItem: React.FC<SortableQueueItemProps> = ({
  item,
  dragDisabled,
  dragHandleLabel,
  preview,
  fileCountLabel,
  t,
  onEdit,
  onSendNow,
  onRemove,
  onDragHandlePointerDown,
}) => {
  const { attributes, listeners, setNodeRef, setActivatorNodeRef, transform, transition, isDragging } = useSortable({
    id: item.id,
    disabled: dragDisabled,
  });

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    zIndex: isDragging ? 2 : undefined,
    position: 'relative',
  };

  return (
    <div ref={setNodeRef} style={style}>
      <QueueItemCard
        item={item}
        isDragging={isDragging}
        dragDisabled={dragDisabled}
        dragHandleLabel={dragHandleLabel}
        preview={preview}
        fileCountLabel={fileCountLabel}
        t={t}
        onEdit={onEdit}
        onSendNow={onSendNow}
        onRemove={onRemove}
        onDragHandlePointerDown={onDragHandlePointerDown}
        dragHandleRef={setActivatorNodeRef}
        dragHandleButtonProps={{
          ...(attributes as React.ButtonHTMLAttributes<HTMLButtonElement>),
          ...(listeners as React.ButtonHTMLAttributes<HTMLButtonElement>),
        }}
      />
    </div>
  );
};

const CommandQueuePanel: React.FC<CommandQueuePanelProps> = ({
  items,
  paused,
  interactionLocked,
  onPause,
  onResume,
  onInteractionLock,
  onInteractionUnlock,
  onEdit,
  onSendNow,
  onReorder,
  onRemove,
  onClear,
}) => {
  const { t } = useTranslation();
  const queueContainerRef = useRef<HTMLDivElement | null>(null);
  const activeDragHandleRef = useRef<HTMLButtonElement | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 8,
      },
    })
  );

  const clearDragHandleFocus = () => {
    activeDragHandleRef.current?.blur();
    activeDragHandleRef.current = null;
  };

  const handleDragEnd = ({ active, over }: DragEndEvent) => {
    onInteractionUnlock();
    clearDragHandleFocus();

    if (!over || active.id === over.id) {
      return;
    }

    onReorder(String(active.id), String(over.id));
  };

  const handleDragStart = () => {
    if (interactionLocked) {
      return;
    }

    onInteractionLock();
  };

  const handleDragCancel = () => {
    onInteractionUnlock();
    clearDragHandleFocus();
  };

  const dragHandleLabel = t('conversation.commandQueue.reorder', {
    defaultValue: 'Drag to reorder queued command',
  });
  const dragModifiers = useMemo(
    () => [restrictQueueDragToVerticalAxis, createRestrictToQueueContainerModifier(queueContainerRef)],
    []
  );
  const headerActionLabel = paused
    ? t('conversation.commandQueue.resume', { defaultValue: 'Resume' })
    : t('conversation.commandQueue.pause', { defaultValue: 'Pause' });

  if (items.length === 0) {
    return null;
  }

  return (
    <div className={styles.root} data-testid='command-queue-panel'>
      <div
        aria-label={t('conversation.commandQueue.title', { defaultValue: 'Queued Commands' })}
        className={styles.card}
      >
        <div className={styles.header}>
          <span className={styles.count} data-testid='command-queue-count'>
            {t('conversation.commandQueue.queuedCount', {
              count: items.length,
              defaultValue: `${items.length} Queued`,
            })}
          </span>
          <Dropdown
            trigger='click'
            droplist={
              <Menu>
                <Menu.Item key='pause-resume' onClick={paused ? onResume : onPause}>
                  {headerActionLabel}
                </Menu.Item>
                <Menu.Item key='clear-queue' onClick={onClear}>
                  {t('conversation.commandQueue.clear', { defaultValue: 'Clear queue' })}
                </Menu.Item>
              </Menu>
            }
          >
            <button
              type='button'
              className={styles.headerAction}
              aria-haspopup='menu'
              data-testid='command-queue-actions'
            >
              <span>{headerActionLabel}</span>
              <Down theme='outline' size='10' strokeWidth={1.5} aria-hidden='true' />
            </button>
          </Dropdown>
        </div>
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragStart={handleDragStart}
          onDragEnd={handleDragEnd}
          onDragCancel={handleDragCancel}
          modifiers={dragModifiers}
        >
          <SortableContext items={items.map((item) => item.id)} strategy={verticalListSortingStrategy}>
            <div
              ref={queueContainerRef}
              data-command-queue-list='true'
              data-drag-axis='vertical'
              data-drag-bounds='queue'
              className={styles.list}
            >
              {items.map((item) => {
                const preview = getCommandPreview(item.input);
                const fileCountLabel =
                  item.files.length > 0
                    ? t('conversation.commandQueue.files', {
                        count: item.files.length,
                        defaultValue: `${item.files.length} files`,
                      })
                    : null;

                return (
                  <SortableQueueItem
                    key={item.id}
                    item={item}
                    dragDisabled={false}
                    dragHandleLabel={dragHandleLabel}
                    preview={preview}
                    fileCountLabel={fileCountLabel}
                    t={t}
                    onEdit={onEdit}
                    onSendNow={onSendNow}
                    onRemove={onRemove}
                    onDragHandlePointerDown={(event) => {
                      activeDragHandleRef.current = event.currentTarget;
                    }}
                  />
                );
              })}
            </div>
          </SortableContext>
        </DndContext>
      </div>
    </div>
  );
};

export default CommandQueuePanel;
