/**
 * Editable JSON artifact panel for Montage Backlot workspace.
 */
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input, Spin } from '@arco-design/web-react';
import { putArtifact } from '../api';
import type { ArtifactContent } from '../types';
import { extractMediaPaths } from '../stageI18n';

const TextArea = Input.TextArea;

export interface ArtifactPreviewPanelProps {
  projectId: string;
  artifactName: string | null;
  preview: ArtifactContent | null;
  previewLoading: boolean;
  disabled?: boolean;
  onChanged: () => void;
}

const ArtifactPreviewPanel: React.FC<ArtifactPreviewPanelProps> = ({
  projectId,
  artifactName,
  preview,
  previewLoading,
  disabled,
  onChanged,
}) => {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);
  const [parseError, setParseError] = useState<string | null>(null);

  useEffect(() => {
    setEditing(false);
    setDraft(preview?.text ?? '');
    setParseError(null);
  }, [artifactName, preview?.text]);

  const mediaPaths = (() => {
    if (!preview?.text) return [] as string[];
    try {
      return [...new Set(extractMediaPaths(JSON.parse(preview.text)))].slice(0, 24);
    } catch {
      return [] as string[];
    }
  })();

  const handleSave = async () => {
    if (!artifactName || saving) return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(draft);
      setParseError(null);
    } catch (e) {
      setParseError(e instanceof Error ? e.message : String(e));
      return;
    }
    setSaving(true);
    try {
      await putArtifact(projectId, artifactName, parsed);
      setEditing(false);
      onChanged();
    } catch (e) {
      setParseError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  if (!artifactName) {
    return (
      <div className='flex flex-1 items-center justify-center p-24px text-13px text-[var(--color-text-3)]'>
        {t('videoGeneration.artifacts.empty', {
          defaultValue: '选择一个阶段产物以查看或编辑。',
        })}
      </div>
    );
  }

  if (previewLoading) {
    return (
      <div className='flex flex-1 items-center justify-center p-24px'>
        <Spin />
      </div>
    );
  }

  return (
    <div className='flex min-h-0 flex-1 flex-col gap-12px'>
      <div className='flex items-center justify-between gap-8px'>
        <div className='truncate text-13px font-600 text-[var(--color-text-1)]'>{artifactName}</div>
        <div className='flex shrink-0 gap-8px'>
          {editing ? (
            <>
              <Button size='mini' onClick={() => setEditing(false)} disabled={saving}>
                {t('common.cancel', { defaultValue: '取消' })}
              </Button>
              <Button size='mini' type='primary' loading={saving} disabled={disabled} onClick={() => void handleSave()}>
                {t('videoGeneration.artifacts.save', { defaultValue: '保存' })}
              </Button>
            </>
          ) : (
            <Button size='mini' type='outline' disabled={disabled || !preview?.text} onClick={() => setEditing(true)}>
              {t('videoGeneration.artifacts.edit', { defaultValue: '编辑' })}
            </Button>
          )}
        </div>
      </div>

      {parseError ? (
        <div className='rd-8px bg-[rgba(var(--danger-6),0.08)] px-10px py-8px text-12px text-[rgb(var(--danger-6))]'>
          {parseError}
        </div>
      ) : null}

      {mediaPaths.length > 0 ? (
        <div className='flex flex-col gap-6px'>
          <div className='text-12px font-600 text-[var(--color-text-2)]'>
            {t('videoGeneration.artifacts.mediaStrip', { defaultValue: '媒体路径' })}
          </div>
          <div className='flex flex-wrap gap-6px'>
            {mediaPaths.map((path) => (
              <span
                key={path}
                className='max-w-full truncate rd-6px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-8px py-4px text-11px text-[var(--color-text-3)]'
                title={path}
              >
                {path}
              </span>
            ))}
          </div>
        </div>
      ) : null}

      {editing ? (
        <TextArea
          className='min-h-280px flex-1 font-mono text-12px'
          value={draft}
          onChange={setDraft}
          disabled={saving || disabled}
        />
      ) : (
        <pre className='m-0 min-h-280px flex-1 overflow-auto rd-10px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] p-12px text-12px leading-[1.5] text-[var(--color-text-2)] whitespace-pre-wrap break-words'>
          {preview?.text ||
            t('videoGeneration.artifacts.missing', { defaultValue: '该产物尚未生成。' })}
        </pre>
      )}
    </div>
  );
};

export default ArtifactPreviewPanel;
