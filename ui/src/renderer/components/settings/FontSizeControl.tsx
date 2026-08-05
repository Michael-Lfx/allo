

import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Button, Slider } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { useThemeContext } from '@renderer/hooks/context/ThemeContext';
import { FONT_SCALE_DEFAULT, FONT_SCALE_MAX, FONT_SCALE_MIN, FONT_SCALE_STEP } from '@renderer/hooks/ui/useFontScale';

// 浮点数比较容差 / Floating point comparison tolerance
const EPSILON = 0.001;
const RESET_THRESHOLD = 0.01;

/**
 * 将值限制在字体缩放的有效范围内 / Clamp value within valid font scale range
 * @param value - 要限制的值 / Value to clamp
 * @returns 限制后的值 / Clamped value
 */
const clamp = (value: number) => Math.min(FONT_SCALE_MAX, Math.max(FONT_SCALE_MIN, value));

const round2 = (value: number) => Number(value.toFixed(2));

/**
 * 字体大小控制组件 / Font size control component
 *
 * 提供界面缩放功能，支持滑块和按钮调节
 * Provides interface scaling with slider and button controls
 *
 * 说明:字体大小本质是 webview 级整页缩放(setZoom),在拖拽中逐帧调用会导致整页重排与抖动。
 * 因此滑块拖拽过程中(pointer 按下期间,含停顿)仅更新本地预览值,不触发 setZoom/写配置/进 context;
 * 真正应用缩放与持久化发生在松手(onAfterChange)。键盘方向键(无 pointer 按下)经 onChange 立即应用。
 * Note: font size is a webview-level zoom (setZoom); calling it on every drag frame reflows the
 * whole page and jitters. So while the pointer is down (drag, including pauses) only a local
 * preview updates — no setZoom / config write / context. Zoom + persistence happen on release
 * (onAfterChange). Keyboard arrows (no pointer down) apply immediately via onChange.
 */
