
import React, { useCallback, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Input, Message, Modal, Spin } from '@arco-design/web-react';
import { BookOne, Earth, FolderOpen, Plus } from '@icon-park/react';
import { ipcBridge } from '@/common';
import { isDesktopShell } from '@renderer/utils/platform';
import type { KnowledgeBaseId } from '@/common/types/ids';
import { knowledgeErrorText } from './useKnowledge';
import { bindingForNewBase, stashKnowledgeActivation } from './knowledgeActivation';

export type QuickCaptureSeed = 'sample' | 'blank' | 'local' | 'web';

type QuickCaptureProps = {
  visible: boolean;
  initialSeed?: QuickCaptureSeed;
  onClose: () => void;
  onAdvanced?: () => void;
};

const QuickCapture: React.FC<QuickCaptureProps> = ({
  visible,
  initialSeed = 'sample',
  onClose,
  onAdvanced,
}) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [seed, setSeed] = useState<QuickCaptureSeed>(initialSeed);
  const [url, setUrl] = useState('');
  const [rootPath, setRootPath] = useState('');
  const [busy, setBusy] = useState(false);

  React.useEffect(() => {
    if (visible) {
      setSeed(initialSeed);
      setUrl('');
      setRootPath('');
    }
  }, [visible, initialSeed]);

  const finishActivation = useCallback(
    (knowledgeBaseId: KnowledgeBaseId, suggestPrompt: string) => {
      stashKnowledgeActivation({
        knowledge_base_id: knowledgeBaseId,
        suggest_prompt: suggestPrompt,
        binding: bindingForNewBase(knowledgeBaseId),
        auto_send: true,
      });
      onClose();
      navigate('/guid');
    },
    [navigate, onClose]
  );

  const runQuick = useCallback(
    async (nextSeed: QuickCaptureSeed, overrides?: { root_path?: string; url?: string }) => {
      setBusy(true);
      try {
        const outcome = await ipcBridge.knowledge.quickCreate.invoke({
          seed: nextSeed,
          root_path: overrides?.root_path,
          url: overrides?.url,
        });
        Message.success(
          t('knowledge.quick.createOkAuto', {
            defaultValue: '知识库已就绪，正在打开对话并提问…',
          })
        );
        finishActivation(outcome.base.knowledge_base_id, outcome.suggest_prompt);
      } catch (e) {
        Message.error(knowledgeErrorText(e));
      } finally {
        setBusy(false);
      }
    },
    [finishActivation, t]
  );

  const pickLocal = useCallback(async () => {
    if (!isDesktopShell()) {
      Message.warning(t('knowledge.quick.desktopOnly', { defaultValue: '选择本地目录需要桌面端' }));
      return;
    }
    try {
      const paths = await ipcBridge.dialog.showOpen.invoke({ properties: ['openDirectory'] });
      const path = paths?.[0];
      if (!path) return;
      setRootPath(path);
      setSeed('local');
      await runQuick('local', { root_path: path });
    } catch (e) {
      Message.error(String(e));
    }
  }, [runQuick, t]);

  const submitWeb = useCallback(async () => {
    const trimmed = url.trim();
    if (!trimmed) {
      Message.warning(t('knowledge.quick.urlRequired', { defaultValue: '请粘贴一个网页 URL' }));
      return;
    }
    await runQuick('web', { url: trimmed });
  }, [runQuick, t, url]);

  const tiles: Array<{
    key: QuickCaptureSeed;
    title: string;
    desc: string;
    icon: React.ReactNode;
    onClick: () => void;
  }> = [
    {
      key: 'sample',
      title: t('knowledge.quick.sampleTitle', { defaultValue: '用示例库试试' }),
      desc: t('knowledge.quick.sampleDesc', { defaultValue: '内置 FAQ，60 秒看到有据回答' }),
      icon: <BookOne theme='outline' size='20' />,
      onClick: () => void runQuick('sample'),
    },
    {
      key: 'local',
      title: t('knowledge.quick.localTitle', { defaultValue: '本地目录' }),
      desc: t('knowledge.quick.localDesc', { defaultValue: '引用电脑上已有的 Markdown 文件夹' }),
      icon: <FolderOpen theme='outline' size='20' />,
      onClick: () => void pickLocal(),
    },
    {
      key: 'web',
      title: t('knowledge.quick.webTitle', { defaultValue: '抓取网页' }),
      desc: t('knowledge.quick.webDesc', { defaultValue: '粘贴 URL，快照进知识库' }),
      icon: <Earth theme='outline' size='20' />,
      onClick: () => setSeed('web'),
    },
    {
      key: 'blank',
      title: t('knowledge.quick.blankTitle', { defaultValue: '空白库' }),
      desc: t('knowledge.quick.blankDesc', { defaultValue: '先建空库，稍后再放资料' }),
      icon: <Plus theme='outline' size='20' />,
      onClick: () => void runQuick('blank'),
    },
  ];

  return (
    <Modal
      title={t('knowledge.quick.title', { defaultValue: '把资料变成 Agent 的工作记忆' })}
      visible={visible}
      onCancel={onClose}
      footer={null}
      unmountOnExit
      style={{ width: 560 }}
    >
      <Spin loading={busy} className='w-full'>
        <div className='flex flex-col gap-14px'>
          <p className='m-0 text-13px leading-relaxed text-[var(--color-text-3)]'>
            {t('knowledge.quick.subtitle', {
              defaultValue: '选一种来源，系统会建库并带你去对话里试问一句。',
            })}
          </p>
          <div className='grid grid-cols-1 gap-10px sm:grid-cols-2'>
            {tiles.map((tile) => (
              <button
                key={tile.key}
                type='button'
                className={`knowledge-quick-tile flex flex-col gap-6px rounded-12px border border-solid px-14px py-12px text-left cursor-pointer transition-colors ${
                  seed === tile.key
                    ? 'border-[rgba(var(--primary-6),0.4)] bg-[rgba(var(--primary-6),0.08)]'
                    : 'border-[var(--color-border-2)] bg-[var(--color-fill-1)] hover:border-[rgba(var(--primary-6),0.28)]'
                }`}
                onClick={tile.onClick}
                disabled={busy}
              >
                <span className='inline-flex items-center gap-8px text-[var(--color-text-1)]'>
                  <span className='text-[rgb(var(--primary-6))]'>{tile.icon}</span>
                  <span className='text-13px font-600'>{tile.title}</span>
                </span>
                <span className='text-12px leading-relaxed text-[var(--color-text-3)]'>{tile.desc}</span>
              </button>
            ))}
          </div>

          {seed === 'web' ? (
            <div className='flex flex-col gap-8px rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] p-12px'>
              <Input
                allowClear
                placeholder='https://example.com/docs'
                value={url}
                onChange={setUrl}
                onPressEnter={() => void submitWeb()}
              />
              <Button type='primary' long onClick={() => void submitWeb()} loading={busy}>
                {t('knowledge.quick.captureWeb', { defaultValue: '抓取并开始' })}
              </Button>
            </div>
          ) : null}

          {rootPath ? (
            <p className='m-0 text-12px text-[var(--color-text-3)] truncate'>
              {t('knowledge.quick.selectedPath', { defaultValue: '已选：{{path}}', path: rootPath })}
            </p>
          ) : null}

          {onAdvanced ? (
            <button
              type='button'
              className='self-start border-none bg-transparent p-0 text-12px text-[var(--color-text-3)] cursor-pointer hover:text-[var(--color-text-1)]'
              onClick={onAdvanced}
            >
              {t('knowledge.quick.moreSources', { defaultValue: '更多来源（飞书 / 导入 zip）›' })}
            </button>
          ) : null}
        </div>
      </Spin>
    </Modal>
  );
};

export default QuickCapture;
