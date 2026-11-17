/**
 * @license
 * Copyright 2025-2026 NomiFuni (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Progress } from '@arco-design/web-react';

/** S3 共享进度条（课程卡与工作区大纲头同一形状）：分母 0（percent = null
 * ——f2 口径，无可计课时）不渲染；其余按 small / 无文字渲染。计数行的
 * 「a/N / —」文本面在 model.progressReadoutText（纯决策层），本组件只管
 * 条形渲染与空值守卫。 */
export function ProgressReadout({ percent }: { percent: number | null }) {
  if (percent === null) return null;
  return <Progress percent={percent} showText={false} size='small' />;
}
