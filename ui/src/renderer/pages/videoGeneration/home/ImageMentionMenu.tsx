import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import {
  HOME_IMAGE_MENTION_MENU_WIDTH_PX,
  placeHomeImageMentionMenu,
  type TextareaCaretRect,
} from './mentionCaret';
import styles from './home.module.css';

export type HomeImageMentionCandidate = {
  index: number;
  label: string;
  name: string;
  previewUrl: string;
};

export function ImageMentionMenu({
  caret,
  candidates,
  activeIndex,
  onSelect,
}: {
  caret: TextareaCaretRect;
  candidates: HomeImageMentionCandidate[];
  activeIndex: number;
  onSelect: (index: number) => void;
}) {
  const { t } = useTranslation();
  const estimatedHeight = candidates.length === 0 ? 44 : Math.min(224, 8 + candidates.length * 48);
  const geometry = placeHomeImageMentionMenu(
    caret,
    { width: window.innerWidth, height: window.innerHeight },
    { width: HOME_IMAGE_MENTION_MENU_WIDTH_PX, estimatedHeight },
  );

  return createPortal(
    <div
      className={styles.mentionMenu}
      style={geometry}
      role='listbox'
      aria-label={t('videoGeneration.create.composer.mentionMenuAria', {
        defaultValue: '引用已上传的图片',
      })}
      onMouseDown={(event) => event.preventDefault()}
    >
      {candidates.length === 0 ? (
        <div className={styles.mentionEmpty}>
          {t('videoGeneration.create.composer.mentionEmpty', {
            defaultValue: '没有匹配的图片',
          })}
        </div>
      ) : (
        candidates.map((item, index) => (
          <button
            key={`${item.index}-${item.label}`}
            type='button'
            role='option'
            aria-selected={index === activeIndex}
            className={`${styles.mentionItem} ${index === activeIndex ? styles.mentionItemActive : ''}`}
            onMouseDown={(event) => {
              event.preventDefault();
              onSelect(item.index);
            }}
          >
            <img src={item.previewUrl} alt='' className={styles.mentionThumb} />
            <span className={styles.mentionCopy}>
              <strong>{item.label}</strong>
              {item.name ? <em>{item.name}</em> : null}
            </span>
          </button>
        ))
      )}
    </div>,
    document.body,
  );
}
