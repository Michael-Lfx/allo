import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import { Button, Dropdown, Menu } from '@arco-design/web-react';
import { FileZip, FolderOpen } from '@icon-park/react';
import React, { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';
import AgentSkillImportDrawer from './AgentSkillImportDrawer';
import type { ExternalAgentSkillSource } from './agentSkillImportUtils';

type SkillImportMenuProps = {
  onImported?: () => void;
};

const SkillImportMenu: React.FC<SkillImportMenuProps> = ({ onImported }) => {
  const { t } = useTranslation();
  const [message, messageContext] = useArcoMessage({ maxCount: 10 });
  const [agentImportVisible, setAgentImportVisible] = useState(false);
  const [existingSkillNames, setExistingSkillNames] = useState<string[]>([]);

  const refreshExistingNames = useCallback(async () => {
    try {
      const skills = await ipcBridge.fs.listAvailableSkills.invoke();
      setExistingSkillNames(skills.map((skill) => skill.name));
    } catch (error) {
      console.error('Failed to list skills for import:', error);
    }
  }, []);

  const openAgentImport = useCallback(() => {
    setAgentImportVisible(true);
    void refreshExistingNames();
  }, [refreshExistingNames]);

  const handleImport = async (skillPath: string) => {
    try {
      const result = await ipcBridge.fs.importSkillWithSymlink.invoke({ skill_path: skillPath });
      const importedNames = result.skill_names?.length
        ? result.skill_names
        : result.skill_name
          ? [result.skill_name]
          : [];
      const count = importedNames.length;
      const names = importedNames.join(', ');
      message.success(
        t('settings.skillsHub.importSuccessDetailed', {
          count,
          names,
          defaultValue: count > 1 ? `Imported ${count} skills: ${names}` : `Imported skill: ${names}`,
        })
      );
      await refreshExistingNames();
      onImported?.();
    } catch (error) {
      console.error('Failed to import skill:', error);
      const detail = isBackendHttpError(error) ? error.backendMessage : '';
      message.error(
        detail
          ? t('settings.skillsHub.importErrorDetailed', { detail, defaultValue: `Error importing skill: ${detail}` })
          : t('settings.skillsHub.importError', { defaultValue: 'Error importing skill' })
      );
    }
  };

  const handleImportFolder = async () => {
    try {
      const result = await ipcBridge.dialog.showOpen.invoke({ properties: ['openDirectory'] });
      if (result && result.length > 0) await handleImport(result[0]);
    } catch (error) {
      console.error('Failed to open folder dialog:', error);
    }
  };

  const handleImportZip = async () => {
    try {
      const result = await ipcBridge.dialog.showOpen.invoke({
        properties: ['openFile'],
        filters: [{ name: 'Skill zip archives', extensions: ['zip'] }],
      });
      if (result && result.length > 0) await handleImport(result[0]);
    } catch (error) {
      console.error('Failed to open zip dialog:', error);
    }
  };

  const loadAgentSkillSources = useCallback(async (): Promise<ExternalAgentSkillSource[]> => {
    return (await ipcBridge.fs.detectAndCountExternalSkills.invoke()) as ExternalAgentSkillSource[];
  }, []);

  return (
    <>
      {messageContext}
      <Dropdown
        trigger='click'
        droplist={
          <Menu>
            <Menu.Item key='agent' data-testid='btn-import-agent-skills' onClick={openAgentImport}>
              <FolderOpen size={14} fill='currentColor' /> {t('settings.agentSkillImport.shortAction', { defaultValue: 'Import from Agent' })}
            </Menu.Item>
            <Menu.Item key='folder' data-testid='btn-manual-import' onClick={() => void handleImportFolder()}>
              <FolderOpen size={14} fill='currentColor' /> {t('settings.skillsHub.manualImport', { defaultValue: 'Import Skills' })}
            </Menu.Item>
            <Menu.Item key='zip' data-testid='btn-import-zip' onClick={() => void handleImportZip()}>
              <FileZip size={14} fill='currentColor' /> {t('settings.skillsHub.importZip', { defaultValue: 'Import .zip' })}
            </Menu.Item>
          </Menu>
        }
      >
        <Button
          size='small'
          type='outline'
          data-testid='btn-import-skills'
          className='flowy-icon-text-btn capability-hub-action-btn'
          icon={<FolderOpen size={14} fill='currentColor' />}
        >
          {t('settings.skillsHub.manualImport', { defaultValue: 'Import Skills' })}
        </Button>
      </Dropdown>
      <AgentSkillImportDrawer
        visible={agentImportVisible}
        onClose={() => setAgentImportVisible(false)}
        existingSkillNames={existingSkillNames}
        onImported={async () => {
          await refreshExistingNames();
          onImported?.();
        }}
        loadSources={loadAgentSkillSources}
      />
    </>
  );
};

export default SkillImportMenu;
