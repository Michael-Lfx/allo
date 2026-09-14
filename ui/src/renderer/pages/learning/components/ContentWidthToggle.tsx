import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';
import { IconFullscreen, IconFullscreenExit } from '@arco-design/web-react/icon';
import { configService } from '@/common/config/configService';
import { useConfig } from '@/renderer/hooks/config/useConfig';

const STORAGE_KEY = 'learning.wideContent';

/**
 * 学习模块的「标准 / 满宽」布局偏好。三个渲染面（课程列表、大纲工作区、学习图
 * 工作区）共用这一个键，所以切一次就整体生效，不会出现「列表宽了、点进课程又
 * 缩回去」。宽度类名本身来自 `../layout`，这里只管读写偏好。
 */
export function useWideContentLayout(): {
  wide: boolean;
  setWide: (next: boolean) => Promise<void>;
  toggle: () => Promise<void>;
} {
  // 响应式读取（useSyncExternalStore 订阅），与 useLearningAutogenModel 一致：
  // 切一次要立刻传播到三个面上的类名，一次性 configService.get() 拿不到更新。
  const [stored] = useConfig(STORAGE_KEY);
  const wide = stored === true;

  // 回到标准布局即删键，让「未设置」与「标准」归一，存档里不长留一个等于默认
  // 值的布尔。（不能用 set(key, undefined)：那会 PUT 出一个空对象，后端不会删行。）
  const setWide = useCallback(
    (next: boolean) =>
      next ? configService.set(STORAGE_KEY, true) : configService.remove(STORAGE_KEY),
    []
  );

  const toggle = useCallback(() => setWide(!wide), [setWide, wide]);

  return { wide, setWide, toggle };
}

/**
 * 内容宽度切换按钮：图标表示「点了会切换到哪个布局」，tooltip 用同一个字符串
 * 说明，避免"图标表示当前状态还是目标状态"的歧义。三个渲染面的头部各放一个。
 */
const ContentWidthToggle: React.FC = () => {
  const { t } = useTranslation();
  const { wide, toggle } = useWideContentLayout();
  const label = wide ? t('learning.contentWidthToStandard') : t('learning.contentWidthToWide');

  return (
    <Button
      size='small'
      type='text'
      aria-label={label}
      title={label}
      onClick={() => void toggle()}
    >
      {wide ? <IconFullscreenExit /> : <IconFullscreen />}
    </Button>
  );
};

export default ContentWidthToggle;
