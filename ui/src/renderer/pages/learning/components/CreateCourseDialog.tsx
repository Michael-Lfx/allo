import { Input, Modal, Radio, Select, Tabs, Typography } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import LearningModelSelector, {
  type LearningModelChoice,
} from './LearningModelSelector';

const { Text, Paragraph } = Typography;

/** 创建课程对话框：方式一（从知识库生成）/ 方式二（描述生成 → 跳转 AI 对话自动创建） */
export function CreateCourseDialog({
  visible,
  busy,
  knowledgeLoading,
  knowledgeBases,
  allKnowledgeBases,
  selectedKnowledgeBaseId,
  generationDomain,
  modelChoice,
  creationTab,
  creationDescription,
  creationBaseMode,
  creationBaseId,
  onClose,
  onOk,
  onSelectedBaseChange,
  onDomainChange,
  onModelChange,
  onTabChange,
  onDescriptionChange,
  onBaseModeChange,
  onCreationBaseIdChange,
}: {
  visible: boolean;
  busy: boolean;
  knowledgeLoading: boolean;
  knowledgeBases: IKnowledgeBase[];
  allKnowledgeBases: IKnowledgeBase[];
  selectedKnowledgeBaseId?: string;
  generationDomain: string;
  modelChoice: LearningModelChoice;
  creationTab: 'base' | 'description';
  creationDescription: string;
  creationBaseMode: 'existing' | 'auto';
  creationBaseId?: string;
  onClose: () => void;
  onOk: () => void;
  onSelectedBaseChange: (value: string) => void;
  onDomainChange: (value: string) => void;
  onModelChange: (choice: LearningModelChoice) => void;
  onTabChange: (tab: 'base' | 'description') => void;
  onDescriptionChange: (value: string) => void;
  onBaseModeChange: (mode: 'existing' | 'auto') => void;
  onCreationBaseIdChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <Modal
      title={t('learning.createTitle')}
      visible={visible}
      confirmLoading={busy}
      onCancel={onClose}
      onOk={onOk}
      style={{ width: 560 }}
    >
      <Tabs activeTab={creationTab} onChange={(key) => onTabChange(key as 'base' | 'description')}>
        <Tabs.TabPane key='base' title={t('learning.generateFromBase')} destroyOnHide={false}>
          <Paragraph className='mt-0 text-t-secondary'>{t('learning.generateHint')}</Paragraph>
          <div className='flex flex-col gap-16px'>
            <div>
              <div className='mb-6px font-500'>{t('learning.knowledgeBase')}</div>
              <Select
                className='w-full'
                loading={knowledgeLoading}
                value={selectedKnowledgeBaseId}
                placeholder={t('learning.selectKnowledgeBase')}
                onChange={(value: string) => onSelectedBaseChange(value)}
              >
                {knowledgeBases.map((base) => (
                  <Select.Option key={base.knowledge_base_id} value={base.knowledge_base_id}>
                    {base.name} ({base.file_count} {t('learning.files')})
                  </Select.Option>
                ))}
              </Select>
              {!knowledgeLoading && knowledgeBases.length === 0 && (
                <Text type='secondary'>{t('learning.noUsableKnowledgeBases')}</Text>
              )}
            </div>
            <div>
              <div className='mb-6px font-500'>{t('learning.domain')}</div>
              <Input
                value={generationDomain}
                placeholder={t('learning.domainPlaceholder')}
                onChange={onDomainChange}
              />
            </div>
            <div className='flex items-center justify-between'>
              <Text>{t('learning.model')}</Text>
              <LearningModelSelector
                choice={modelChoice}
                disabled={busy}
                onChange={onModelChange}
                size='small'
              />
            </div>
          </div>
        </Tabs.TabPane>
        <Tabs.TabPane key='description' title={t('learning.generateFromDescription')} destroyOnHide={false}>
          <div className='flex flex-col gap-16px'>
            <div>
              <div className='mb-6px font-500'>{t('learning.courseDescription')}</div>
              <Input.TextArea
                value={creationDescription}
                placeholder={t('learning.courseDescriptionPlaceholder')}
                onChange={onDescriptionChange}
                autoSize={{ minRows: 3, maxRows: 6 }}
              />
            </div>
            <div>
              <div className='mb-6px font-500'>{t('learning.knowledgeSource')}</div>
              <Radio.Group
                value={creationBaseMode}
                onChange={(value) => onBaseModeChange(value as 'existing' | 'auto')}
              >
                <Radio value='existing'>{t('learning.useExistingBase')}</Radio>
                <Radio value='auto'>{t('learning.autoCreateBase')}</Radio>
              </Radio.Group>
            </div>
            {creationBaseMode === 'existing' && (
              <div>
                <Select
                  className='w-full'
                  loading={knowledgeLoading}
                  value={creationBaseId}
                  placeholder={t('learning.selectKnowledgeBase')}
                  onChange={(value: string) => onCreationBaseIdChange(value)}
                >
                  {allKnowledgeBases.map((base) => (
                    <Select.Option key={base.knowledge_base_id} value={base.knowledge_base_id}>
                      {base.name} ({base.file_count} {t('learning.files')})
                    </Select.Option>
                  ))}
                </Select>
                {!knowledgeLoading && allKnowledgeBases.length === 0 && (
                  <Text type='secondary'>{t('learning.noKnowledgeBases')}</Text>
                )}
              </div>
            )}
            <Paragraph className='mt-0 text-t-secondary'>{t('learning.createViaAgentHint')}</Paragraph>
          </div>
        </Tabs.TabPane>
      </Tabs>
    </Modal>
  );
}
