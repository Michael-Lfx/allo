import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import { Button, Checkbox, Drawer, Tag } from '@arco-design/web-react';
import { CheckSmall, FolderOpen, ImportAndExport, Info, Refresh } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  buildAgentSkillRows,
  defaultSelectedAgentSkillKeys,
  summarizeAgentSkillImport,
  type AgentSkillImportRow,
  type ExternalAgentSkillSource,
} from './agentSkillImportUtils';

export type ImportedAgentSkill = {
  name: string;
  description: string;
  path: string;
  source: string;
  sourceName: string;
  alreadyImported: boolean;
  skillId?: string;
};

type ImportMode = 'library' | 'preset';

type AgentSkillImportDrawerProps = {
  visible: boolean;
  onClose: () => void;
  existingSkillNames?: string[];
  onImported?: (skills: ImportedAgentSkill[]) => Promise<void> | void;
  mode?: ImportMode;
  loadSources?: () => Promise<ExternalAgentSkillSource[]>;
  importSkills?: (rows: AgentSkillImportRow[]) => Promise<ImportedAgentSkill[]>;
};

type AgentSkillImportController = {
  sources: ExternalAgentSkillSource[];
  loading: boolean;
  importing: boolean;
  rows: AgentSkillImportRow[];
  selectedRows: AgentSkillImportRow[];
  selectedKeySet: Set<string>;
  summary: ReturnType<typeof summarizeAgentSkillImport>;
  allSelected: boolean;
  fetchSources: () => Promise<void>;
  toggleRow: (row: AgentSkillImportRow) => void;
  handleSelectAll: (checked: boolean) => void;
  handleImport: () => Promise<void>;
  messageContext: React.ReactNode;
};

const sourceToneClass = (_source: string) => 'bg-fill-2 text-t-secondary border border-solid border-border-2';

const toImportedSkill = (row: AgentSkillImportRow, name = row.name, skillId?: string): ImportedAgentSkill => ({
  name,
  description: row.description,
  path: row.path,
  source: row.source,
  sourceName: row.sourceName,
  alreadyImported: row.alreadyImported,
  skillId,
});

const useAgentSkillImportController = ({
  visible,
  onClose,
  existingSkillNames = [],
  onImported,
  mode,
  loadSources,
  importSkills,
}: Omit<AgentSkillImportDrawerProps, 'mode'> & { mode: ImportMode }): AgentSkillImportController => {
  const { t } = useTranslation();
  const [message, messageContext] = useArcoMessage({ maxCount: 5 });
  const [sources, setSources] = useState<ExternalAgentSkillSource[]>([]);
  const [loading, setLoading] = useState(false);
  const [importing, setImporting] = useState(false);
  const [selectedKeys, setSelectedKeys] = useState<string[]>([]);
  const existingNames = useMemo(() => new Set(existingSkillNames), [existingSkillNames]);
  const rows = useMemo(() => buildAgentSkillRows(sources, existingNames), [sources, existingNames]);
  const selectedKeySet = useMemo(() => new Set(selectedKeys), [selectedKeys]);
  const selectedRows = useMemo(() => rows.filter((row) => selectedKeySet.has(row.key)), [rows, selectedKeySet]);
  const summary = useMemo(() => summarizeAgentSkillImport(rows, selectedRows), [rows, selectedRows]);
  const rowSignature = useMemo(() => rows.map((row) => `${row.key}:${row.alreadyImported ? '1' : '0'}`).join('|'), [rows]);

  const fetchSources = useCallback(async () => {
    setLoading(true);
    try {
      const detected = loadSources ? await loadSources() : await ipcBridge.fs.detectAndCountExternalSkills.invoke();
      setSources(detected as ExternalAgentSkillSource[]);
    } catch (error) {
      console.error('Failed to detect external agent skills:', error);
      const detail = isBackendHttpError(error) ? error.backendMessage : '';
      message.error(
        detail
          ? t('settings.agentSkillImport.detectErrorDetailed', { detail, defaultValue: `Unable to scan agent skills: ${detail}` })
          : t('settings.agentSkillImport.detectError', { defaultValue: 'Unable to scan agent skills' }),
      );
    } finally {
      setLoading(false);
    }
  }, [loadSources, message, t]);

  useEffect(() => {
    if (visible) void fetchSources();
  }, [fetchSources, visible]);

  useEffect(() => {
    if (visible) setSelectedKeys(defaultSelectedAgentSkillKeys(rows));
  }, [rowSignature, visible]);

  const toggleRow = useCallback((row: AgentSkillImportRow) => {
    setSelectedKeys((previous) => (previous.includes(row.key) ? previous.filter((key) => key !== row.key) : [...previous, row.key]));
  }, []);

  const handleSelectAll = useCallback((checked: boolean) => {
    setSelectedKeys(checked ? rows.map((row) => row.key) : []);
  }, [rows]);

  const handleImport = useCallback(async () => {
    if (selectedRows.length === 0) return;
    setImporting(true);
    try {
      const imported: ImportedAgentSkill[] = importSkills ? await importSkills(selectedRows) : [];
      if (!importSkills) {
        for (const row of selectedRows) {
          if (row.alreadyImported) {
            imported.push(toImportedSkill(row));
            continue;
          }
          const result = await ipcBridge.fs.importSkillWithSymlink.invoke({ skill_path: row.path });
          const names = result.skill_names?.length ? result.skill_names : result.skill_name ? [result.skill_name] : [row.name];
          for (const [index, name] of names.entries()) imported.push(toImportedSkill(row, name, result.skill_ids?.[index]));
        }
      }
      await onImported?.(imported);
      message.success(
        mode === 'preset'
          ? t('settings.agentSkillImport.presetSuccess', { count: imported.length, defaultValue: `Added ${imported.length} skills to this preset` })
          : t('settings.agentSkillImport.librarySuccess', { count: imported.length, defaultValue: `Imported ${imported.length} agent skills` }),
      );
      onClose();
    } catch (error) {
      console.error('Failed to import agent skills:', error);
      const detail = isBackendHttpError(error) ? error.backendMessage : '';
      message.error(
        detail
          ? t('settings.agentSkillImport.importErrorDetailed', { detail, defaultValue: `Unable to import agent skills: ${detail}` })
          : t('settings.agentSkillImport.importError', { defaultValue: 'Unable to import agent skills' }),
      );
    } finally {
      setImporting(false);
    }
  }, [importSkills, message, mode, onClose, onImported, selectedRows, t]);

  return {
    sources,
    loading,
    importing,
    rows,
    selectedRows,
    selectedKeySet,
    summary,
    allSelected: rows.length > 0 && selectedRows.length === rows.length,
    fetchSources,
    toggleRow,
    handleSelectAll,
    handleImport,
    messageContext,
  };
};

