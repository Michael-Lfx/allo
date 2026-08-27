

import type { NomiModelSelection } from './useNomiModelSelection';
import { usePreviewContext } from '@/renderer/pages/conversation/Preview';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { getModelDisplayLabel } from '@/renderer/utils/model/agentLogo';
import { iconColors } from '@/renderer/styles/colors';
import ChatModelPickerMenu from '@/renderer/components/model/ChatModelPickerMenu';
import { Button, Dropdown } from '@arco-design/web-react';
import { Brain, Down } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import classNames from 'classnames';
import { useModelSelectorProviderLabel } from '@/renderer/hooks/agent/useModelSelectorProviderLabel';
import {
  AUTO_TIER_LABEL_FALLBACK,
  findChatModelOption,
  type AutoTier,
  type ChatModelPickerViewModel,
} from '@/renderer/utils/model/chatModelPicker';

const EMPTY_MODEL_PICKER: ChatModelPickerViewModel = {
  autoModels: [],
  cloudModels: [],
  otherProviderGroups: [],
} as const;

const NomiModelSelector: React.FC<{
  selection?: NomiModelSelection;
  disabled?: boolean;
  hasImageAttachments?: boolean;
  compact?: boolean;
  className?: string;
  popupVisible?: boolean;
  onPopupVisibleChange?: (visible: boolean) => void;
}> = ({
  selection,
  disabled = false,
  hasImageAttachments = false,
  compact: compactProp,
  className,
  popupVisible: popupVisibleProp,
  onPopupVisibleChange,
}) => {
  const { t } = useTranslation();
  const { isOpen: isPreviewOpen } = usePreviewContext();
  const layout = useLayoutContext();
  const [localModelPickerOpen, setLocalModelPickerOpen] = React.useState(false);
  const modelPickerOpen = popupVisibleProp ?? localModelPickerOpen;
  const compact = compactProp ?? (isPreviewOpen || layout?.isMobile);
  const defaultModelLabel = t('common.defaultModel');
  const providerLabel = useModelSelectorProviderLabel();

  const current_model = selection?.current_model;
  const modelPicker = selection?.modelPicker ?? EMPTY_MODEL_PICKER;
  const isCatalogLoading = Boolean(selection?.isModelCatalogLoading);
  const getDisplayModelName = selection?.getDisplayModelName;
  const handleSelectModel = selection?.handleSelectModel ?? (async () => false);

  const handleModelPickerVisibleChange = (visible: boolean) => {
    if (popupVisibleProp === undefined) {
      setLocalModelPickerOpen(visible);
    }
    onPopupVisibleChange?.(visible);
  };

  const renderLogo = () => <Brain theme='outline' size='14' fill={iconColors.secondary} className='shrink-0' />;

  const selectedLabel = getDisplayModelName?.(current_model?.use_model) ?? '';
  const selectedOption = findChatModelOption(modelPicker, current_model?.id, current_model?.use_model);

  const autoTierLabel = (tier?: AutoTier) =>
    tier
      ? t(`conversation.modelPicker.autoTier.${tier}`, {
          defaultValue: AUTO_TIER_LABEL_FALLBACK[tier],
        })
      : t('conversation.modelPicker.autoTier.unknown', { defaultValue: 'Auto' });

  const modelButtonLabel = selectedOption?.family === 'auto'
    ? `${t('conversation.modelPicker.auto', { defaultValue: 'Auto' })} · ${autoTierLabel(selectedOption.autoTier)}`
    : selectedOption?.label || selectedLabel;

  const label = getModelDisplayLabel({
    selected_value: current_model?.use_model,
    selectedLabel: modelButtonLabel,
    defaultModelLabel,
    fallbackLabel: t('conversation.welcome.selectModel'),
  });

  if (disabled || !selection) {
    return (
      <Button
        data-testid='nomi-model-selector'
        className={classNames(
          'sendbox-model-btn header-model-btn min-w-0',
          'flowy-icon-text-btn',
          'chat-model-picker-trigger',
          compact && 'chat-model-picker-trigger--compact',
          className
        )}
        shape='round'
        size='small'
        loading={selection?.isModelCatalogLoading}
        style={{ cursor: 'default' }}
        aria-label={
          selection?.isModelCatalogLoading
            ? t('common.loading')
            : t('conversation.welcome.useCliModel')
        }
        title={
          selection?.isModelCatalogLoading
            ? t('common.loading')
            : t('conversation.welcome.useCliModel')
        }
      >
        <span className='flowy-button-inline-content flex items-center gap-6px min-w-0'>
          <span className='sendbox-responsive-leading-icon' data-layout-part='leading-icon'>
            {renderLogo()}
          </span>
          <span className='sendbox-responsive-label block truncate min-w-0'>
            {t('conversation.welcome.useCliModel')}
          </span>
        </span>
      </Button>
    );
  }

  return (
    <Dropdown
      trigger='click'
      getPopupContainer={() => document.body}
      popupVisible={modelPickerOpen}
      onVisibleChange={handleModelPickerVisibleChange}
      droplist={
        <ChatModelPickerMenu
          viewModel={modelPicker}
          selectedOption={selectedOption}
          hasImageAttachments={hasImageAttachments}
          isLoading={isCatalogLoading}
          catalogError={selection?.modelCatalogError}
          onSelect={(option) => void handleSelectModel(option.provider, option.model)}
          onRetry={selection?.refreshModelCatalog}
          providerLabel={providerLabel}
        />
      }
    >
      <Button
        data-testid='nomi-model-selector'
        className={classNames(
          'sendbox-model-btn header-model-btn min-w-0',
          'flowy-icon-text-btn',
          'chat-model-picker-trigger',
          compact && 'chat-model-picker-trigger--compact',
          modelPickerOpen && 'sendbox-responsive-control-open',
          className
        )}
        shape='round'
        size='small'
        aria-label={label}
        aria-expanded={modelPickerOpen}
        data-popup-open={modelPickerOpen ? 'true' : undefined}
        title={label}
      >
        <span className='flowy-button-inline-content flex items-center gap-6px min-w-0'>
          <span className='sendbox-responsive-leading-icon' data-layout-part='leading-icon'>
            {renderLogo()}
          </span>
          <span className='sendbox-responsive-label block truncate min-w-0'>{label}</span>
          <span className='sendbox-responsive-chevron-slot' data-layout-part='chevron'>
            <Down
              theme='outline'
              size={11}
              fill={iconColors.secondary}
              className='sendbox-responsive-chevron shrink-0'
            />
          </span>
        </span>
      </Button>
    </Dropdown>
  );
};

export default NomiModelSelector;
