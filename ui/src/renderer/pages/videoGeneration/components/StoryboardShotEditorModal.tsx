import React, { useCallback, useEffect, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Modal } from '@arco-design/web-react';
import { Left, Right } from '@icon-park/react';
import type { StoryboardScene } from '../artifactPresentation';

export type StoryboardShotEditorFocus = 'visual' | 'audio';

interface StoryboardShotEditorModalProps {
  scene: StoryboardScene;
  sceneNumber: number;
  total: number;
  visible: boolean;
  focusField: StoryboardShotEditorFocus;
  hasPrev: boolean;
  hasNext: boolean;
  onClose: () => void;
  onPrev: () => void;
  onNext: () => void;
}

const StoryboardShotEditorModal: React.FC<StoryboardShotEditorModalProps> = ({
  scene,
  sceneNumber,
  total,
  visible,
  focusField,
  hasPrev,
  hasNext,
  onClose,
  onPrev,
  onNext,
}) => {
  const { t } = useTranslation();
  const visualRef = useRef<HTMLDivElement>(null);
  const audioRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!visible) return;
    const target = focusField === 'audio' ? audioRef.current : visualRef.current;
    target?.scrollIntoView({ block: 'nearest' });
  }, [focusField, scene.id, visible]);

  const packedBeats = scene.beats && scene.beats.length >= 2 ? scene.beats : null;

  const title = useMemo(
    () => (
      <div className='flex items-center gap-8px'>
        {hasPrev || hasNext ? (
          <Button
            size='mini'
            type='text'
            icon={<Left theme='outline' size={14} />}
            disabled={!hasPrev}
            aria-label={t('videoGeneration.studio.storyboard.prevShot', {
              defaultValue: '上一镜头',
            })}
            onClick={onPrev}
          />
        ) : null}
        <span>
          {t('videoGeneration.studio.storyboard.shotNumberOf', {
            number: sceneNumber,
            total,
            defaultValue: '镜头 {{number}} / {{total}}',
          })}
        </span>
        {hasPrev || hasNext ? (
          <Button
            size='mini'
            type='text'
            icon={<Right theme='outline' size={14} />}
            disabled={!hasNext}
            aria-label={t('videoGeneration.studio.storyboard.nextShot', {
              defaultValue: '下一镜头',
            })}
            onClick={onNext}
          />
        ) : null}
      </div>
    ),
    [hasNext, hasPrev, onNext, onPrev, sceneNumber, t, total]
  );

  const handleCancel = useCallback(() => {
    onClose();
  }, [onClose]);

  return (
    <Modal
      visible={visible}
      onCancel={handleCancel}
      unmountOnExit
      style={{ width: 'min(720px, 92vw)' }}
      title={title}
      footer={
        <Button onClick={handleCancel}>
          {t('common.close', { defaultValue: '关闭' })}
        </Button>
      }
    >
      <div className='flex flex-col gap-16px'>
        <div
          ref={visualRef}
          className='flex flex-col gap-8px'
          data-focus={focusField === 'visual' ? 'true' : undefined}
        >
          <span className='text-12px font-650 text-[var(--color-text-2)]'>
            {t('videoGeneration.studio.storyboard.visualDirection', {
              defaultValue: '画面描述',
            })}
          </span>
          <p className='m-0 min-h-80px whitespace-pre-wrap text-13px leading-22px text-[var(--color-text-1)]'>
            {scene.visualDescription ||
              t('videoGeneration.studio.storyboard.visualPending', {
                defaultValue: '画面生成后将在这里展示。',
              })}
          </p>
        </div>
        {packedBeats ? (
          <div className='flex flex-col gap-8px rd-8px bg-[var(--color-fill-2)] px-12px py-10px'>
            {packedBeats.map((beat, beatIndex) => (
              <div key={`${scene.id}-beat-${beatIndex}`} className='flex flex-col gap-4px'>
                <div className='text-11px font-650 text-[var(--color-text-3)]'>
                  {t('videoGeneration.studio.storyboard.packedBeatItem', {
                    number: beatIndex + 1,
                    defaultValue: '切镜 {{number}}',
                  })}
                </div>
                <p className='m-0 whitespace-pre-wrap text-12px leading-20px text-[var(--color-text-2)]'>
                  {beat.visualDescription}
                </p>
              </div>
            ))}
          </div>
        ) : null}
        <div
          ref={audioRef}
          className='flex flex-col gap-8px'
          data-focus={focusField === 'audio' ? 'true' : undefined}
        >
          <span className='text-12px font-650 text-[var(--color-text-2)]'>
            {t('videoGeneration.studio.storyboard.audioDirection', {
              defaultValue: '音频 / 台词',
            })}
          </span>
          <p className='m-0 min-h-60px whitespace-pre-wrap text-13px leading-22px text-[var(--color-text-1)]'>
            {scene.audioDescription ||
              t('videoGeneration.studio.storyboard.audioPending', {
                defaultValue: '暂无音频或台词描述',
              })}
          </p>
        </div>
      </div>
    </Modal>
  );
};

export default StoryboardShotEditorModal;
