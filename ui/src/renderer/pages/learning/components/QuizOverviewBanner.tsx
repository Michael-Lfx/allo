/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Typography } from '@arco-design/web-react';
import { IconThunderbolt } from '@arco-design/web-react/icon';
import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { learningApi } from '../api';
import type { QuizOverviewCourse } from '../types';

const { Title, Text } = Typography;

// 外壳/行按钮类串克隆自 ReviewBanner（u7 沿用既有视觉；ReviewBanner 是
// 存量，不跨文件抽常量）。color-mix 长串在本文件多处出现——抽文件内常量
// 消重；常量内是完整 class token 原文，UnoCSS 文本扫描照常提取。
const SHELL_CLASS =
  'rounded-12px border border-solid border-[color-mix(in_srgb,rgb(var(--primary-6))_35%,var(--color-border-2))] bg-[color-mix(in_srgb,rgb(var(--primary-6))_6%,var(--color-bg-2))] px-20px py-16px';
const ROW_CLASS =
  'flex w-full cursor-pointer flex-wrap items-center justify-between gap-12px rounded-10px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-16px py-12px transition-colors hover:border-[color-mix(in_srgb,rgb(var(--primary-6))_50%,var(--color-border-2))] hover:bg-[color-mix(in_srgb,rgb(var(--primary-6))_6%,var(--color-bg-2))]';

/** 检测感知横幅（u7，grill 裁决 Q3 第一步）：学习首页的跨课程检测聚合——
 * 每行课程名 + N/K 读数 + due 标记，点行直达该课程工作区（传统/图课程
 * 各自工作区的检测入口可见；图分支入口由 u6 并行落地，本票只管跳路由）。
 *
 * 渲染策略与 ReviewBanner **刻意不同**：复习每天天然有货（队列空也保留
 * 常驻入口），检测是稀事件——有 due 课程才渲染，无 due 整块缺席（不渲染
 * 空壳）。拉取中与拉取失败同样整块缺席：横幅本身是感知信号，空壳与常驻
 * 失败句都是噪音，且课程页内入口（QuizEntryCard）不依赖本横幅，缺席不损
 * 功能，故失败不吐 toast、不渲染占位。
 *
 * 数据面（读数非门禁）：进页挂载拉一次（learningApi.quizOverview，后端已
 * 按 due 优先、课程名次之排好序，前端不再重排）；组件自拉（QuizEntryCard
 * 同款，index.tsx 只接 navigate），并发重拉 last-wins 序号防陈旧回写。
 * N/K 读数复用 quizProgressReading 词典键（与课程页入口同文案）。 */
export function QuizOverviewBanner({
  onOpenCourse,
}: {
  onOpenCourse: (courseId: string) => void;
}) {
  const { t } = useTranslation();
  const [courses, setCourses] = useState<QuizOverviewCourse[]>([]);
  // 并发重拉 last-wins 序号：挂载后若触发重拉，只让最新一次写读数。
  const pullSeqRef = useRef(0);

  const pullOverview = useCallback(async () => {
    const seq = ++pullSeqRef.current;
    try {
      const rows = await learningApi.quizOverview();
      if (seq === pullSeqRef.current) setCourses(rows);
    } catch {
      // 拉取失败：横幅整块缺席（头部注释——感知面不渲染失败噪音）。
    }
  }, []);

  // 进页挂载拉一次；无定时器即无轮询面（读数非门禁，QuizEntryCard 同律）
  useEffect(() => {
    void pullOverview();
  }, [pullOverview]);

  const dueCourses = courses.filter((course) => course.due);
  // 有 due 课程才渲染：无 due（含拉取中/失败=空列表）整块不出现
  if (dueCourses.length === 0) return null;

  return (
    <div className={SHELL_CLASS}>
      <Title heading={5} className='!m-0'>
        {t('learning.quizOverviewTitle')}
      </Title>
      <div className='mt-8px flex flex-col gap-8px'>
        {dueCourses.map((course) => (
          <button
            key={course.course_id}
            type='button'
            className={ROW_CLASS}
            onClick={() => onOpenCourse(course.course_id)}
          >
            <span className='flex min-w-0 flex-wrap items-center gap-8px'>
              <IconThunderbolt className='shrink-0 text-16px text-primary-6' />
              <Text bold>{course.title}</Text>
              <Text className='rounded-full border border-solid border-primary-6 px-8px py-2px text-12px leading-18px text-primary-6'>
                {t('learning.quizOverviewDueTag')}
              </Text>
            </span>
            <Text type='secondary'>
              {t('learning.quizProgressReading', {
                settled: course.settled_count,
                trigger: course.trigger_count,
              })}
            </Text>
          </button>
        ))}
      </div>
    </div>
  );
}
