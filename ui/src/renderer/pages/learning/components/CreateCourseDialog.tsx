import { useTranslation } from 'react-i18next';
import { Button, Input, Message, Modal, Radio, Select, Tabs, Tag, Typography } from '@arco-design/web-react';
import type { IKnowledgeBase } from '@/common/adapter/ipcBridge';
import type { CourseGenerationEstimateView, TeachingStyle } from '../types';
import { MAX_COURSE_KNOWLEDGE_BASES, type CourseGenerationState } from '../hooks/useCourseCreation';
import { knowledgeBaseNames } from '../utils';
import LearningModelSelector, {
  type LearningModelChoice,
} from './LearningModelSelector';
import { CourseGenerationProgress } from './CourseGenerationProgress';
import { CourseCostGateModal } from './CourseCostGateModal';

const { Text, Paragraph } = Typography;

/** 创建课程对话框：方式一（从知识库生成）/ 方式二（描述直接生成，无知识库参与）/
 * 方式三（学习图 beta：目标 + 可选程度/基础声明 → 建课；知识结构生成与首生长
 * 在后台启动）。提交后就地切换为生成/进度视图：结构与生长在后台继续（工作区
 * 顶条与悬浮指示条可回看）。 */
export function CreateCourseDialog({
  visible,
  busy,
  knowledgeLoading,
  knowledgeBases,
  selectedKnowledgeBaseIds,
  generationDomain,
  modelChoice,
  creationTab,
  creationDescription,
  graphProficiency,
  graphBasis,
  creationGoal,
  teachingStyle,
  onTeachingStyleChange,
  generation,
  onClose,
  onOk,
  onSelectedBaseChange,
  onDomainChange,
  onModelChange,
  onTabChange,
  onDescriptionChange,
  onGraphProficiencyChange,
  onGraphBasisChange,
  onConfirmGraphCreation,
  onGoalChange,
  onRetry,
  onCancel,
  onStartLearning,
  costGate,
  onDismissCostGate,
}: {
  visible: boolean;
  busy: boolean;
  knowledgeLoading: boolean;
  knowledgeBases: IKnowledgeBase[];
  /** 多知识库（t4）：来源库列表（首个 = 主来源），上限 MAX_COURSE_KNOWLEDGE_BASES */
  selectedKnowledgeBaseIds: string[];
  generationDomain: string;
  modelChoice: LearningModelChoice;
  creationTab: 'base' | 'description' | 'graph';
  creationDescription: string;
  /** 学习图课程的程度声明（可选）：缺省由结构生成依据目标扩写 */
  graphProficiency: string;
  /** 学习图课程的基础声明（可选）：缺省按纯零基础画地形 */
  graphBasis: string;
  /** 用户目标（知识库流，t1）：整体文本即目标，课程名默认即该文本。 */
  creationGoal: string;
  teachingStyle: TeachingStyle;
  onTeachingStyleChange: (style: TeachingStyle) => void;
  generation: CourseGenerationState | null;
  onClose: () => void;
  onOk: () => void;
  onSelectedBaseChange: (values: string[]) => void;
  onDomainChange: (value: string) => void;
  onModelChange: (choice: LearningModelChoice) => void;
  onTabChange: (tab: 'base' | 'description' | 'graph') => void;
  onDescriptionChange: (value: string) => void;
  onGraphProficiencyChange: (value: string) => void;
  onGraphBasisChange: (value: string) => void;
  onConfirmGraphCreation: () => void;
  onGoalChange: (value: string) => void;
  onRetry: () => void;
  onCancel: () => void;
  onStartLearning: (courseId: string) => void;
  /** t5 成本门：超阈值（>40 文件或 >20 次预估调用）时挂起的估算 + 放行
   * 回调；null = 低于阈值/未触发，不打扰。 */
  costGate: { estimate: CourseGenerationEstimateView; proceed: () => void } | null;
  /** t5 成本门：取消确认（丢弃挂起的请求，回到表单）。 */
  onDismissCostGate: () => void;
}) {
  const { t } = useTranslation();
  const generating = generation !== null;
  const graphWizard = creationTab === 'graph' && !generating;

  // 主按钮语义按 tab 分流：学习图单步向导不走 onOk（默认提交）。
  const handleOk = () => {
    if (graphWizard) {
      onConfirmGraphCreation();
      return;
    }
    onOk();
  };
  const okText = graphWizard ? t('learning.learningGraphCreateSubmit') : t('learning.createSubmit');

  // 多知识库（t4）：生成视图的来源披露（请求里的库 id → 库名，口径与悬浮
  // 指示条共用 `knowledgeBaseNames`）。
  const generatingSources: string[] = knowledgeBaseNames(
    generation?.request.knowledge_base_ids ?? [],
    knowledgeBases
  );

  return (
    <Modal
      title={t('learning.createTitle')}
      visible={visible}
      confirmLoading={busy}
      okText={okText}
      onCancel={onClose}
      cancelText={t('learning.genClose')}
      onOk={handleOk}
      // 生成视图：隐藏主按钮，只保留关闭（结构与生长在后台继续）
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
          sources={generatingSources}
          onStartLearning={onStartLearning}
          onRetry={onRetry}
          onCancel={onCancel}
        />
      ) : (
        <div className='max-h-[calc(100vh-190px)] overflow-y-auto'>
          <Tabs activeTab={creationTab} onChange={(key) => onTabChange(key as 'base' | 'description' | 'graph')}>
            <Tabs.TabPane key='base' title={t('learning.generateFromBase')} destroyOnHide={false}>
              <Paragraph className='mt-0 text-t-secondary'>{t('learning.generateHint')}</Paragraph>
              <div className='flex flex-col gap-16px'>
                {/* 学习目标（t1）：整体文本即目标——简单时即课程名，复杂时是
                    目标描述；课程标题默认即该文本，采样与大纲范围都以此为主索引 */}
                <div>
                  <div className='mb-6px font-500'>{t('learning.courseGoal')}</div>
                  <Input.TextArea
                    value={creationGoal}
                    placeholder={t('learning.courseGoalPlaceholder')}
                    onChange={onGoalChange}
                    autoSize={{ minRows: 3, maxRows: 6 }}
                  />
                  <Text type='secondary' className='text-12px'>
                    {t('learning.courseGoalHint')}
                  </Text>
                </div>
                <div>
                  <div className='mb-6px font-500'>{t('learning.knowledgeBase')}</div>
                  {/* 多知识库（t4）：多选（参照 CS 智能体的挂载库多选）；
                      首个选中 = 主来源，上限与后端同一口径 */}
                  <Select
                    className='w-full'
                    mode='multiple'
                    loading={knowledgeLoading}
                    value={selectedKnowledgeBaseIds}
                    placeholder={t('learning.selectKnowledgeBase')}
                    maxTagCount={MAX_COURSE_KNOWLEDGE_BASES}
                    onChange={(values: string[]) => {
                      const next = values ?? [];
                      // 超限是可见的错误并**拒绝这次选择**（保留上一次的合法
                      // 选择）——绝不静默截断把第 6 个库悄悄丢掉。
                      if (next.length > MAX_COURSE_KNOWLEDGE_BASES) {
                        Message.warning(
                          t('learning.kbSelectionTooMany', {
                            count: MAX_COURSE_KNOWLEDGE_BASES,
                          })
                        );
                        return;
                      }
                      onSelectedBaseChange(next);
                    }}
                  >
                    {knowledgeBases.map((base) => (
                      <Select.Option key={base.knowledge_base_id} value={base.knowledge_base_id}>
                        {base.name} ({base.file_count} {t('learning.files')})
                      </Select.Option>
                    ))}
                  </Select>
                  <Text type='secondary' className='text-12px'>
                    {t('learning.selectKnowledgeBaseHint', { count: MAX_COURSE_KNOWLEDGE_BASES })}
                  </Text>
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
                <div>
                  <div className='mb-6px font-500'>{t('learning.learningGraphProficiencyLabel')}</div>
                  <Input
                    value={graphProficiency}
                    placeholder={t('learning.learningGraphProficiencyPlaceholder')}
                    maxLength={200}
                    onChange={onGraphProficiencyChange}
                  />
                </div>
                <div>
                  <div className='mb-6px font-500'>{t('learning.learningGraphBasisLabel')}</div>
                  <Input.TextArea
                    value={graphBasis}
                    placeholder={t('learning.learningGraphBasisPlaceholder')}
                    autoSize={{ minRows: 2, maxRows: 4 }}
                    onChange={onGraphBasisChange}
                  />
                </div>
                <Paragraph className='mt-0 text-t-secondary'>{t('learning.learningGraphCreateHint')}</Paragraph>
              </div>
            </Tabs.TabPane>
          </Tabs>
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
        </div>
      )}
      {/* t5 成本门确认弹窗：估算 + 规则分诊读数，「继续」放行原请求、
          「取消」丢弃。独立于生成进度视图（提交前确认，不是生成中）。 */}
      {costGate && (
        <CourseCostGateModal
          estimate={costGate.estimate}
          onConfirm={costGate.proceed}
          onCancel={onDismissCostGate}
        />
      )}
    </Modal>
  );
}
