/**
 * Skill detail — centered modal for the canonical SKILL.md.
 * Metadata stays visible above the document, while Preview / Source modes let
 * people understand the skill or inspect its exact file without leaving Nomi.
 */
import { ipcBridge } from '@/common';
import type { PresetTag } from '@/common/types/agent/presetTypes';
import LazyMarkdownView from '@/renderer/components/Markdown/LazyMarkdownView';
import type { SkillInfo } from '@/renderer/pages/settings/PresetSettings/types';
import { Message, Modal, Spin, Switch } from '@arco-design/web-react';
import { Code, Delete, FileText, FolderOpen, Refresh, SettingOne } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import SkillAvatar from './SkillAvatar';
import { resolveSkillDisplay } from './skillDisplay';
import { readSkillContent, stripSkillFrontmatter } from './skillDetail';

type SkillDetailDrawerProps = {
  visible: boolean;
  skill: SkillInfo | null;
  tagByKey: Map<string, PresetTag>;
  localeKey: string;
  isAutoInjected: boolean;
  onClose: () => void;
  onEditTags: (skill: SkillInfo) => void;
  onDelete: (skill: SkillInfo) => void;
};

type ViewMode = 'preview' | 'source';

const SkillDetailDrawer: React.FC<SkillDetailDrawerProps> = ({
  visible,
  skill,
  tagByKey,
  localeKey,
  isAutoInjected,
  onClose,
  onEditTags,
  onDelete,
}) => {
  const { t } = useTranslation();
  const [content, setContent] = useState('');
  const [loading, setLoading] = useState(false);
  const [loadFailed, setLoadFailed] = useState(false);
  const [viewMode, setViewMode] = useState<ViewMode>('preview');
  const [allowModelAuto, setAllowModelAuto] = useState(true);
  const [savingInvocation, setSavingInvocation] = useState(false);
  const [catalogSkillId, setCatalogSkillId] = useState<string | null>(null);
  const requestIdRef = useRef(0);

  const loadContent = useCallback(async () => {
    if (!visible || !skill) return;
    const requestId = ++requestIdRef.current;
    setLoading(true);
    setLoadFailed(false);
    try {
      const nextContent = await readSkillContent(skill, {
        readBuiltinSkill: (relativeLocation) =>
          ipcBridge.fs.readBuiltinSkill.invoke({ file_name: relativeLocation }),
        readFile: (location) => ipcBridge.fs.readFile.invoke({ path: location }),
      });
      if (requestId !== requestIdRef.current) return;
      setContent(nextContent);
    } catch (error) {
      if (requestId !== requestIdRef.current) return;
      console.error('Failed to read skill content:', error);
      setContent('');
      setLoadFailed(true);
    } finally {
      if (requestId === requestIdRef.current) setLoading(false);
    }
  }, [skill, visible]);

  useEffect(() => {
    if (!visible || !skill) return;
    setContent('');
    setViewMode('preview');
    setCatalogSkillId(null);
    setAllowModelAuto(true);
    void loadContent();
  }, [loadContent, skill, visible]);

  useEffect(() => {
    if (!visible || !skill) return;
    let cancelled = false;
    const sourceHint =
      skill.source === 'builtin' ? 'builtin' : skill.source === 'extension' ? 'extension' : 'user';
    void (async () => {
      try {
        const [catalog, overrides] = await Promise.all([
          ipcBridge.fs.listSkillCatalog.invoke(),
          ipcBridge.fs.listSkillInvocation.invoke(),
        ]);
        if (cancelled) return;
        const match =
          catalog.skills.find(
            (entry) => entry.name === skill.name && entry.source === sourceHint
          ) ?? catalog.skills.find((entry) => entry.name === skill.name);
        const skillId = match?.skill_id ?? null;
        setCatalogSkillId(skillId);
        const override = skillId
          ? overrides.find((row) => row.skill_id === skillId)
          : undefined;
        if (override) {
          setAllowModelAuto(override.allow_model_auto !== 0);
          return;
        }
        const body = await readSkillContent(skill, {
          readBuiltinSkill: (relativeLocation) =>
            ipcBridge.fs.readBuiltinSkill.invoke({ file_name: relativeLocation }),
          readFile: (location) => ipcBridge.fs.readFile.invoke({ path: location }),
        });
        if (cancelled) return;
        const slashOnly =
          /disable-model-invocation:\s*true/i.test(body) ||
          /invocation-policy:\s*\/invoke/i.test(body);
        setAllowModelAuto(!slashOnly);
      } catch (error) {
        console.warn('[skills] failed to load invocation policy', error);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [skill, visible]);

  const display = skill ? resolveSkillDisplay(skill, localeKey) : null;
  const previewContent = useMemo(() => stripSkillFrontmatter(content).trim(), [content]);
  const resolveTags = (keys: string[] | undefined) =>
    (keys ?? []).map((key) => tagByKey.get(key)).filter((tag): tag is PresetTag => Boolean(tag));
  const scenarioTags = resolveTags(skill?.scenario_tags);

  const sourceLabel = isAutoInjected
    ? t('settings.skillsHub.sourceAuto', { defaultValue: 'Auto' })
    : skill?.source === 'custom'
      ? t('settings.skillsHub.custom', { defaultValue: 'Custom' })
      : skill?.source === 'extension'
        ? t('settings.skillsHub.sourceExtension', { defaultValue: 'Extension' })
        : t('settings.skillsHub.builtin', { defaultValue: 'Built-in' });
  const sourceBadgeClass = isAutoInjected
    ? 'bg-[rgba(var(--success-6),0.1)] text-[rgb(var(--success-6))]'
    : skill?.source === 'custom'
      ? 'bg-[rgba(var(--orange-6),0.1)] text-[rgb(var(--orange-6))]'
      : skill?.source === 'extension'
        ? 'bg-fill-2 text-t-secondary'
        : 'bg-primary-1 text-primary-6';

  const renderTagGroup = (label: string, values: PresetTag[]) => (
    <div className='flex min-w-0 items-start gap-10px'>
      <span className='w-72px flex-shrink-0 pt-2px text-11px font-600 uppercase tracking-[0.08em] text-t-tertiary'>
        {label}
      </span>
      <div className='flex min-w-0 flex-1 flex-wrap gap-6px'>
        {values.length > 0 ? (
          values.map((tag) => (
            <span
              key={tag.key}
              className='inline-flex rounded-[12px] border border-solid border-[var(--color-border-2)] bg-fill-2 px-8px py-1px text-11px leading-16px text-t-secondary'
            >
              {tag.label_i18n?.[localeKey] || tag.label}
            </span>
          ))
        ) : (
          <span className='pt-1px text-12px text-t-tertiary'>
            {t('settings.skillsHub.detailNoTags', { defaultValue: 'No tags' })}
          </span>
        )}
      </div>
    </div>
  );

  const themeButtonClass =
    'inline-flex cursor-pointer items-center gap-6px rounded-full border-none bg-[var(--color-text-1)] px-16px py-6px text-13px font-semibold leading-20px text-[var(--color-bg-1)]';
  const secondaryButtonClass =
    'inline-flex cursor-pointer items-center gap-6px rounded-full border-none bg-fill-2 px-16px py-6px text-13px font-semibold leading-20px text-t-primary';

  return (
    <Modal
      visible={visible}
      unmountOnExit
      onCancel={onClose}
      autoFocus={false}
      title={t('settings.skillsHub.detailTitle', { defaultValue: 'Skill Details' })}
      className='skill-detail-modal'
      data-testid='skill-detail-modal'
      style={{ width: 760, maxWidth: '92vw', borderRadius: 16 }}
      footer={
        <div className='flex w-full items-center justify-between gap-12px'>
          {skill?.source === 'custom' ? (
            <button
              type='button'
              onClick={() => onDelete(skill)}
              className='inline-flex cursor-pointer items-center gap-6px border-none bg-transparent px-8px py-6px text-13px font-semibold text-t-tertiary hover:text-[rgb(var(--danger-6))]'
              data-testid='btn-detail-delete'
            >
              <Delete theme='outline' size={14} fill='currentColor' />
              {t('common.delete', { defaultValue: 'Delete' })}
            </button>
          ) : (
            <span />
          )}
          <div className='flex items-center gap-12px'>
          <button type='button' onClick={onClose} className={secondaryButtonClass}>
            {t('common.close', { defaultValue: 'Close' })}
          </button>
          {skill ? (
            <button
              type='button'
              onClick={() => onEditTags(skill)}
              className={themeButtonClass}
              data-testid='btn-detail-edit-tags'
            >
              <SettingOne theme='outline' size={14} fill='currentColor' strokeWidth={3} />
              {t('settings.skillsHub.editTags', { defaultValue: 'Edit Tags' })}
            </button>
          ) : null}
          </div>
        </div>
      }
    >
      {skill && display && (
        <div className='flex flex-col' data-testid='skill-detail-content'>
          <div className='flex-shrink-0 border-b border-b-solid border-[var(--color-border-1)] px-24px py-20px'>
            <div className='flex items-start gap-14px'>
              <SkillAvatar skill={skill} isAutoInjected={isAutoInjected} size={44} radiusClassName='rounded-12px' />
              <div className='min-w-0 flex-1'>
                <div className='flex flex-wrap items-center gap-8px'>
                  <h2 className='m-0 break-words text-20px font-700 leading-28px text-t-primary'>{display.name}</h2>
                  <span className={`rounded-[10px] px-8px py-2px text-11px font-600 ${sourceBadgeClass}`}>
                    {sourceLabel}
                  </span>
                </div>
                <p className='mb-0 mt-6px text-13px leading-20px text-t-secondary'>
                  {display.description || t('settings.skillsHub.noDescription', { defaultValue: 'No description provided.' })}
                </p>
                {catalogSkillId && (
                  <div
                    className='mt-10px flex items-center justify-between gap-12px rounded-12px bg-fill-2 px-12px py-8px'
                    onClick={(event) => event.stopPropagation()}
                  >
                    <div className='min-w-0'>
                      <div className='text-12px font-600 text-t-primary'>
                        {t('settings.skillsHub.allowModelAuto', { defaultValue: 'Allow model auto-invoke' })}
                      </div>
                      <div className='mt-2px text-11px text-t-tertiary'>
                        {t('settings.skillsHub.allowModelAutoHint', {
                          defaultValue: 'When on, the agent may load this skill from its description without a / selection.',
                        })}
                      </div>
                    </div>
                    <Switch
                      size='small'
                      checked={allowModelAuto}
                      loading={savingInvocation}
                      data-testid='skill-allow-model-auto'
                      onChange={async (checked) => {
                        setSavingInvocation(true);
                        try {
                          await ipcBridge.fs.setSkillInvocation.invoke({
                            skill_id: catalogSkillId,
                            allow_model_auto: checked,
                          });
                          setAllowModelAuto(checked);
                          Message.success(
                            t('settings.skillsHub.invocationSaved', { defaultValue: 'Invocation setting saved' })
                          );
                        } catch (error) {
                          console.error('Failed to save skill invocation policy', error);
                          Message.error(
                            t('settings.skillsHub.invocationSaveFailed', {
                              defaultValue: 'Failed to save invocation setting',
                            })
                          );
                        } finally {
                          setSavingInvocation(false);
                        }
                      }}
                    />
                  </div>
                )}
              </div>
            </div>

            <div className='mt-18px flex flex-col gap-10px rounded-12px bg-fill-2 p-12px'>
              {renderTagGroup(t('settings.presetTagScenario', { defaultValue: 'Skill Scenario' }), scenarioTags)}
              <div className='flex min-w-0 items-start gap-10px pt-10px'>
                <span className='w-72px flex-shrink-0 pt-2px text-11px font-600 uppercase tracking-[0.08em] text-t-tertiary'>
                  {t('settings.skillsHub.detailLocation', { defaultValue: 'Location' })}
                </span>
                <div className='flex min-w-0 flex-1 items-center gap-6px'>
                  <FolderOpen size={13} fill='currentColor' className='flex-shrink-0 text-t-tertiary' />
                  <code className='min-w-0 break-all text-11px leading-17px text-t-secondary' title={skill.location}>
                    {skill.location}
                  </code>
                </div>
              </div>
            </div>
          </div>

          <div className='flex flex-col px-24px py-18px'>
            <div className='mb-12px flex flex-shrink-0 items-center justify-between gap-12px'>
              <div>
                <div className='text-13px font-700 text-t-primary'>
                  {t('settings.skillsHub.detailInstructions', { defaultValue: 'Instructions' })}
                </div>
                <div className='mt-2px text-11px text-t-tertiary'>SKILL.md</div>
              </div>
              <div className='flex rounded-[100px] bg-fill-2 p-3px' role='tablist'>
                {([
                  ['preview', t('settings.skillsHub.detailPreview', { defaultValue: 'Preview' }), FileText],
                  ['source', t('settings.skillsHub.detailSource', { defaultValue: 'Source' }), Code],
                ] as const).map(([mode, label, Icon]) => (
                  <div
                    key={mode}
                    role='tab'
                    tabIndex={0}
                    aria-selected={viewMode === mode}
                    onClick={() => setViewMode(mode)}
                    onKeyDown={(event) => {
                      if (event.key === 'Enter' || event.key === ' ') {
                        event.preventDefault();
                        setViewMode(mode);
                      }
                    }}
                    className={[
                      'flex cursor-pointer items-center gap-5px rounded-[100px] px-10px py-5px text-11px font-600 transition-colors',
                      viewMode === mode ? 'bg-base text-t-primary shadow-sm' : 'text-t-tertiary hover:text-t-secondary',
                    ].join(' ')}
                  >
                    <Icon size={12} />
                    {label}
                  </div>
                ))}
              </div>
            </div>

            <div className='max-h-[42vh] min-h-180px overflow-auto rounded-14px border border-solid border-[var(--color-border-1)] bg-base p-18px'>
              {loading ? (
                <div className='flex h-full min-h-180px items-center justify-center' data-testid='skill-detail-loading'>
                  <Spin size={24} />
                </div>
              ) : loadFailed ? (
                <div
                  className='flex h-full min-h-180px flex-col items-center justify-center gap-10px text-center'
                  data-testid='skill-detail-error'
                >
                  <div className='text-13px font-600 text-t-primary'>
                    {t('settings.skillsHub.detailLoadFailed', { defaultValue: 'Could not load this skill file.' })}
                  </div>
                  <div className='max-w-360px text-12px leading-18px text-t-tertiary'>
                    {t('settings.skillsHub.detailLoadFailedHint', {
                      defaultValue: 'The file may have moved or is no longer readable. Refresh the skill list and try again.',
                    })}
                  </div>
                  <button
                    type='button'
                    className='inline-flex cursor-pointer items-center gap-6px rounded-full border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-1)] px-16px py-6px text-13px font-semibold leading-20px text-t-primary'
                    onClick={() => void loadContent()}
                  >
                    <Refresh theme='outline' size={13} fill='currentColor' />
                    {t('common.retry', { defaultValue: 'Retry' })}
                  </button>
                </div>
              ) : viewMode === 'source' ? (
                <pre
                  className='m-0 min-w-max whitespace-pre font-mono text-12px leading-19px text-t-secondary'
                  data-testid='skill-detail-source'
                >
                  {content}
                </pre>
              ) : previewContent ? (
                <LazyMarkdownView hiddenCodeCopyButton className='text-13px' fontSize='13px' lineHeight='1.65'>
                  {previewContent}
                </LazyMarkdownView>
              ) : (
                <div className='flex min-h-180px items-center justify-center text-12px text-t-tertiary'>
                  {t('settings.skillsHub.detailEmpty', { defaultValue: 'This skill has no instructions yet.' })}
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </Modal>
  );
};

export default SkillDetailDrawer;
