import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { Button, Result, Spin } from '@arco-design/web-react';
import { Delete, Platte, Plus, Search } from '@icon-park/react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import {
  deleteCanvasProject,
  listCanvasProjects,
  type CanvasProjectMeta,
} from '../../videoCanvas/api';
import { loadVideoCanvasProjectPage } from '../../videoCanvas/loadProjectPage';
import { videoCanvasProjectPath } from '../../videoCanvas/routes';
import { rememberVideoGenerationCanvas } from '../routeMemory';
import { toUpdatedAtMs } from '../recentCreations';
import styles from './home.module.css';

function formatUpdatedAt(ms: number, t: TFunction): string {
  const diff = Date.now() - ms;
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  if (minutes < 1) return t('videoGeneration.time.justNow', { defaultValue: '刚刚' });
  if (minutes < 60)
    return t('videoGeneration.time.minutesAgo', {
      count: minutes,
      defaultValue: '{{count}} 分钟前',
    });
  if (hours < 24)
    return t('videoGeneration.time.hoursAgo', {
      count: hours,
      defaultValue: '{{count}} 小时前',
    });
  if (days === 1) return t('videoGeneration.time.yesterday', { defaultValue: '昨天' });
  if (days < 7)
    return t('videoGeneration.time.daysAgo', {
      count: days,
      defaultValue: '{{count}} 天前',
    });
  return t('videoGeneration.time.weeksAgo', { defaultValue: '上周' });
}

export type CanvasProjectCardProps = {
  project: CanvasProjectMeta;
  untitled: string;
  opening?: boolean;
  disabled?: boolean;
  deleting?: boolean;
  onOpen: (projectId: string) => void;
  onDelete?: (project: CanvasProjectMeta) => void;
};

export function CanvasProjectCard({
  project,
  untitled,
  opening = false,
  disabled = false,
  deleting = false,
  onOpen,
  onDelete,
}: CanvasProjectCardProps) {
  const { t } = useTranslation();
  const title = project.title || untitled;
  return (
    <div
      role='button'
      tabIndex={disabled ? -1 : 0}
      aria-disabled={disabled}
      aria-busy={opening}
      className={`${styles.canvasCard}${opening ? ` ${styles.canvasCardOpening}` : ''}`}
      style={disabled ? { cursor: 'default', opacity: 0.72, transform: 'none' } : undefined}
      onMouseEnter={() => void loadVideoCanvasProjectPage().catch(() => undefined)}
      onFocus={() => void loadVideoCanvasProjectPage().catch(() => undefined)}
      onClick={() => onOpen(project.project_id)}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onOpen(project.project_id);
        }
      }}
    >
      <span className={styles.canvasCardPreview}>
        {opening ? (
          <>
            <Spin size={16} />
            <small>
              {t('videoGeneration.create.gallery.opening', {
                defaultValue: '正在打开…',
              })}
            </small>
          </>
        ) : (
          <>
            <Platte size={26} />
            <small>
              {t('videoGeneration.create.gallery.kind', {
                defaultValue: '无限画布',
              })}
              {' · '}
              {t('videoGeneration.create.gallery.nodes', {
                count: project.node_count,
                defaultValue: '{{count}} 个节点',
              })}
            </small>
          </>
        )}
      </span>
      <span className={styles.canvasCardBody}>
        <span>
          <strong>{title}</strong>
          <small>{formatUpdatedAt(toUpdatedAtMs(project.updated_at), t)}</small>
        </span>
        {onDelete ? (
          <Button
            type='text'
            size='mini'
            status='danger'
            loading={deleting}
            disabled={disabled && !deleting}
            icon={<Delete size={14} />}
            aria-label={t('videoGeneration.create.gallery.deleteAria', {
              title,
              defaultValue: '删除 {{title}}',
            })}
            onClick={(event) => {
              event.stopPropagation();
              onDelete(project);
            }}
          />
        ) : null}
      </span>
    </div>
  );
}