const FontSizeControl: React.FC = () => {
  const { t } = useTranslation();
  const { fontScale, setFontScale, theme } = useThemeContext();

  // 本地预览值:仅驱动滑块显示,不进入 context,不触发 setZoom / NomiModal 等消费者重排。
  // Local preview: drives the slider display only — never enters context, so no setZoom / consumer reflow.
  const [sliderValue, setSliderValue] = useState(fontScale);

  // 拖拽锁:pointer 按下期间为 true,用于把「拖拽(含停顿)」与「键盘等无 pointer 操作」区分开。
  // Drag lock: true while the pointer is held, to tell drag (incl. pauses) apart from keyboard input.
  const isPointerDownRef = useRef(false);

  // 全局抬起/取消时释放锁:覆盖在滑块外松手、pointercancel、拖拽被中断等情况。
  // Release the lock on global pointer up/cancel: covers release outside the slider, pointercancel, aborts.
  useEffect(() => {
    const release = () => {
      isPointerDownRef.current = false;
    };
    window.addEventListener('pointerup', release);
    window.addEventListener('pointercancel', release);
    return () => {
      window.removeEventListener('pointerup', release);
      window.removeEventListener('pointercancel', release);
    };
  }, []);

  // 外部来源(启动恢复 / ±按钮 / 重置 / onAfterChange 应用 / 后端 clamp 修正)改变了 context
  // fontScale 时,同步到本地预览。拖拽中跳过,避免外部变更打断用户正在调整的预览值。
  // Sync context fontScale (startup restore / ± buttons / reset / onAfterChange apply / backend clamp)
  // back into the local preview, but skip while dragging so an external change can't hijack the value
  // the user is actively adjusting. One-way safe: setSliderValue never writes context, so no loop.
  useEffect(() => {
    if (!isPointerDownRef.current) setSliderValue(fontScale);
  }, [fontScale]);

  // 格式化显示值为百分比 / Format display value as percentage
  const formattedValue = useMemo(() => `${Math.round(sliderValue * 100)}%`, [sliderValue]);

  // 默认标记（100%位置）/ Default mark (100% position)
  const defaultMarks = useMemo(
    () => ({
      1: <span className='font-scale-default-mark' aria-hidden='true' title='100%'></span>,
    }),
    []
  );

  // apply = 既有 setFontScale:setZoom + 持久化 + 更新 context(随后 useEffect 同步 sliderValue)
  // apply = existing setFontScale: setZoom + persist + update context (useEffect then syncs sliderValue)
  const apply = (value: number) => {
    void setFontScale(clamp(round2(value)));
  };

  /**
   * 处理滑块值变化(拖拽中 / 键盘)——拖拽中仅本地预览;非拖拽(键盘等)立即应用
   * Handle slider value change (dragging / keyboard) — preview only while dragging; apply immediately otherwise
   * @param value - 新的缩放值 / New scale value
   */
  const handleSliderChange = (value: number | number[]) => {
    if (typeof value !== 'number') return;
    const v = clamp(round2(value));
    setSliderValue(v); // 预览(跟手),不进 context、不 setZoom
    if (!isPointerDownRef.current) {
      apply(v); // 非拖拽(键盘方向键等):立即应用
    }
  };

  /**
   * 处理滑块交互结束(松手 / 点击轨道)——立即应用
   * Handle end of slider interaction (release / track click) — apply immediately
   * @param value - 最终缩放值 / Final scale value
   */
  const handleSliderDone = (value: number | number[]) => {
    if (typeof value === 'number') apply(value);
  };

  /**
   * 处理步进调节 / Handle step adjustment
   * @param delta - 步进增量（正数增大，负数减小）/ Step delta (positive to increase, negative to decrease)
   */
  const handleStep = (delta: number) => {
    const next = clamp(Number((sliderValue + delta).toFixed(2)));
    apply(next);
  };

  /**
   * 重置到默认值 / Reset to default value
   */
  const handleReset = () => {
    apply(FONT_SCALE_DEFAULT);
  };
  const isResetDisabled = Math.abs(sliderValue - FONT_SCALE_DEFAULT) < RESET_THRESHOLD;

  return (
    <div className='flex flex-col gap-8px w-full min-w-0'>
      {/* onPointerDownCapture 标记拖拽开始;Arco Slider 不透传 DOM 事件,故挂在父 div。
          +/− 按钮的 pointerdown 也会置位,但按钮不触发 slider onChange,无副作用。 */}
      <div
        className='flex items-center gap-6px w-full min-w-0'
        onPointerDownCapture={() => {
          isPointerDownRef.current = true;
        }}
      >
        <Button
          size='mini'
          type='secondary'
          shape='circle'
          className='w-24px h-24px !min-w-24px shrink-0 flex items-center justify-center p-0'
          onClick={() => handleStep(-FONT_SCALE_STEP)}
          disabled={sliderValue <= FONT_SCALE_MIN + EPSILON}
        >
          -
        </Button>
        {/* 滑杆覆盖 80%-130% 区间，松手写入配置 / Slider covers 80%-130% range, persists on release */}
        <Slider
          className='flex-1 min-w-0 font-scale-slider p-0 m-0'
          showTicks
          min={FONT_SCALE_MIN}
          max={FONT_SCALE_MAX}
          step={FONT_SCALE_STEP}
          value={sliderValue}
          onChange={handleSliderChange}
          onAfterChange={handleSliderDone}
          marks={defaultMarks}
        />
        <Button
          size='mini'
          type='secondary'
          shape='circle'
          className='w-24px h-24px !min-w-24px shrink-0 flex items-center justify-center p-0'
          onClick={() => handleStep(FONT_SCALE_STEP)}
          disabled={sliderValue >= FONT_SCALE_MAX - EPSILON}
        >
          +
        </Button>
      </div>
      <div className='flex items-center justify-between gap-8px w-full min-w-0'>
        <span className='text-12px text-t-primary tabular-nums shrink-0'>{formattedValue}</span>
        <Button
          size='mini'
          type='text'
          className='!px-0 !h-auto min-w-0 text-12px'
          onClick={handleReset}
          disabled={isResetDisabled}
          style={{
            color: isResetDisabled
              ? theme === 'dark'
                ? 'rgba(230, 232, 236, 0.62)'
                : 'rgba(78, 89, 105, 0.72)'
              : 'rgb(var(--primary-6))',
            opacity: 1,
          }}
        >
          {t('settings.fontSizeReset')}
        </Button>
      </div>
    </div>
  );
};

export default FontSizeControl;
