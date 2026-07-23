

import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { FolderOpen, FileText, Pic, VideoOne } from '@icon-park/react';
import type { ArtifactNode } from '../types';

function guessKind(node: ArtifactNode): 'dir' | 'image' | 'video' | 'text' {
  if (node.is_dir) return 'dir';
  const mime = (node.mime ?? '').toLowerCase();
  const p = node.path.toLowerCase();
  if (mime.startsWith('image/') || /\.(png|jpe?g|gif|webp|bmp)$/.test(p)) return 'image';
  if (mime.startsWith('video/') || /\.(mp4|webm|mov|avi|mkv)$/.test(p)) return 'video';
  return 'text';
}

function NodeIcon({ kind }: { kind: ReturnType<typeof guessKind> }) {
  const props = { theme: 'outline' as const, size: 14, fill: 'currentColor', className: 'block shrink-0', style: { lineHeight: 0 } };
  switch (kind) {
    case 'dir':
      return <FolderOpen {...props} />;
    case 'image':
      return <Pic {...props} />;
    case 'video':
      return <VideoOne {...props} />;
    default:
      return <FileText {...props} />;
  }
}

interface TreeRowProps {
  node: ArtifactNode;
  depth: number;
  selectedPath: string | null;
  onSelect: (path: string) => void;
}

const TreeRow: React.FC<TreeRowProps> = ({ node, depth, selectedPath, onSelect }) => {
  const [open, setOpen] = useState(depth < 1);
  const kind = guessKind(node);
  const selected = !node.is_dir && selectedPath === node.path;

  return (
    <div>
      <button
        type='button'
        className={[
          'w-full flex items-center gap-6px px-8px py-5px rd-6px text-left text-12px leading-18px cursor-pointer border-none bg-transparent',
          'transition-colors',
          selected
            ? 'bg-primary-1 text-primary-6'
            : 'text-[var(--color-text-2)] hover:bg-fill-2 hover:text-[var(--color-text-1)]',
        ].join(' ')}
        style={{ paddingLeft: 8 + depth * 12 }}
        onClick={() => {
          if (node.is_dir) {
            setOpen((v) => !v);
          } else {
            onSelect(node.path);
          }
        }}
      >
        <span className='opacity-70'>
          <NodeIcon kind={kind} />
        </span>
        <span className='truncate'>{node.name}</span>
        {node.is_dir && (
          <span className='ml-auto text-10px text-[var(--color-text-4)] shrink-0'>{open ? '▾' : '▸'}</span>
        )}
      </button>
      {node.is_dir && open && node.children?.length
        ? node.children.map((child) => (
            <TreeRow
              key={child.path}
              node={child}
              depth={depth + 1}
              selectedPath={selectedPath}
              onSelect={onSelect}
            />
          ))
        : null}
    </div>
  );
};

interface ArtifactTreeProps {
  tree: ArtifactNode[];
  selectedPath: string | null;
  onSelect: (path: string) => void;
  emptyHint?: string;
}

const ArtifactTree: React.FC<ArtifactTreeProps> = ({ tree, selectedPath, onSelect, emptyHint }) => {
  const { t } = useTranslation();
  const nodes = useMemo(() => tree, [tree]);

  if (!nodes.length) {
    return (
      <div className='px-12px py-24px text-center text-12px text-[var(--color-text-3)]'>
        {emptyHint ??
          t('videoGeneration.workspace.artifactsEmpty', {
            defaultValue: '规划完成后，产物将显示在这里。',
          })}
      </div>
    );
  }

  return (
    <div className='flex flex-col gap-1px py-4px overflow-y-auto'>
      {nodes.map((node) => (
        <TreeRow
          key={node.path}
          node={node}
          depth={0}
          selectedPath={selectedPath}
          onSelect={onSelect}
        />
      ))}
    </div>
  );
};

export default ArtifactTree;
