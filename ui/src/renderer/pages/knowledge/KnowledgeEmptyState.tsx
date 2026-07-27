
import React, { useCallback, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Message, Spin } from '@arco-design/web-react';
import { BookOne, Earth, FolderOpen, Plus, Upload } from '@icon-park/react';
import { ipcBridge } from '@/common';
import { isDesktopShell } from '@renderer/utils/platform';
import { knowledgeErrorText } from './useKnowledge';
import { bindingForNewBase, stashKnowledgeActivation } from './knowledgeActivation';

export type KnowledgeKindShortcut = 'blank' | 'local' | 'web' | 'feishu' | 'sample';

interface KnowledgeEmptyStateProps {
  onCreate: (initialKind?: KnowledgeKindShortcut) => void;
  onImport?: () => void;
  onAdvanced?: () => void;
}

const KnowledgeEmptyState: React.FC<KnowledgeEmptyStateProps> = ({ onCreate, onImport, onAdvanced }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [busy, setBusy] = useState(false);
  const [dropActive, setDropActive] = useState(false);

  const activate = useCallback(
    async (seed: 'sample' | 'blank' | 'local' | 'web', opts?: { root_path?: string; url?: string }) => {
      setBusy(true);
      try {
        const outcome = await ipcBridge.knowledge.quickCreate.invoke({
          seed,
          root_path: opts?.root_path,
          url: opts?.url,
        });
        stashKnowledgeActivation({
          knowledge_base_id: outcome.base.knowledge_base_id,
          suggest_prompt: outcome.suggest_prompt,
          binding: bindingForNewBase(outcome.base.knowledge_base_id),
          auto_send: true,
        });
        Message.success(
          t('knowledge.quick.createOkAuto', {
            defaultValue: '知识库已就绪，正在打开对话并提问…',
          })
        );
        navigate('/guid');
      } catch (e) {
        Message.error(knowledgeErrorText(e));
      } finally {
        setBusy(false);
      }
    },
    [navigate, t]
  );

  const pickLocal = useCallback(async () => {
    if (!isDesktopShell()) {
      onCreate('local');
      return;
    }
    try {
      const paths = await ipcBridge.dialog.showOpen.invoke({ properties: ['openDirectory'] });
      const path = paths?.[0];
      if (!path) return;
      await activate('local', { root_path: path });
    } catch (e) {
      Message.error(String(e));
    }
  }, [activate, onCreate]);

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setDropActive(false);
      // Browser DnD of folders is limited; guide user to local picker.
      void pickLocal();
    },
    [pickLocal]
  );

  const tiles = [
    {
      key: 'sample' as const,
      label: t('knowledge.quick.sampleTitle', { defaultValue: '用示例库试试' }),
      icon: <BookOne theme='outline' size='20' />,
      onClick: () => void activate('sample'),
    },
    {
      key: 'local' as const,
      label: t('knowledge.empty.kindLocal', { defaultValue: '本地目录' }),
      icon: <FolderOpen theme='outline' size='20' />,
      onClick: () => void pickLocal(),
    },
    {
      key: 'web' as const,
      label: t('knowledge.empty.kindWeb', { defaultValue: '从网页抓取' }),
      icon: <Earth theme='outline' size='20' />,
      onClick: () => onCreate('web'),
    },
    {
      key: 'blank' as const,
      label: t('knowledge.empty.kindBlank', { defaultValue: '空白知识库' }),
      icon: <Plus theme='outline' size='20' />,
      onClick: () => void activate('blank'),
    },
  ];

  return (
    <div className='flex w-full flex-col items-center gap-28px px-16px py-48px'>
      <div className='flex flex-col items-center gap-10px text-center'>
        <h2 className='m-0 text-22px font-bold text-[var(--color-text-1)]'>
          {t('knowledge.onboarding.title', { defaultValue: '把资料丢进去，下一句就有据可查' })}
        </h2>
        <p className='m-0 max-w-520px text-14px leading-relaxed text-[var(--color-text-3)]'>
          {t('knowledge.onboarding.subtitle', {
            defaultValue: '知识库是 Agent 可见的工作记忆。先塞资料，再问一句，马上看到命中。',
          })}
        </p>
      </div>

      <Spin loading={busy} className='w-full max-w-720px'>
        <div
          className={`knowledge-drop-zone flex w-full flex-col items-center gap-14px rounded-18px border border-dashed px-24px py-36px transition-colors ${
            dropActive
              ? 'border-[rgba(var(--primary-6),0.55)] bg-[rgba(var(--primary-6),0.08)]'
              : 'border-[var(--color-border-3)] bg-[var(--color-fill-1)]'
          }`}
          onDragEnter={(e) => {
            e.preventDefault();
            setDropActive(true);
          }}
          onDragOver={(e) => {
            e.preventDefault();
            setDropActive(true);
          }}
          onDragLeave={() => setDropActive(false)}
          onDrop={onDrop}
        >
          <div className='flex size-56px items-center justify-center rounded-full bg-[var(--control-selected-bg)] text-[var(--control-selected-fg)]'>
            <Upload theme='outline' size='26' fill='currentColor' />
          </div>
          <div className='text-15px font-600 text-[var(--color-text-1)]'>
            {t('knowledge.onboarding.dropTitle', { defaultValue: '拖入文件夹，或选一种方式开始' })}
          </div>
          <div className='text-13px text-[var(--color-text-3)]'>
            {t('knowledge.onboarding.dropHint', {
              defaultValue: '试问还没资料可答——先放入 Markdown，再去对话验证。',
            })}
          </div>

          <div className='mt-8px grid w-full grid-cols-2 gap-10px sm:grid-cols-4'>
            {tiles.map((tile) => (
              <button
                key={tile.key}
                type='button'
                className='knowledge-empty-kind-tile flex flex-col items-center gap-8px rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-1)] px-10px py-14px cursor-pointer transition-colors hover:border-[rgba(var(--primary-6),0.36)] hover:bg-[rgba(var(--primary-6),0.05)]'
                onClick={tile.onClick}
                disabled={busy}
              >
                <span className='text-[rgb(var(--primary-6))]'>{tile.icon}</span>
                <span className='text-12px font-500 text-[var(--color-text-1)] text-center'>{tile.label}</span>
              </button>
            ))}
          </div>
        </div>
      </Spin>

      <div className='flex flex-wrap items-center justify-center gap-12px'>
        <button
          type='button'
          className='border-none bg-transparent p-0 text-13px text-[var(--color-text-3)] cursor-pointer hover:text-[var(--color-text-1)]'
          onClick={() => onAdvanced?.() ?? onCreate()}
        >
          {t('knowledge.quick.moreSources', { defaultValue: '更多来源（飞书 / 导入 zip）›' })}
        </button>
        {onImport ? (
          <button
            type='button'
            className='border-none bg-transparent p-0 text-13px text-[var(--color-text-3)] cursor-pointer hover:text-[var(--color-text-1)]'
            onClick={onImport}
          >
            {t('knowledge.onboarding.import', { defaultValue: '导入' })}
          </button>
        ) : null}
      </div>
    </div>
  );
};

export default KnowledgeEmptyState;
