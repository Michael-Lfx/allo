/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

export type CommercialPathState =
  | 'first_user'
  | 'returning_user'
  | 'missing_model'
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
    title: '已有用户 · 首页',
    body: '单一主输入，意图建议填入，运行设置默认折叠。',
    primaryAction: '发送任务',
  },
  {
    state: 'missing_model',
    scene: 'home',
    title: '缺少模型',
    body: '原位凭据补全，不离开任务舞台跳到设置迷宫。',
    primaryAction: '添加模型密钥',
    secondaryAction: '稍后再说',
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
    title: '任务成功',
    body: '成果摘要 + 文件变更预览 + 撤销 / 继续追问，同屏连续。',
    primaryAction: '继续追问',
    secondaryAction: '撤销本轮',
  },
];
