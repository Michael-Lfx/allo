import { useTranslation } from 'react-i18next';
import { Button, Input, Modal, Select, Tabs, Tag, Typography } from '@arco-design/web-react';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import type { CourseGenerationState } from '../hooks/useCourseCreation';
import LearningModelSelector, {
  type LearningModelChoice,
} from './LearningModelSelector';
import { CourseGenerationProgress } from './CourseGenerationProgress';

const { Text, Paragraph } = Typography;

/** 创建课程对话框：方式一（从知识库生成）/ 方式二（描述直接生成，无知识库参与）。
 * 提交后就地切换为生成视图：agent loop 同步执行需要一些时间，过程事件实时
 * 展示；关闭对话框不会终止生成（后台继续，悬浮指示条可回到这里）。 */
export function CreateCourseDialog({
  visible,
  busy,
  knowledgeLoading,
  knowledgeBases,
  selectedKnowledgeBaseId,
  generationDomain,
  modelChoice,
  creationTab,
  creationDescription,
  generation,
  onClose,
  onOk,
  onSelectedBaseChange,
  onDomainChange,
  onModelChange,
  onTabChange,
  onDescriptionChange,
  onRetry,
  onCancel,
  onStartLearning,
}: {
  visible: boolean;
  busy: boolean;
  knowledgeLoading: boolean;
  knowledgeBases: IKnowledgeBase[];
  selectedKnowledgeBaseId?: string;
  generationDomain: string;
  modelChoice: LearningModelChoice;
  creationTab: 'base' | 'description' | 'graph';
  creationDescription: string;
  generation: CourseGenerationState | null;
  onClose: () => void;
  onOk: () => void;
  onSelectedBaseChange: (value: string) => void;
  onDomainChange: (value: string) => void;
  onModelChange: (choice: LearningModelChoice) => void;
  onTabChange: (tab: 'base' | 'description' | 'graph') => void;
  onDescriptionChange: (value: string) => void;
  onRetry: () => void;
  onCancel: () => void;
  onStartLearning: (courseId: string) => void;
}) {
  const { t } = useTranslation();
  const generating = generation !== null;
  return (
    <Modal
      title={t('learning.createTitle')}
      visible={visible}
      confirmLoading={busy}
      onCancel={onClose}
      onOk={onOk}
      // 生成视图：隐藏主按钮，只保留关闭（后台继续，不终止生成）
      footer={
        generating ? (
          <Button onClick={onClose}>{t('learning.genClose')}</Button>
        ) : undefined
      }
      style={{ width: 560 }}
    >
      {generating ? (
        <CourseGenerationProgress
          status={generation.status}
          result={generation.result}
          error={generation.error}
          onStartLearning={onStartLearning}
          onRetry={onRetry}
          onCancel={onCancel}
        />
      ) : (
        <>
          <Tabs activeTab={creationTab} onChange={(key) => onTabChange(key as 'base' | 'description' | 'graph')}>
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
                <Paragraph className='mt-0 text-t-secondary'>{t('learning.generateDescriptionHint')}</Paragraph>
              </div>
            </Tabs.TabPane>
            <Tabs.TabPane
              key='graph'
              title={
                <span className='inline-flex items-center gap-6px'>
                  {t('learning.learningGraphTabTitle')}
                  <Tag size='small' color='orangered' className='!mx-0'>
                    {t('learning.learningGraphBeta')}
                  </Tag>
                </span>
              }
              destroyOnHide={false}
            >
              <div className='flex flex-col gap-16px'>
                <div>
                  <div className='mb-6px font-500'>{t('learning.learningGraphGoalInputLabel')}</div>
                  <Input.TextArea
                    value={creationDescription}
                    placeholder={t('learning.learningGraphGoalInputPlaceholder')}
                    onChange={onDescriptionChange}
                    autoSize={{ minRows: 3, maxRows: 6 }}
                  />
                </div>
                <Paragraph className='mt-0 text-t-secondary'>{t('learning.learningGraphCreateHint')}</Paragraph>
              </div>
            </Tabs.TabPane>
          </Tabs>
          <div className='mt-20px flex flex-col gap-16px'>
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
        </>
      )}
    </Modal>
  );
}
