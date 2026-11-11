import styles from './videoModelOption.module.css';

export { videoModelSelectFilter } from './videoModelTag';

export const videoModelSelectClassName = styles.select;

export function VideoModelOptionLabel({
  name,
  className,
}: {
  name: string;
  model: string;
  className?: string;
}) {
  return (
    <span className={[styles.trail, className].filter(Boolean).join(' ')} data-video-model-option=''>
      <span className={styles.name}>{name}</span>
    </span>
  );
}