type AgentSkillImportContentProps = {
  controller: AgentSkillImportController;
};

/** Shared body used by the standalone Drawer and the embedded preset step. */
export const AgentSkillImportContent: React.FC<AgentSkillImportContentProps> = ({ controller }) => {
  const { t } = useTranslation();
  const { sources, loading, rows, selectedRows, selectedKeySet, allSelected, fetchSources, toggleRow, handleSelectAll, messageContext } = controller;
  return (
    <>
      {messageContext}
      <div className='flex flex-col gap-16px' data-testid='agent-skill-import-content'>
        <div className='flex items-start gap-10px rounded-10px bg-fill-2 p-12px'>
          <Info size={16} className='mt-2px flex-shrink-0 text-primary-6' />
          <div className='text-13px leading-20px text-t-secondary'>
            {t('settings.agentSkillImport.description', { defaultValue: 'Bring reusable skills from Claude, Gemini, Codex-compatible Agent Skills, or custom external folders into Flowy.' })}
          </div>
        </div>
        <div className='flex items-center justify-between gap-10px'>
          <div className='text-14px font-600 text-t-primary'>{t('settings.agentSkillImport.sources', { defaultValue: 'Detected sources' })}</div>
          <Button size='small' type='text' onClick={fetchSources} loading={loading} icon={<Refresh size={14} fill='currentColor' />} className='flowy-icon-text-btn !rounded-10px' data-testid='btn-refresh-agent-skills'>
            {t('common.refresh', { defaultValue: 'Refresh' })}
          </Button>
        </div>
        {sources.length > 0 && (
          <div className='grid gap-8px' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(180px, 1fr))' }}>
            {sources.map((source) => (
              <div key={`${source.source}:${source.path}`} className='min-w-0 rounded-8px bg-fill-2 p-10px'>
                <div className='flex min-w-0 items-center gap-8px'><FolderOpen size={15} className='flex-shrink-0 text-t-secondary' /><span className='truncate text-13px font-600 text-t-primary' title={source.name}>{source.name}</span></div>
                <div className='mt-6px flex items-center gap-6px'><Tag size='small' bordered={false} className={sourceToneClass(source.source)}>{source.skill_count ?? source.skills.length}</Tag><span className='truncate text-11px text-t-tertiary' title={source.path}>{source.path}</span></div>
              </div>
            ))}
          </div>
        )}
        <div className='flex items-center justify-between'>
          <Checkbox checked={allSelected} indeterminate={selectedRows.length > 0 && !allSelected} onChange={handleSelectAll}>{t('settings.agentSkillImport.selectAll', { defaultValue: 'Select all' })}</Checkbox>
          <span className='text-12px text-t-secondary'>{t('settings.agentSkillImport.count', { count: rows.length, defaultValue: `${rows.length} skills` })}</span>
        </div>
        <div className='overflow-hidden rounded-12px bg-fill-1'>
          {rows.length > 0 ? (
            <div className='max-h-[420px] divide-y divide-border-2 overflow-auto'>
              {rows.map((row) => (
                <div key={row.key} className='flex items-start gap-10px p-10px transition-colors hover:bg-fill-1' data-testid={`agent-skill-import-row-${row.source}-${row.name}`}>
                  <Checkbox checked={selectedKeySet.has(row.key)} onChange={() => toggleRow(row)} className='mt-2px' />
                  <div className='min-w-0 flex-1'>
                    <div className='flex min-w-0 items-center gap-8px'>
                      <span className='truncate text-13px font-600 text-t-primary' title={row.name}>{row.name}</span>
                      <Tag size='small' bordered={false} className={sourceToneClass(row.source)}>{row.sourceName}</Tag>
                      {row.alreadyImported && <Tag size='small' bordered={false} className='!bg-fill-2 !text-t-secondary'><span className='inline-flex items-center gap-3px'><CheckSmall size={12} fill='currentColor' />{t('settings.agentSkillImport.alreadyImported', { defaultValue: 'In library' })}</span></Tag>}
                    </div>
                    <div className='mt-3px line-clamp-2 text-12px text-t-secondary'>{row.description || t('settings.skillsHub.noDescription', { defaultValue: 'No description provided.' })}</div>
                    <div className='mt-4px truncate font-mono text-11px text-t-tertiary' title={row.path}>{row.path}</div>
                  </div>
                </div>
              ))}
            </div>
          ) : <div className='px-16px py-36px text-center text-t-secondary'>{loading ? t('common.loading', { defaultValue: 'Please wait...' }) : t('settings.agentSkillImport.empty', { defaultValue: 'No external agent skills found.' })}</div>}
        </div>
      </div>
    </>
  );
};

