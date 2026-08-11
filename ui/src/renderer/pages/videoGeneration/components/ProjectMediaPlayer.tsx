/**
 * Auth-aware project media player (blob URL via Montage files API).
 */
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Spin } from '@arco-design/web-react';
import { fetchProjectFileBlob } from '../api';

interface ProjectMediaPlayerProps {
  projectId: string;
  /** Relative path under the project root, e.g. `renders/final.mp4`. */
  relPath: string;
  title?: string;
  className?: string;
}

const ProjectMediaPlayer: React.FC<ProjectMediaPlayerProps> = ({
  projectId,
  relPath,
  title,
  className,
}) => {
  const { t } = useTranslation();
  const [blobUrl, setBlobUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    let objectUrl: string | null = null;
    setLoading(true);
    setError(null);
    setBlobUrl(null);
    void fetchProjectFileBlob(projectId, relPath)
      .then((blob) => {
        if (cancelled) return;
        objectUrl = URL.createObjectURL(blob);
        setBlobUrl(objectUrl);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(e instanceof Error ? e.message : String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [projectId, relPath]);

  if (loading) {
    return (
      <div className='flex min-h-180px items-center justify-center rd-12px bg-[var(--color-fill-2)]'>
        <Spin />
      </div>
    );
  }

  if (error || !blobUrl) {
    return (
      <div className='rd-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-2)] px-14px py-16px text-13px text-[var(--color-text-3)]'>
        {t('videoGeneration.workspace.finalVideo.loadFailed', {
          defaultValue: '成片加载失败',
        })}
        {error ? `：${error}` : ''}
      </div>
    );
  }

  return (
    <div className={className}>
      {title ? (
        <div className='mb-8px text-12px font-650 text-[var(--color-text-2)]'>{title}</div>
      ) : null}
      <video
        key={blobUrl}
        className='w-full max-h-480px rd-12px bg-black object-contain'
        src={blobUrl}
        controls
        playsInline
        preload='metadata'
      />
      <div className='mt-6px truncate text-11px text-[var(--color-text-4)]' title={relPath}>
        {relPath}
      </div>
    </div>
  );
};

export default ProjectMediaPlayer;
