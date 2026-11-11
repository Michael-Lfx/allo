/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { ipcBridge } from '@/common';
import { joinPath } from '@/common/chat/chatLib';
import { usePreviewLauncher } from '@/renderer/hooks/file/usePreviewLauncher';
import { getWorkspaceFileIconKind, type WorkspaceFileIconKind } from '@/renderer/pages/conversation/Workspace/components/WorkspaceFileIcon';
import { isPreviewSupportedExt } from '@/renderer/pages/conversation/Workspace/utils/filePreview';
import { diffColors, iconColors } from '@/renderer/styles/colors';
import { formatBytes } from '@/renderer/utils/file/formatBytes';
import { getFileTypeInfo } from '@/renderer/utils/file/fileType';
import { splitFileDisplayPath } from '@/renderer/utils/file/pathDisplay';
import { isDesktopShell } from '@/renderer/utils/platform';
import codeFileIcon from '@/renderer/assets/file-types/CODE.svg';
import docxFileIcon from '@/renderer/assets/file-types/DOCX.svg';
import javaFileIcon from '@/renderer/assets/file-types/JAVA.svg';
import jpgFileIcon from '@/renderer/assets/file-types/JPG.svg';
import mp3FileIcon from '@/renderer/assets/file-types/MP3.svg';
import mp4FileIcon from '@/renderer/assets/file-types/MP4.svg';
import pdfFileIcon from '@/renderer/assets/file-types/PDF.svg';
import pptFileIcon from '@/renderer/assets/file-types/PPT.svg';
import xlsFileIcon from '@/renderer/assets/file-types/XLS.svg';
import zipFileIcon from '@/renderer/assets/file-types/ZIP.svg';
import { Down, Export, FolderOpen, PreviewOpen, Shield } from '@icon-park/react';
import classNames from 'classnames';
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { TurnDeliverableItem } from '../turnDeliverablesModel';
import styles from './TurnDeliverablesCard.module.css';

const DEFAULT_VISIBLE_COUNT = 3;
const DELIVERABLE_SETTLE_MS = 180;

const FILE_SHEET_PATH =
  'M7.7 2.7h10.1l7.5 7.5v15.9a3.25 3.25 0 0 1-3.25 3.25H7.7a3.25 3.25 0 0 1-3.25-3.25V5.95A3.25 3.25 0 0 1 7.7 2.7Z';
const FILE_FOLD_PATH = 'M17.8 2.95v5.72c0 .9.73 1.63 1.63 1.63h5.63L17.8 2.95Z';
const FILE_MARK_STROKE = {
  stroke: '#fff',
  strokeWidth: 1.65,
  strokeLinecap: 'round',
  strokeLinejoin: 'round',
} as const;

const FILE_ICON_BY_KIND: Partial<Record<WorkspaceFileIconKind, string>> = {
  archive: zipFileIcon,
  audio: mp3FileIcon,
  code: codeFileIcon,
  excel: xlsFileIcon,
  image: jpgFileIcon,
  pdf: pdfFileIcon,
  presentation: pptFileIcon,
  video: mp4FileIcon,
  word: docxFileIcon,
};

// Markdown has no replacement in the supplied artwork set; keep its existing
// file-sheet glyph. HTML and plain text likewise retain the shared visual
// language until dedicated assets are available.
const CUSTOM_FILE_MARK_BY_KIND: Partial<Record<WorkspaceFileIconKind, React.ReactNode>> = {
  html: (
    <g {...FILE_MARK_STROKE} strokeWidth={1.55}>
      <circle cx='15.8' cy='18.8' r='4.8' />
      <path d='M11 18.8h9.6M15.8 14c2.1 2.4 2.1 7.2 0 9.6M15.8 14c-2.1 2.4-2.1 7.2 0 9.6' />
    </g>
  ),
  markdown: (
    <g {...FILE_MARK_STROKE}>
      <path d='M9.1 20.7v-5.1l2.3 2.5 2.3-2.5v5.1' />
      <path d='M18.2 15.9v4.8m-1.75-1.75 1.75 1.75 1.75-1.75' />
    </g>
  ),
  text: (
    <g {...FILE_MARK_STROKE}>
      <path d='M9.3 15.3h13.1M9.3 18.7h13.1M9.3 22.1h8.7' />
    </g>
  ),
};

