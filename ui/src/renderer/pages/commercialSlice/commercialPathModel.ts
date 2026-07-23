

export type CommercialPathState =
  | 'first_user'
  | 'returning_user'
  | 'missing_model'
  | 'missing_workspace'
  | 'network_failure'
  | 'model_failure'
  | 'task_success';

export type CommercialPathScene = 'login' | 'home' | 'execution' | 'result';

export type CommercialPathFrame = {
  state: CommercialPathState;
  scene: CommercialPathScene;
  title: string;
  body: string;
  primaryAction: string;
  secondaryAction?: string;
  statusChips?: Array<{ id: string; label: string; state: 'ready' | 'blocked' | 'optional' }>;
  planPreview?: string;
};

export const COMMERCIAL_PATH_FRAMES: CommercialPathFrame[] = [
  {
    state: 'first_user',
    scene: 'login',
    title: '首次用户 · 登录',
    body: '价值预览 + 邮箱验证码。一条主行动：发送验证码 / 验证登录。',
    primaryAction: '发送验证码',
    secondaryAction: '隐私与条款',
  },
  {
    state: 'returning_user',
    scene: 'home',
    title: '成果启动台 · 就绪',
    body: '单一主输入 + 状态芯片 + 执行预览。输入或点模板后立即看到 Flowy 如何接管。',
    primaryAction: '开始任务',
    statusChips: [
      { id: 'project', label: '项目已识别', state: 'ready' },
      { id: 'plan', label: '执行方案已准备', state: 'ready' },
      { id: 'permission', label: '权限按需询问', state: 'ready' },
    ],
    planPreview: '读取项目 → 定位失败 → 修改并验证 → 汇报根因',
  },
  {
    state: 'missing_model',
    scene: 'home',
    title: '成果启动台 · 缺模型',
    body: '阻断态用警示语义，不离开任务舞台。连接后自动恢复草稿并继续。',
    primaryAction: '原位连接模型',
    secondaryAction: '稍后再说',
    statusChips: [
      { id: 'project', label: '项目可选', state: 'optional' },
      { id: 'plan', label: '需连接模型', state: 'blocked' },
      { id: 'permission', label: '权限按需询问', state: 'ready' },
    ],
    planPreview: '理解目标 → 选择工具 → 执行并检查 → 交付成果',
  },
  {
    state: 'missing_workspace',
    scene: 'home',
    title: '成果启动台 · 缺项目',
    body: '项目类模板前置要求文件夹。选择后自动续接，不要求二次发送。',
    primaryAction: '选择项目文件夹',
    secondaryAction: '改用通用任务',
    statusChips: [
      { id: 'project', label: '项目待选择', state: 'blocked' },
      { id: 'plan', label: '执行方案已准备', state: 'ready' },
      { id: 'permission', label: '权限按需询问', state: 'ready' },
    ],
    planPreview: '读取项目 → 定位失败 → 修改并验证 → 汇报根因',
  },
  {
    state: 'network_failure',
    scene: 'execution',
    title: '网络失败',
    body: '进度轨停在可恢复点，提供重试与保留草稿。',
    primaryAction: '重试',
    secondaryAction: '返回编辑',
  },
  {
    state: 'model_failure',
    scene: 'execution',
    title: '模型失败',
    body: '明确错误原因，允许换模型或调整提示后继续。',
    primaryAction: '更换模型',
    secondaryAction: '修改任务',
  },
  {
    state: 'task_success',
    scene: 'result',
    title: '首个可检查成果',
    body: '成果卡：已验证状态、文件变更、根因摘要；提供继续追问与保存为可复用流程。',
    primaryAction: '继续追问',
    secondaryAction: '保存为流程',
  },
];
