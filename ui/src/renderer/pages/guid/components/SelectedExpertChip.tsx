import { FlowyIcon } from '@renderer/components/base/FlowyIcon';
import { Button } from '@arco-design/web-react';
import React, { createContext, useContext } from 'react';
import { useTranslation } from 'react-i18next';
import { CUSTOM_AVATAR_IMAGE_MAP } from '../constants';
import { isEmoji, resolveAvatarImageSrc } from '@/renderer/pages/settings/PresetSettings/presetUtils';
import styles from '../index.module.css';

type SelectedExpertChipProps = {
  name: string;
  avatar?: string;
  onClear: () => void;
  testId?: string;
};

type SelectedExpertContextValue = {
  name: string;
  avatar?: string;
  onClear: () => void;
};

const SelectedExpertContext = createContext<SelectedExpertContextValue | null>(null);

export const SelectedExpertProvider = SelectedExpertContext.Provider;

const SelectedExpertChip: React.FC<SelectedExpertChipProps> = ({
  name,
  avatar,
  onClear,
  testId = 'guid-selected-expert',
}) => {
  const { t } = useTranslation();
  const avatarValue = avatar?.trim();
  const avatarImage = resolveAvatarImageSrc(avatarValue, CUSTOM_AVATAR_IMAGE_MAP);

  return (
    <Button
      type='text'
      shape='round'
      size='small'
      className={`${styles.actionExpertButton} flowy-icon-text-btn`}
      onClick={onClear}
      aria-label={`${t('common.remove')} ${name}`}
      title={name}
      data-testid={testId}
    >
      <span className={`${styles.actionExpertButtonContent} flowy-button-inline-content`}>
        <span className={styles.actionExpertMark} aria-hidden='true'>
          <span className={styles.actionExpertAvatar}>
            {avatarImage ? (
              <img src={avatarImage} alt='' className={styles.actionExpertAvatarImage} />
            ) : avatarValue && isEmoji(avatarValue) ? (
              <span className={styles.actionExpertEmoji}>{avatarValue}</span>
            ) : (
              <FlowyIcon name='robot' size={14} />
            )}
          </span>
          <span className={styles.actionExpertDismiss}>
            <FlowyIcon name='close' size={12} />
          </span>
        </span>
        <span className={styles.actionExpertLabel}>{name}</span>
      </span>
    </Button>
  );
};

export function ConversationExpertChip() {
  const value = useContext(SelectedExpertContext);
  if (!value?.name) return null;
  return (
    <SelectedExpertChip
      name={value.name}
      avatar={value.avatar}
      onClear={value.onClear}
      testId='sendbox-selected-expert'
    />
  );
}

export default SelectedExpertChip;
