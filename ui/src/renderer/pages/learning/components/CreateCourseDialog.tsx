import { Input, Modal, Radio, Select, Tabs, Typography } from '@arco-design/web-react';
import { IconCheckCircleFill, IconFile, IconThunderbolt } from '@arco-design/web-react/icon';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import LearningModelSelector, {
  type LearningModelChoice,
} from './LearningModelSelector';

const { Text, Paragraph } = Typography;

/** 生成方式选项卡片：图标 + 标题 + 描述，选中时主色描边并显示对勾 */
function GenerationModeCard({
  selected,
  icon,
  title,
  badge,
  desc,
  onSelect,
}: {
  selected: boolean;
  icon: ReactNode;
  title: string;
  badge?: string;
  desc: string;
  onSelect: () => void;
}) {
  return (
    <button
      type='button'
      role='radio'
      aria-checked={selected}
      onClick={onSelect}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onSelect();
        }
      }}
      className={[
        'relative flex cursor-pointer flex-col items-start gap-6px rounded-10px border border-solid p-12px text-left transition-colors focus-visible:outline-none focus-visible:shadow-[0_0_0_3px_rgba(var(--primary-6),0.15)]',
        selected
          ? 'border-[rgb(var(--primary-6))] bg-[rgba(var(--primary-6),0.06)]'
          : 'border-[var(--color-border-2)] bg-[var(--color-fill-1)] hover:border-[rgba(var(--primary-6),0.4)] hover:bg-[rgba(var(--primary-6),0.03)]',
      ].join(' ')}
    >
      <span
        className={[
          'grid h-26px w-26px place-items-center rounded-8px transition-colors',
          selected
            ? 'bg-[rgba(var(--primary-6),0.12)] text-[rgb(var(--primary-6))]'
            : 'bg-[var(--color-fill-2)] text-[var(--color-text-2)]',
        ].join(' ')}
        aria-hidden='true'
      >
        {icon}
      </span>
      <span className='flex items-center gap-6px'>
        <span className='text-13px font-600 text-[var(--color-text-1)]'>{title}</span>
        {badge && (
          <span className='rounded-full bg-[rgba(var(--primary-6),0.08)] px-6px py-1px text-10px font-500 text-[rgb(var(--primary-6))]'>
            {badge}
          </span>
        )}
      </span>
      <span className='text-12px leading-relaxed text-[var(--color-text-3)]'>{desc}</span>
      {selected && (
        <IconCheckCircleFill
          fontSize={16}
          className='absolute right-10px top-10px text-[rgb(var(--primary-6))]'
          aria-hidden='true'
        />
      )}
    </button>
  );
}

/** 创建课程对话框：方式一（从知识库生成）/ 方式二（描述生成 → 跳转 AI 对话自动创建） */
export function CreateCourseDialog({
  visible,
  busy,
  knowledgeLoading,
  knowledgeBases,
  allKnowledgeBases,
  selectedKnowledgeBaseId,
  generationDomain,
  generationMode,
  modelChoice,
  creationTab,
  creationDescription,
  creationBaseMode,
  creationBaseId,
  onClose,
  onOk,
  onSelectedBaseChange,
  onDomainChange,
  onGenerationModeChange,
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
  generationMode: 'full' | 'on_demand';
  modelChoice: LearningModelChoice;
  creationTab: 'base' | 'description';
  creationDescription: string;
  creationBaseMode: 'existing' | 'auto';
  creationBaseId?: string;
  onClose: () => void;
  onOk: () => void;
  onSelectedBaseChange: (value: string) => void;
  onDomainChange: (value: string) => void;
  onGenerationModeChange: (mode: 'full' | 'on_demand') => void;
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
      <div className='mt-20px flex flex-col gap-16px'>
        <div>
          <div className='mb-8px font-500'>{t('learning.generationMode')}</div>
          <div className='grid grid-cols-2 gap-10px' role='radiogroup' aria-label={t('learning.generationMode')}>
            <GenerationModeCard
              selected={generationMode === 'on_demand'}
              icon={<IconThunderbolt fontSize={16} />}
              title={t('learning.generationModeOnDemand')}
              badge={t('learning.generationModeRecommended')}
              desc={t('learning.generationModeOnDemandDesc')}
              onSelect={() => onGenerationModeChange('on_demand')}
            />
            <GenerationModeCard
              selected={generationMode === 'full'}
              icon={<IconFile fontSize={16} />}
              title={t('learning.generationModeFull')}
              desc={t('learning.generationModeFullDesc')}
              onSelect={() => onGenerationModeChange('full')}
            />
          </div>
        </div>
        <div>
          <div className='mb-8px font-500'>{t('learning.model')}</div>
          <LearningModelSelector
            choice={modelChoice}
            disabled={busy}
            onChange={onModelChange}
            size='small'
          />
        </div>
      </div>
    </Modal>
  );
}
