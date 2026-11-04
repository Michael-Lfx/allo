import { useTranslation } from 'react-i18next';
import { videoModelTagKind, type VideoModelTagKind } from './videoModelTag';
import styles from './videoModelOption.module.css';

export { videoModelSelectFilter, videoModelTagKind } from './videoModelTag';

export const videoModelSelectClassName = styles.select;

function VideoModelTagMark({ kind }: { kind: VideoModelTagKind }) {
  const { t } = useTranslation();
  const label =
    kind === 'value'
      ? t('videoGeneration.workspace.models.tags.value', { defaultValue: '性价比' })
      : t('videoGeneration.workspace.models.tags.passRate', { defaultValue: '高通过率' });
  return (
    <span className={`${styles.tag} ${kind === 'passRate' ? styles.tagPass : ''}`}>{label}</span>
  );
}

export function VideoModelOptionLabel({
  name,
  model,
  className,
}: {
  name: string;
  model: string;
  className?: string;
}) {
  const kind = videoModelTagKind(model);
  return (
    <span className={[styles.trail, className].filter(Boolean).join(' ')} data-video-model-option=''>
      <span className={styles.name}>{name}</span>
      {kind ? <VideoModelTagMark kind={kind} /> : null}
    </span>
  );
}
