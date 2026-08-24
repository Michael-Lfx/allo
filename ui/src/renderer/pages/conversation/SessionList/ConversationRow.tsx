

import { CapabilityIconCluster } from '@/renderer/components/capability/CapabilityIcon';
import MarqueeText from '@/renderer/components/base/MarqueeText';
import FlexFullContainer from '@/renderer/components/layout/FlexFullContainer';
import ConversationHoverCard from '@/renderer/pages/conversation/components/ConversationHoverCard';
import { cleanupSiderTooltips, getSiderTooltipProps } from '@/renderer/utils/ui/siderTooltip';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { Checkbox, Dropdown, Menu, Popover, Spin, Tooltip } from '@arco-design/web-react';
import { DeleteOne, EditOne, Export, MoreOne, Pushpin } from '@icon-park/react';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';

import type { ConversationRowProps } from './types';
import { isConversationPinned } from './utils/conversationPinned';
import { buildSessionCapabilityItems, CAPABILITY_ICON_SIZE } from './utils/sessionCapabilityItems';
import { formatSessionAgeLabel } from './utils/sessionAge';

const ConversationRow: React.FC<ConversationRowProps> = (props) => {
  const {
    conversation,
    isGenerating,
    hasCompletionUnread,
    collapsed,
    tooltipEnabled,
    batchMode,
    checked,
    selected,
    menuVisible,
    showSessionAge = true,
  } = props;
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const {
    onToggleChecked,
    onConversationClick,
    onOpenMenu,
    onMenuVisibleChange,
    onEditStart,
    onDelete,
    onExport,
    onTogglePin,
    getJobStatus,
    autoworkState,
    idmmState,
  } = props;
  const { t } = useTranslation();
  const isPinned = isConversationPinned(conversation);
  const cronStatus = getJobStatus(conversation.id);
  const siderTooltipProps = getSiderTooltipProps(tooltipEnabled);
  const ageLabel = formatSessionAgeLabel(t, conversation.created_at);
  const displayName = conversation.name?.trim() || t('conversation.historySearch.untitled');

  // Session-level capability markers (trailing group): 召唤伙伴 → 定时任务 →
  // 自动工作 → 智能决策, shared builder with TerminalRow. The summon marker
  // reads straight off the synced conversation extra (设计 B5).
  const summoned = Boolean((conversation.extra as { summon?: unknown } | undefined)?.summon);
  const capabilityItems = buildSessionCapabilityItems(t, { cronStatus, autoworkState, idmmState, summoned });

  const handleRowClick = () => {
    cleanupSiderTooltips();
    if (batchMode) {
      onToggleChecked(conversation);
      return;
    }
    onConversationClick(conversation);
  };

  const handleRowContextMenu = (event: React.MouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.stopPropagation();
    cleanupSiderTooltips();
    if (batchMode) {
      return;
    }
    onOpenMenu(conversation);
  };

  const showUnreadDot = !batchMode && hasCompletionUnread && !isGenerating;
  const showAgeMeta = showSessionAge && !!ageLabel && !collapsed;
  const showDesktopTrailingMeta = !collapsed && !isMobile && (showAgeMeta || showUnreadDot);
  const showCompactUnreadDot = showUnreadDot && (collapsed || isMobile);
  const showHoverPinnedIcon = !batchMode && isPinned && !isMobile && !isGenerating;

  const unreadDot = (
    <span
      className='h-8px w-8px shrink-0 rounded-full bg-[var(--color-primary)] shadow-[0_0_0_2px_rgba(var(--primary-6),0.18)]'
      aria-hidden
    />
  );

  const renderRow = () => (
      <div
        id={'c-' + conversation.id}
        className={classNames(
          'chat-history__item h-34px rd-10px flex items-center group cursor-pointer relative overflow-hidden shrink-0 conversation-item [&.conversation-item+&.conversation-item]:mt-2px min-w-0 transition-colors',
          collapsed ? 'justify-center px-0' : 'justify-start gap-8px pr-16px',
          // Nested workpath conversations use the compact baseline; the parent
          // row and the surrounding drawer already provide the hierarchy cue.
          !collapsed && 'pl-18px',
          {
            'hover:bg-fill-2': !batchMode && !selected,
            // Keep the active conversation visually lifted like a hovered row;
            // batch selection retains its own state color below.
            'session-list-active-row !text-t-primary': selected && !batchMode,
            'bg-[rgba(var(--primary-6),0.08)]': batchMode && checked,
          }
        )}
        onClick={handleRowClick}
        onContextMenu={handleRowContextMenu}
      >
        {batchMode && (
          <span
            className='mr-8px flex-center'
            onClick={(event) => {
              event.stopPropagation();
              onToggleChecked(conversation);
            }}
          >
            <Checkbox checked={checked} className='session-batch-selection-checkbox' />
          </span>
        )}
        {isGenerating && !batchMode && <Spin size={16} />}
        {showHoverPinnedIcon && (
          <span
            className='absolute left-18px top-1/2 z-1 -translate-y-1/2 text-t-secondary pointer-events-none opacity-0 group-hover:opacity-100 transition-opacity'
            style={{ lineHeight: 0 }}
          >
            <Pushpin theme='outline' size='14' />
          </span>
        )}
        {showHoverPinnedIcon && <span className='w-22px shrink-0' aria-hidden='true' />}
        {/* Capability markers are session identity, so they sit before the text and
            stay visible while hover-only actions appear on the right. */}
        {!batchMode && !collapsed && capabilityItems.length > 0 && (
          <CapabilityIconCluster items={capabilityItems} size={CAPABILITY_ICON_SIZE} className='shrink-0' />
        )}
        {/* Name owns the flexible middle; age is a fixed right-aligned marker so
            rows scan cleanly without metadata hugging the title. */}
        <FlexFullContainer
          className='h-24px min-w-0 flex-1 collapsed-hidden'
          containerClassName='flex items-center'
        >
          <MarqueeText
            text={displayName}
            trigger='hover'
            title=''
            disabled={collapsed || batchMode || isMobile || menuVisible}
            className='chat-history__item-name block overflow-hidden text-ellipsis whitespace-nowrap min-w-0 text-14px font-[500] lh-24px text-t-primary'
          />
        </FlexFullContainer>
        {/* Keep trailing width in the layout on hover (invisible, not display:none)
            so the title ellipsis width does not flash wider then narrower. */}
        {showDesktopTrailingMeta && (
          <span
            className={classNames('flex shrink-0 items-center gap-6px', {
              'group-hover:invisible': !menuVisible,
              invisible: menuVisible,
            })}
          >
            {showAgeMeta && (
              <span className='session-age-meta w-40px text-right text-11px text-t-tertiary'>{ageLabel}</span>
            )}
            {showUnreadDot && unreadDot}
          </span>
        )}
        {/* When there is no age/unread meta, still reserve the more-button width. */}
        {!batchMode && !collapsed && !isMobile && !showDesktopTrailingMeta && (
          <span className='w-20px shrink-0' aria-hidden />
        )}

        {showCompactUnreadDot && (
          <span className='absolute right-8px top-1/2 -translate-y-1/2 flex items-center justify-center group-hover:hidden'>
            {unreadDot}
          </span>
        )}

        {!batchMode && (
          <div
            className={classNames(
              'absolute right-8px top-1/2 -translate-y-1/2 items-center justify-end !collapsed-hidden',
              {
                flex: isMobile || menuVisible,
                'hidden group-hover:flex': !isMobile && !menuVisible,
              }
            )}
            onClick={(event) => {
              event.stopPropagation();
            }}
          >
            <Dropdown
              droplist={
                <Menu
                  onClickMenuItem={(key) => {
                    if (key === 'pin') {
                      onTogglePin(conversation);
                      return;
                    }
                    if (key === 'rename') {
                      onEditStart(conversation);
                      return;
                    }
                    if (key === 'export') {
                      onExport?.(conversation);
                      return;
                    }
                    if (key === 'delete') {
                      onDelete(conversation.id);
                    }
                  }}
                >
                  <Menu.Item key='pin'>
                    <div className='flex items-center gap-8px'>
                      <Pushpin theme='outline' size='14' />
                      <span>{isPinned ? t('conversation.history.unpin') : t('conversation.history.pin')}</span>
                    </div>
                  </Menu.Item>
                  <Menu.Item key='rename'>
                    <div className='flex items-center gap-8px'>
                      <EditOne theme='outline' size='14' />
                      <span>{t('conversation.history.rename')}</span>
                    </div>
                  </Menu.Item>
                  {onExport && (
                    <Menu.Item key='export'>
                      <div className='flex items-center gap-8px'>
                        <Export theme='outline' size='14' />
                        <span>{t('conversation.history.export')}</span>
                      </div>
                    </Menu.Item>
                  )}
                  <Menu.Item key='delete'>
                    <div className='flex items-center gap-8px text-[rgb(var(--warning-6))]'>
                      <DeleteOne theme='outline' size='14' />
                      <span>{t('conversation.history.deleteTitle')}</span>
                    </div>
                  </Menu.Item>
                </Menu>
              }
              trigger='click'
              position='br'
              popupVisible={menuVisible}
              onVisibleChange={(visible) => onMenuVisibleChange(conversation.id, visible)}
              getPopupContainer={() => document.body}
              unmountOnExit={false}
            >
              <span
                className={classNames(
                  'flex-center cursor-pointer transition-colors text-t-secondary hover:text-t-primary size-20px rd-4px sider-action-btn session-action-icon-btn',
                  {
                    flex: isMobile || menuVisible,
                    'hidden group-hover:flex': !isMobile && !menuVisible,
                  }
                )}
                onClick={(event) => {
                  event.stopPropagation();
                  onOpenMenu(conversation);
                }}
              >
                <MoreOne theme='outline' size='14' fill='currentColor' className='block leading-none' />
              </span>
            </Dropdown>
          </div>
        )}
      </div>
  );

  // When collapsed, show a simple tooltip (sidebar behavior). When expanded, show a richer Popover card.
  if (collapsed) {
    return (
      <Tooltip
        key={conversation.id}
        {...siderTooltipProps}
        content={displayName}
        position='right'
      >
        {renderRow()}
      </Tooltip>
    );
  }

  return (
    <Popover
      key={conversation.id}
      trigger='hover'
      position='right'
      content={<ConversationHoverCard conversation={conversation} />}
      triggerProps={{ mouseEnterDelay: 400 }}
    >
      {renderRow()}
    </Popover>
  );
};

export default ConversationRow;