const fileTitle = (fileName: string): string => {
  const dot = fileName.lastIndexOf('.');
  if (dot <= 0) return fileName;
  return fileName.slice(0, dot);
};

const DeliverableGlyph: React.FC<{ fileName: string }> = ({ fileName }) => {
  const kind = getWorkspaceFileIconKind(fileName);
  const extension = fileName.toLowerCase().split('.').pop() || '';
  const assetIcon = kind === 'code' && extension === 'java' ? javaFileIcon : FILE_ICON_BY_KIND[kind];

  if (assetIcon) {
    return (
      <span className={styles.glyph} data-kind={kind} aria-hidden='true'>
        <img className={styles.assetGlyph} src={assetIcon} alt='' draggable={false} />
      </span>
    );
  }

  const customGlyphSize = kind === 'markdown' ? 32 : 28;

  return (
    <span className={styles.glyph} data-kind={kind} aria-hidden='true'>
      <svg className={styles.fileGlyph} width={customGlyphSize} height={customGlyphSize} viewBox='0 0 32 32' fill='none'>
        <path fill='currentColor' d={FILE_SHEET_PATH} />
        <path fill='#fff' fillOpacity='0.34' d={FILE_FOLD_PATH} />
        {CUSTOM_FILE_MARK_BY_KIND[kind]}
      </svg>
    </span>
  );
};

type ResolvedDeliverable = TurnDeliverableItem & { statPath?: string };

/**
 * A reported deliverable may only be presented as available after the backend
 * confirms the file still exists inside an allowed root. Results are cached
 * per app session; a rejected probe is retried on the next mount instead of
 * being cached, so transient transport failures cannot permanently hide a card.
 */
const availabilityCache = new Map<string, Promise<number | null>>();

const probeAvailability = (statPath: string, workspace?: string): Promise<number | null> => {
  const key = `${workspace ?? ''}|${statPath}`;
  const cached = availabilityCache.get(key);
  if (cached) return cached;

  const probe = ipcBridge.fs.getFileMetadata
    .invoke({ path: statPath, workspace })
    .then((metadata) => (metadata && metadata.isDirectory !== true ? metadata.size : null))
    .catch((error) => {
      availabilityCache.delete(key);
      throw error;
    });
  availabilityCache.set(key, probe);
  return probe;
};

const getStatPath = (item: TurnDeliverableItem, workspace?: string): string | undefined => {
  if (item.absolutePath) return item.absolutePath;
  if (workspace) return joinPath(workspace, item.relativePath);
  return undefined;
};

