import { useLocation, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { isGrowthActive } from '../growthTaskPill';
import { useGrowthTaskPills } from '../hooks/useGrowthTaskPills';
import { setLearningFloatingSlot } from '../learningFloatingDock';
import { GrowthTaskPill } from './GrowthTaskPill';

/**
 * 生长全局悬浮 Pill 宿主（g4）：挂在 ProtectedLayout（全部受保护路由的公共
 * 包装），跨页常驻——生长运行/排队中切任意页面 Pill 都在，可回图工作区
 * （/learn/{course_id}）或取消；终态（以快照为准）可关闭，关闭只是本地视图
 * 操作。已在该课程图工作区时不出「回到图工作区」（工作区自有 growth 读数）。
 *
 * 右下角停靠栏（复用 CourseGenerationPill 的 fixed right-24px bottom-24px
 * z-100 骨架）纵向排队：生长 Pill 在上，底部是建课 Pill 插槽——学习页列表
 * 的 CourseGenerationPill 经 portal 落在这里，两者同屏不重叠。停靠栏本身
 * 不吃点击（pointer-events-none），只有 Pill 卡片可交互。
 */
export function GrowthTaskPillHost() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const { items, cancelling, cancel, dismiss } = useGrowthTaskPills(t);

  return (
    <div className='pointer-events-none fixed right-24px bottom-24px z-100 flex flex-col items-end gap-8px'>
      {items.map((item) => {
        const workspace = `/learn/${item.courseId}`;
        return (
          <div key={item.key} className='pointer-events-auto'>
            <GrowthTaskPill
              item={item}
              cancelling={cancelling.has(item.key)}
              onView={
                pathname === workspace
                  ? undefined
                  : () => {
                      // 去看终态结果 = 已读：顺手关掉；活跃项保留继续跟进度。
                      if (!isGrowthActive(item.status)) dismiss(item.key);
                      void navigate(workspace);
                    }
              }
              onCancel={() => void cancel(item)}
              onDismiss={() => dismiss(item.key)}
            />
          </div>
        );
      })}
      <div ref={setLearningFloatingSlot} className='pointer-events-auto contents' />
    </div>
  );
}