const CanvasProjectGallery: React.FC<{ embedded?: boolean }> = ({ embedded = false }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [message, messageHolder] = useArcoMessage();
  const [projects, setProjects] = useState<CanvasProjectMeta[]>([]);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [openingId, setOpeningId] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const untitledCanvas = t('videoGeneration.create.gallery.untitled', {
    defaultValue: '未命名画布',
  });

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setProjects(await listCanvasProjects());
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (!projects.length) return;
    void loadVideoCanvasProjectPage().catch(() => undefined);
  }, [projects.length]);

  const displayed = useMemo(() => {
    const normalized = query.trim().toLowerCase();
    if (!normalized) return projects;
    return projects.filter(
      (project) =>
        project.title.toLowerCase().includes(normalized) ||
        project.project_id.toLowerCase().includes(normalized)
    );
  }, [projects, query]);

  const openProject = (projectId: string) => {
    if (openingId || creating || deletingId) return;
    const project = projects.find((item) => item.project_id === projectId);
    rememberVideoGenerationCanvas(projectId, project?.title ?? untitledCanvas);
    setOpeningId(projectId);
    void loadVideoCanvasProjectPage().catch(() => undefined);
    navigate(videoCanvasProjectPath(projectId));
  };

  const createBlankCanvas = async () => {
    if (creating || openingId) return;
    setCreating(true);
    try {
      const { createServerBackedCanvasProject } = await import(
        '../../videoCanvas/lib/ocBridge'
      );
      const id = await createServerBackedCanvasProject(untitledCanvas);
      rememberVideoGenerationCanvas(id, untitledCanvas);
      setOpeningId(id);
      navigate(videoCanvasProjectPath(id));
    } catch (cause) {
      message.error(
        t('videoGeneration.create.gallery.createFailed', {
          error: cause instanceof Error ? cause.message : String(cause),
          defaultValue: '创建失败：{{error}}',
        })
      );
    } finally {
      setCreating(false);
    }
  };

  const removeProject = async (project: CanvasProjectMeta) => {
    if (deletingId || openingId) return;
    setDeletingId(project.project_id);
    try {
      await deleteCanvasProject(project.project_id);
      setProjects((current) =>
        current.filter((item) => item.project_id !== project.project_id)
      );
      message.success(
        t('videoGeneration.create.gallery.deleteOk', {
          defaultValue: '画布已删除',
        })
      );
    } catch (cause) {
      message.error(
        t('videoGeneration.create.gallery.deleteFailed', {
          error: cause instanceof Error ? cause.message : String(cause),
          defaultValue: '删除失败：{{error}}',
        })
      );
    } finally {
      setDeletingId(null);
    }
  };

  return (
    <section className={embedded ? undefined : styles.gallerySection}>
      {messageHolder}
      {embedded ? (
        <div className={styles.galleryActions} style={{ marginBottom: 12, justifyContent: 'flex-end' }}>
          {projects.length > 0 ? (
            <label className={styles.gallerySearch}>
              <Search size={14} />
              <input
                value={query}
                placeholder={t('videoGeneration.create.gallery.searchPlaceholder', {
                  defaultValue: '搜索画布…',
                })}
                onChange={(event) => setQuery(event.target.value)}
              />
            </label>
          ) : null}
          <Button
            type='outline'
            size='small'
            loading={creating}
            disabled={Boolean(openingId)}
            onClick={() => void createBlankCanvas()}
          >
            <span className='inline-flex items-center gap-5px'>
              <Plus size={14} />
              {t('videoGeneration.create.gallery.createBlank', {
                defaultValue: '新建空白画布',
              })}
            </span>
          </Button>
        </div>
      ) : (
        <div className={styles.galleryHeader}>
          <div>
            <h2>
              {t('videoGeneration.create.gallery.title', {
                defaultValue: '最近画布',
              })}
            </h2>
            <p>
              {t('videoGeneration.create.gallery.subtitle', {
                defaultValue: '继续编辑已有项目，或从一个空白画布开始。',
              })}
            </p>
          </div>
          <div className={styles.galleryActions}>
            {projects.length > 0 ? (
              <label className={styles.gallerySearch}>
                <Search size={14} />
                <input
                  value={query}
                  placeholder={t('videoGeneration.create.gallery.searchPlaceholder', {
                    defaultValue: '搜索画布…',
                  })}
                  onChange={(event) => setQuery(event.target.value)}
                />
              </label>
            ) : null}
            <Button
              type='outline'
              size='small'
              loading={creating}
              disabled={Boolean(openingId)}
              onClick={() => void createBlankCanvas()}
            >
              <span className='inline-flex items-center gap-5px'>
                <Plus size={14} />
                {t('videoGeneration.create.gallery.createBlank', {
                  defaultValue: '新建空白画布',
                })}
              </span>
            </Button>
          </div>
        </div>
      )}

      {loading ? (
        <div className={styles.galleryCenter}>
          <Spin />
        </div>
      ) : error ? (
        <Result
          status='error'
          title={t('videoGeneration.create.gallery.loadError', {
            defaultValue: '画布加载失败',
          })}
          subTitle={error}
          extra={
            <Button onClick={() => void refresh()}>
              {t('videoGeneration.list.retry', { defaultValue: '重试' })}
            </Button>
          }
        />
      ) : projects.length === 0 ? (
        <button
          type='button'
          className={styles.canvasEmpty}
          onClick={() => void createBlankCanvas()}
        >
          <span>
            <Platte size={22} />
          </span>
          <strong>
            {t('videoGeneration.create.gallery.emptyTitle', {
              defaultValue: '创建你的第一张无限画布',
            })}
          </strong>
          <small>
            {t('videoGeneration.create.gallery.emptyDesc', {
              defaultValue: '在上方输入创意直接开始，或点击这里新建空白画布。',
            })}
          </small>
        </button>
      ) : (
        <div className={styles.canvasGrid}>
          {displayed.map((project) => {
            const isOpening = openingId === project.project_id;
            const isDisabled = Boolean(openingId) && !isOpening;
            return (
              <CanvasProjectCard
                key={project.project_id}
                project={project}
                untitled={untitledCanvas}
                opening={isOpening}
                disabled={isDisabled}
                deleting={deletingId === project.project_id}
                onOpen={openProject}
                onDelete={(item) => void removeProject(item)}
              />
            );
          })}
        </div>
      )}

      {!loading && projects.length > 0 && displayed.length === 0 ? (
        <div className={styles.galleryCenter}>
          {t('videoGeneration.create.gallery.filterEmpty', {
            defaultValue: '没有匹配的画布',
          })}
        </div>
      ) : null}
    </section>
  );
};

export default CanvasProjectGallery;