export const useTurnDeliverableAvailability = (
  items: TurnDeliverableItem[],
  workspace?: string
): { pending: boolean; available: ResolvedDeliverable[] } => {
  const [state, setState] = useState<{ key: string; available: ResolvedDeliverable[] } | null>(null);

  const itemsKey = useMemo(
    () => `${workspace ?? ''}|${items.map((item) => `${item.tier}:${item.relativePath}`).join(',')}`,
    [items, workspace]
  );
  // Cards exist only for closed turns, so item content is stable once the key
  // is. Reading through a ref keeps streaming re-renders (new array identity,
  // same key) from re-probing the backend on every token.
  const itemsRef = useRef(items);
  itemsRef.current = items;

  useEffect(() => {
    let alive = true;
    const timer = window.setTimeout(() => {
      void Promise.all(
        itemsRef.current.map(async (item): Promise<ResolvedDeliverable | null> => {
          const statPath = getStatPath(item, workspace);
          // Committed receipts are already integrity-audited by the backend on
          // both commit and history read; never re-gate them on a client probe.
          if (item.tier === 'receipt') return { ...item, statPath };
          if (!statPath) return null;
          try {
            const size = await probeAvailability(statPath, workspace);
            if (size === null) return null;
            return { ...item, statPath, sizeBytes: item.sizeBytes ?? size };
          } catch {
            return null;
          }
        })
      ).then((resolved) => {
        if (!alive) return;
        setState({
          key: itemsKey,
          available: resolved.filter((item): item is ResolvedDeliverable => item !== null),
        });
      });
    }, DELIVERABLE_SETTLE_MS);

    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [itemsKey, workspace]);

  if (!state || state.key !== itemsKey) return { pending: true, available: [] };
  return { pending: false, available: state.available };
};

const KIND_FALLBACK: Record<WorkspaceFileIconKind, string> = {
  archive: 'Archive',
  audio: 'Audio',
  code: 'Code',
  excel: 'Excel',
  html: 'HTML',
  image: 'Image',
  markdown: 'Markdown',
  pdf: 'PDF',
  presentation: 'PowerPoint',
  text: 'Text',
  video: 'Video',
  word: 'Word',
};

const kindLabelKey = (kind: WorkspaceFileIconKind): `messages.turnDeliverables.kind.${WorkspaceFileIconKind}` =>
  `messages.turnDeliverables.kind.${kind}`;

const DeliverableCard: React.FC<{
  item: ResolvedDeliverable;
}> = ({ item }) => {
  const { t } = useTranslation();
  const { launchPreview, canPreview } = usePreviewLauncher();
  const showShellActions = isDesktopShell() && Boolean(item.statPath);
  const displayPath = splitFileDisplayPath(item.relativePath, item.fileName);
  const previewSupported = canPreview && isPreviewSupportedExt(item.fileName);
  const kind = getWorkspaceFileIconKind(item.fileName);
  const sizeLabel = item.sizeBytes !== undefined && item.sizeBytes > 0 ? formatBytes(item.sizeBytes) : '';
  const kindLabel = t(kindLabelKey(kind), { defaultValue: KIND_FALLBACK[kind] });
  const meta = sizeLabel
    ? t('messages.turnDeliverables.meta', {
        kind: kindLabel,
        size: sizeLabel,
        defaultValue: '{{kind}} · {{size}}',
      })
    : kindLabel;
  const verifiedLabel = t('messages.turnDeliverables.verified', {
    defaultValue: 'Integrity verified (SHA-256)',
  });
  const title = fileTitle(displayPath.fileName);
  const insertions = item.insertions ?? 0;
  const deletions = item.deletions ?? 0;
  const hasEdits = insertions > 0 || deletions > 0;
  const interactive = previewSupported || showShellActions;
  const showActions = hasEdits || previewSupported || showShellActions;

  const handlePreview = () => {
    const { contentType, editable, language } = getFileTypeInfo(item.fileName);
    void launchPreview({
      relativePath: item.absolutePath ? undefined : item.relativePath,
      originalPath: item.absolutePath ?? item.statPath,
      file_name: item.fileName,
      contentType,
      editable,
      language,
      diffContent: item.diff,
    });
  };

  const handleDiffPreview = () => {
    if (!item.diff) return;
    void launchPreview({
      file_name: item.fileName,
      contentType: 'diff',
      editable: false,
      language: 'diff',
      diffContent: item.diff,
    });
  };

  const handleActivate = () => {
    if (previewSupported) {
      handlePreview();
      return;
    }
    if (showShellActions && item.statPath) {
      void ipcBridge.shell.openFile.invoke(item.statPath);
    }
  };

  const body = (
    <>
      <DeliverableGlyph fileName={item.fileName} />
      <span className={styles.copy}>
        <span className={styles.name}>{title}</span>
        <span className={styles.meta}>
          <span className={styles.metaText}>{meta}</span>
          {item.tier === 'receipt' && (
            <span className={styles.verified} title={verifiedLabel}>
              <Shield theme='outline' size='13' fill='currentColor' />
            </span>
          )}
        </span>
      </span>
    </>
  );

  return (
    <div className={styles.card} data-deliverable-path={item.relativePath}>
      {interactive ? (
        <button
          type='button'
          className={styles.main}
          title={displayPath.fullPath}
          aria-label={item.tier === 'receipt' ? `${title}. ${meta}. ${verifiedLabel}` : `${title}. ${meta}`}
          onClick={handleActivate}
        >
          {body}
        </button>
      ) : (
        <div className={classNames(styles.main, styles.mainStatic)} title={displayPath.fullPath}>
          {body}
        </div>
      )}
      {showActions && (
      <div className={styles.actions}>
        {hasEdits &&
          (item.diff ? (
            <button
              type='button'
              className={classNames(styles.action, styles.actionDiff)}
              title={t('messages.turnDeliverables.viewDiff', { defaultValue: 'View changes' })}
              onClick={handleDiffPreview}
            >
              {insertions > 0 && <span style={{ color: diffColors.addition }}>+{insertions}</span>}
              {deletions > 0 && <span style={{ color: diffColors.deletion }}>-{deletions}</span>}
            </button>
          ) : (
            <span className={classNames(styles.action, styles.actionDiff)}>
              {insertions > 0 && <span style={{ color: diffColors.addition }}>+{insertions}</span>}
              {deletions > 0 && <span style={{ color: diffColors.deletion }}>-{deletions}</span>}
            </span>
          ))}
        {previewSupported && (
          <button
            type='button'
            className={styles.action}
            title={t('preview.preview')}
            aria-label={t('preview.preview')}
            onClick={handlePreview}
          >
            <PreviewOpen theme='outline' size='14' fill={iconColors.secondary} />
          </button>
        )}
        {showShellActions && (
          <>
            <button
              type='button'
              className={styles.action}
              title={t('messages.turnDeliverables.open', { defaultValue: 'Open' })}
              aria-label={t('messages.turnDeliverables.open', { defaultValue: 'Open' })}
              onClick={() => void ipcBridge.shell.openFile.invoke(item.statPath!)}
            >
              <Export theme='outline' size='14' fill={iconColors.secondary} />
            </button>
            <button
              type='button'
              className={styles.action}
              title={t('messages.turnDeliverables.reveal', { defaultValue: 'Reveal in folder' })}
              aria-label={t('messages.turnDeliverables.reveal', { defaultValue: 'Reveal in folder' })}
              onClick={() => void ipcBridge.shell.showItemInFolder.invoke(item.statPath!)}
            >
              <FolderOpen theme='outline' size='14' fill={iconColors.secondary} />
            </button>
          </>
        )}
      </div>
      )}
    </div>
  );
};

/**
 * Per-turn deliverables card: every verified file artifact of one successfully
 * completed turn, rendered right below that turn's final assistant reply.
 * Renders nothing while availability is being confirmed and nothing when no
 * trustworthy deliverable remains — an empty card must never appear.
 */
const TurnDeliverablesCard: React.FC<{
  items: TurnDeliverableItem[];
  workspace?: string;
}> = ({ items, workspace }) => {
  const { t } = useTranslation();
  const { pending, available } = useTurnDeliverableAvailability(items, workspace);
  const [showAll, setShowAll] = useState(false);

  if (pending || available.length === 0) return null;

  const visible = showAll ? available : available.slice(0, DEFAULT_VISIBLE_COUNT);
  const hiddenCount = available.length - visible.length;

  const revealControl =
    hiddenCount > 0 || showAll ? (
      <button
        type='button'
        aria-expanded={showAll}
        className={showAll ? styles.collapse : styles.more}
        onClick={() => setShowAll(!showAll)}
      >
        {showAll ? (
          <Down
            theme='outline'
            size='14'
            fill={iconColors.secondary}
            className={classNames(styles.collapseIcon, styles.collapseIconOpen)}
          />
        ) : (
          <span className={styles.moreIcon}>
            <FolderOpen theme='outline' size='16' fill={iconColors.secondary} />
          </span>
        )}
        <span className={showAll ? undefined : styles.moreLabel}>
          {showAll
            ? t('messages.turnDeliverables.showLess', { defaultValue: 'Show less' })
            : t('messages.turnDeliverables.showMore', {
                count: hiddenCount,
                defaultValue: 'Show {{count}} more files',
              })}
        </span>
      </button>
    ) : null;

  return (
    <div data-testid='turn-deliverables-card' className={styles.root}>
      <div className={styles.header}>
        <span className={styles.statusDot} />
        <span className={styles.headerTitle}>
          {t('messages.turnDeliverables.title', {
            count: available.length,
            defaultValue: 'Generated {{count}} files',
          })}
        </span>
      </div>

      <div className={styles.grid} data-testid='turn-deliverables-grid'>
        {visible.map((item) => (
          <DeliverableCard key={item.absolutePath ?? item.relativePath} item={item} />
        ))}
        {!showAll && revealControl}
      </div>
      {showAll && revealControl}
    </div>
  );
};

export default React.memo(TurnDeliverablesCard);