const AgentSkillImportFooter: React.FC<{
  controller: AgentSkillImportController;
  mode: ImportMode;
  onClose: () => void;
  closeLabel?: string;
}> = ({ controller, mode, onClose, closeLabel }) => {
  const { t } = useTranslation();
  const { summary, importing, selectedRows, handleImport } = controller;
  return (
    <div className='flex w-full items-center justify-between gap-12px'>
      <div className='text-12px text-t-secondary'>{t('settings.agentSkillImport.selectionSummary', { selected: summary.selectedCount, importable: summary.importableCount, existing: summary.alreadyImportedCount, defaultValue: `${summary.selectedCount} selected · ${summary.importableCount} new · ${summary.alreadyImportedCount} already in library` })}</div>
      <div className='flex items-center gap-8px'>
        <Button onClick={onClose} className='!h-36px !rounded-8px'>
          {closeLabel ?? t('common.cancel', { defaultValue: 'Cancel' })}
        </Button>
        <Button type='primary' loading={importing} disabled={selectedRows.length === 0} onClick={handleImport} className='flowy-icon-text-btn !h-36px !rounded-8px !whitespace-nowrap' icon={<ImportAndExport size={14} fill='currentColor' />} data-testid='btn-confirm-agent-skill-import'>
          {mode === 'preset' ? t('settings.agentSkillImport.addToPreset', { defaultValue: 'Add to preset' }) : t('settings.agentSkillImport.importSelected', { defaultValue: 'Import selected' })}
        </Button>
      </div>
    </div>
  );
};

const AgentSkillImportDrawer: React.FC<AgentSkillImportDrawerProps> = ({ visible, onClose, existingSkillNames, onImported, mode = 'library', loadSources, importSkills }) => {
  const { t } = useTranslation();
  const controller = useAgentSkillImportController({ visible, onClose, existingSkillNames, onImported, mode, loadSources, importSkills });
  return (
    <Drawer visible={visible} onCancel={onClose} width={680} zIndex={1300} placement='right' title={t('settings.agentSkillImport.title', { defaultValue: 'Import from Agent' })} className='agent-skill-import-drawer' data-testid='agent-skill-import-drawer' footer={<AgentSkillImportFooter controller={controller} mode={mode} onClose={onClose} />}>
      <AgentSkillImportContent controller={controller} />
    </Drawer>
  );
};

/** Embedded preset step; it has no second Drawer and keeps the parent draft alive. */
export const AgentSkillImportEmbedded: React.FC<AgentSkillImportDrawerProps & { closeLabel?: string }> = (props) => {
  const mode = props.mode ?? 'preset';
  const controller = useAgentSkillImportController({ ...props, mode });
  return (
    <section className='flex min-h-0 flex-1 flex-col' data-testid='agent-skill-import-embedded'>
      <div className='min-h-0 flex-1 pr-2px'><AgentSkillImportContent controller={controller} /></div>
      <div className='mt-16px border-t border-t-solid border-arco-2 pt-12px'>
        <AgentSkillImportFooter controller={controller} mode={mode} onClose={props.onClose} closeLabel={props.closeLabel} />
      </div>
    </section>
  );
};

export default AgentSkillImportDrawer;
