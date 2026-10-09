import React from 'react';
import { useTranslation } from 'react-i18next';
import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';
import type { PetMessage } from './types';
import { isActivePhase } from './types';

interface PetStatusToastProps {
  messages: PetMessage[];
  onDismiss: (id: string) => void;
  onOpen?: (href: string) => void;
}

const PetStatusToast: React.FC<PetStatusToastProps> = ({ messages, onDismiss, onOpen }) => {
  const { t } = useTranslation();
  if (messages.length === 0) return null;
  return (
    <div className='nomi-pet-feed' data-companion-hit onMouseDown={(event) => event.stopPropagation()}>
      {messages.map((message) => {
        const title = t(message.titleKey, message.titleParams);
        const source = t(`nomi.petMessage.sources.${message.source}` as I18nKey);
        const active = isActivePhase(message.phase);
        return (
          <div
            key={message.id}
            className={`nomi-pet-chip nomi-pet-chip--${message.phase}`}
            role='status'
            aria-live={active ? 'polite' : 'off'}
          >
            <button
              type='button'
              className='nomi-pet-chip__body'
              onClick={() => {
                if (message.href) onOpen?.(message.href);
              }}
              title={message.detail || title}
            >
              <span className='nomi-pet-chip__source'>{source}</span>
              <span className='nomi-pet-chip__title'>{title}</span>
              {message.detail ? <span className='nomi-pet-chip__detail'>{message.detail}</span> : null}
              {message.progress != null ? (
                <span className='nomi-pet-chip__bar' aria-hidden='true'>
                  <span style={{ width: `${Math.round(Math.min(1, Math.max(0, message.progress)) * 100)}%` }} />
                </span>
              ) : null}
            </button>
            <button
              type='button'
              className='nomi-pet-chip__x'
              aria-label={t('nomi.petMessage.dismiss')}
              onClick={() => onDismiss(message.id)}
            >
              ×
            </button>
          </div>
        );
      })}
    </div>
  );
};

export default PetStatusToast;
