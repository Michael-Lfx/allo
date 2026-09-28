import { useTranslation } from 'react-i18next';
import { Button, Input, Modal, Radio, Select, Tabs, Tag, Typography } from '@arco-design/web-react';
import { IconDelete, IconPlus } from '@arco-design/web-react/icon';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import type { EndpointInput, TeachingStyle } from '../types';
import type { CourseGenerationState } from '../hooks/useCourseCreation';
import LearningModelSelector, {
  type LearningModelChoice,
} from './LearningModelSelector';
import { CourseGenerationProgress } from './CourseGenerationProgress';

const { Text, Paragraph } = Typography;

/** 创建课程对话框：方式一（从知识库生成）/ 方式二（描述直接生成，无知识库参与）/
 * 方式三（学习图 beta：目标 → AI 提议终点 → 确认建课）。
 * 提交后就地切换为生成/进度视图：生长在后台继续（悬浮指示条可回到这里）。 */
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
  teachingStyle,
  graphStep,
  proposingEndpoints,
  draftEndpoints,
  onTeachingStyleChange,
  generation,
  onClose,
  onOk,
  onSelectedBaseChange,
  onDomainChange,
  onModelChange,
  onTabChange,
  onDescriptionChange,
  onProposeEndpoints,
  onConfirmGraphCreation,
  onGraphStepBack,
  onDraftEndpointsChange,
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
  teachingStyle: TeachingStyle;
  /** 学习图向导步：goal = 目标输入；endpoints = 终点确认 */
  graphStep: 'goal' | 'endpoints';
  proposingEndpoints: boolean;
  draftEndpoints: EndpointInput[];
  onTeachingStyleChange: (style: TeachingStyle) => void;
  generation: CourseGenerationState | null;
  onClose: () => void;
  onOk: () => void;
  onSelectedBaseChange: (value: string) => void;
  onDomainChange: (value: string) => void;
  onModelChange: (choice: LearningModelChoice) => void;
  onTabChange: (tab: 'base' | 'description' | 'graph') => void;
  onDescriptionChange: (value: string) => void;
  onProposeEndpoints: () => void;
  onConfirmGraphCreation: () => void;
  onGraphStepBack: () => void;
  onDraftEndpointsChange: (endpoints: EndpointInput[]) => void;
  onRetry: () => void;
  onCancel: () => void;
  onStartLearning: (courseId: string) => void;
}) {
  const { t } = useTranslation();
  const generating = generation !== null;
  const graphWizard = creationTab === 'graph' && !generating;
  const footerText =
    graphWizard && graphStep === 'endpoints'
      ? t('learning.learningGraphCreateSubmit')
      : undefined;

  // 主按钮语义按 tab / 向导步分流：学习图向导不走 onOk（默认提交）。
  const handleOk = () => {
    if (graphWizard) {
      if (graphStep === 'goal') onProposeEndpoints();
      else onConfirmGraphCreation();
      return;
    }
    onOk();
  };
  const okText = graphWizard
    ? graphStep === 'goal'
      ? t('learning.learningGraphProposeNext')
      : t('learning.learningGraphCreateSubmit')
    : t('learning.createSubmit');

  return (
    <Modal
      title={t('learning.createTitle')}
      visible={visible}
      confirmLoading={busy || proposingEndpoints}
      okText={okText}
      onCancel={
        graphWizard && graphStep === 'endpoints' && !generating
          ? onGraphStepBack
          : onClose
      }
      cancelText={
        graphWizard && graphStep === 'endpoints' && !generating
          ? t('learning.learningGraphProposeBack')
          : t('learning.genClose')
      }
      onOk={handleOk}
      // 生成视图：隐藏主按钮，只保留关闭（生长在后台继续）
      footer={
        generating ? (
          <Button onClick={onClose}>{t('learning.genClose')}</Button>
        ) : (
          undefined
        )
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
              {graphStep === 'goal' ? (
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
              ) : (
                <div className='flex flex-col gap-12px'>
                  <Paragraph className='mt-0 text-t-secondary'>{t('learning.learningGraphEndpointStepHint')}</Paragraph>
                  {draftEndpoints.map((endpoint, index) => (
                    <div
                      key={index}
                      className='flex flex-col gap-6px rounded-8px border-1 border-solid border-[var(--color-border-2)] p-10px'
                    >
                      <div className='flex items-center gap-8px'>
                        <Input
                          value={endpoint.title}
                          placeholder={t('learning.learningGraphEndpointTitlePlaceholder')}
                          maxLength={40}
                          onChange={(value) =>
                            onDraftEndpointsChange(
                              draftEndpoints.map((item, position) =>
                                position === index ? { ...item, title: value } : item
                              )
                            )
                          }
                        />
                        <Button
                          type='text'
                          status='danger'
                          disabled={draftEndpoints.length === 1}
                          icon={<IconDelete />}
                          onClick={() =>
                            onDraftEndpointsChange(
                              draftEndpoints.filter((_, position) => position !== index)
                            )
                          }
                        />
                      </div>
                      <Input
                        value={endpoint.goal_note}
                        placeholder={t('learning.learningGraphEndpointNotePlaceholder')}
                        onChange={(value) =>
                          onDraftEndpointsChange(
                            draftEndpoints.map((item, position) =>
                              position === index ? { ...item, goal_note: value } : item
                            )
                          )
                        }
                      />
                    </div>
                  ))}
                  {draftEndpoints.length < 3 && (
                    <Button
                      long
                      type='text'
                      icon={<IconPlus />}
                      onClick={() =>
                        onDraftEndpointsChange([...draftEndpoints, { title: '', goal_note: '' }])
                      }
                    >
                      {t('learning.learningGraphEndpointAdd')}
                    </Button>
                  )}
                </div>
              )}
            </Tabs.TabPane>
          </Tabs>
          {footerText && (
            <Paragraph className='!mb-0 mt-12px text-t-secondary'>{footerText}</Paragraph>
          )}
          <div className='mt-20px flex flex-col gap-16px'>
            <div>
              <div className='mb-8px font-500'>{t('learning.teachingStyle')}</div>
              <Radio.Group
                size='small'
                value={teachingStyle}
                onChange={(value) => onTeachingStyleChange(value as TeachingStyle)}
              >
                <Radio value='standard'>{t('learning.styleStandard')}</Radio>
                <Radio value='socratic'>{t('learning.styleSocratic')}</Radio>
                <Radio value='feynman'>{t('learning.styleFeynman')}</Radio>
              </Radio.Group>
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
        </>
      )}
    </Modal>
  );
}
