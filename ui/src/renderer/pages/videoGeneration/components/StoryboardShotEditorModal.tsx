import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input, Modal } from '@arco-design/web-react';
import { Left, Right } from '@icon-park/react';
import ErrorDiagnosticContent from '@/renderer/components/base/ErrorDiagnosticContent';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import { buildUnknownErrorDiagnostic } from '@/renderer/utils/ui/errorDiagnostics';
import { getArtifact, writeArtifactText } from '../api';
import type { StoryboardScene } from '../artifactPresentation';
import { saveShotCopy, ShotCopySaveError, type ShotCopySaveResult } from '../storyboardShotCopy';

const TextArea = Input.TextArea;

export type StoryboardShotEditorFocus = 'visual' | 'audio';

interface StoryboardShotEditorModalProps {
  sessionId: string;
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
  onSaved: (result: ShotCopySaveResult) => void;
}

function isDirty(visual: string, audio: string, scene: StoryboardScene): boolean {
  return visual !== (scene.visualDescription ?? '') || audio !== (scene.audioDescription ?? '');
}

const StoryboardShotEditorModal: React.FC<StoryboardShotEditorModalProps> = ({
  sessionId,
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
  onSaved,
}) => {
  const { t } = useTranslation();
  const [message, messageHolder] = useArcoMessage();
  const [visualDraft, setVisualDraft] = useState(scene.visualDescription ?? '');
  const [audioDraft, setAudioDraft] = useState(scene.audioDescription ?? '');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setVisualDraft(scene.visualDescription ?? '');
    setAudioDraft(scene.audioDescription ?? '');
  }, [scene.audioDescription, scene.id, scene.visualDescription]);

  const canSave = visualDraft.trim().length > 0;

  const confirmIfDirty = useCallback(
    (next: () => void) => {
      if (!isDirty(visualDraft, audioDraft, scene)) {
        next();
        return;
      }
      Modal.confirm({
        title: t('videoGeneration.studio.storyboard.unsavedTitle', {
          defaultValue: '放弃未保存的修改？',
        }),
        content: t('videoGeneration.studio.storyboard.unsavedBody', {
          defaultValue: '关闭或切换镜头后，当前修改会丢失。',
        }),
        okText: t('videoGeneration.studio.storyboard.discard', { defaultValue: '放弃' }),
        cancelText: t('common.cancel', { defaultValue: '取消' }),
        okButtonProps: { status: 'danger' },
        onOk: next,
      });
    },
    [audioDraft, scene, t, visualDraft]
  );

  const requestClose = useCallback(() => {
    confirmIfDirty(onClose);
  }, [confirmIfDirty, onClose]);

  const handleSave = useCallback(async () => {
    if (!visualDraft.trim() || saving) return;
    setSaving(true);
    try {
      const result = await saveShotCopy(
        {
          getText: async (path) => {
            const content = await getArtifact(sessionId, path);
            return content.text;
          },
          writeText: async (path, content) => {
            await writeArtifactText(sessionId, path, content);
          },
        },
        scene,
        { visualDescription: visualDraft, audioDescription: audioDraft }
      );
      onSaved(result);
      message.success(
        t('videoGeneration.studio.storyboard.visualSaveOk', {
          defaultValue: '镜头描述已保存',
        })
      );
    } catch (error) {
      if (error instanceof ShotCopySaveError) {
        switch (error.kind) {
          case 'missing':
            Modal.error({
              title: t('videoGeneration.studio.storyboard.visualSaveMissing', {
                defaultValue: '找不到可保存的分镜文件',
              }),
            });
            break;
          case 'empty_visual':
            break;
          default: {
            const _exhaustive: never = error.kind;
            return _exhaustive;
          }
        }
        return;
      }
      Modal.error({
        title: t('videoGeneration.studio.storyboard.visualSaveFailed', {
          defaultValue: '保存镜头描述失败',
        }),
        content: (
          <ErrorDiagnosticContent
            diagnostic={buildUnknownErrorDiagnostic(
              error,
              t('videoGeneration.studio.storyboard.visualSaveFailed', {
                defaultValue: '保存镜头描述失败',
              })
            )}
          />
        ),
      });
    } finally {
      setSaving(false);
    }
  }, [audioDraft, message, onSaved, saving, scene, sessionId, t, visualDraft]);

  useEffect(() => {
    if (!visible) return;
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        void handleSave();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [handleSave, visible]);

  const packedBeats = scene.beats && scene.beats.length >= 2 ? scene.beats : null;

  const title = useMemo(
    () => (
      <div className='flex items-center gap-8px'>
        {hasPrev || hasNext ? (
          <Button
            size='mini'
            type='text'
            icon={<Left theme='outline' size={14} />}
            disabled={!hasPrev || saving}
            aria-label={t('videoGeneration.studio.storyboard.prevShot', {
              defaultValue: '上一镜头',
            })}
            onClick={() => confirmIfDirty(onPrev)}
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
            disabled={!hasNext || saving}
            aria-label={t('videoGeneration.studio.storyboard.nextShot', {
              defaultValue: '下一镜头',
            })}
            onClick={() => confirmIfDirty(onNext)}
          />
        ) : null}
      </div>
    ),
    [confirmIfDirty, hasNext, hasPrev, onNext, onPrev, saving, sceneNumber, t, total]
  );

  return (
    <>
      {messageHolder}
      <Modal
        visible={visible}
        onCancel={requestClose}
        unmountOnExit
        style={{ width: 'min(720px, 92vw)' }}
        title={title}
        footer={
          <>
            <Button onClick={requestClose} disabled={saving}>
              {t('common.cancel', { defaultValue: '取消' })}
            </Button>
            <Button
              type='primary'
              loading={saving}
              disabled={!canSave}
              onClick={() => void handleSave()}
              aria-label={
                saving
                  ? t('videoGeneration.studio.storyboard.saving', { defaultValue: '保存中' })
                  : t('common.save', { defaultValue: '保存' })
              }
            >
              {t('common.save', { defaultValue: '保存' })}
            </Button>
          </>
        }
      >
        <div className='flex flex-col gap-16px'>
          <label className='flex flex-col gap-8px'>
            <span className='text-12px font-650 text-[var(--color-text-2)]'>
              {t('videoGeneration.studio.storyboard.visualDirection', {
                defaultValue: '画面描述',
              })}
            </span>
            <TextArea
              key={`${scene.id}-visual`}
              value={visualDraft}
              onChange={setVisualDraft}
              autoFocus={focusField === 'visual'}
              autoSize={{ minRows: 8, maxRows: 16 }}
              disabled={saving}
              placeholder={t('videoGeneration.studio.storyboard.visualEditPlaceholder', {
                defaultValue: '描述这个镜头的画面…',
              })}
            />
          </label>
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
          <label className='flex flex-col gap-8px'>
            <span className='text-12px font-650 text-[var(--color-text-2)]'>
              {t('videoGeneration.studio.storyboard.audioDirection', {
                defaultValue: '音频 / 台词',
              })}
            </span>
            <TextArea
              key={`${scene.id}-audio`}
              value={audioDraft}
              onChange={setAudioDraft}
              autoFocus={focusField === 'audio'}
              autoSize={{ minRows: 5, maxRows: 10 }}
              disabled={saving}
              placeholder={t('videoGeneration.studio.storyboard.audioEditPlaceholder', {
                defaultValue: '背景音乐、环境音或台词…',
              })}
            />
          </label>
        </div>
      </Modal>
    </>
  );
};

export default StoryboardShotEditorModal;
