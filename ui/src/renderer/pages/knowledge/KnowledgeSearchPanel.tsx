
import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Empty, Input, Modal, Spin } from '@arco-design/web-react';
import { Search } from '@icon-park/react';
import { ipcBridge } from '@/common';
import type { IKnowledgeSearchHit } from '@/common/adapter/ipcBridge';
import type { KnowledgeBaseId } from '@/common/types/ids';

type KnowledgeSearchPanelProps = {
  visible: boolean;
  knowledgeBaseId: KnowledgeBaseId;
  onClose: () => void;
  onSelectHit: (relPath: string) => void;
};

const KnowledgeSearchPanel: React.FC<KnowledgeSearchPanelProps> = ({
  visible,
  knowledgeBaseId,
  onClose,
  onSelectHit,
}) => {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(false);
  const [hits, setHits] = useState<IKnowledgeSearchHit[]>([]);

  useEffect(() => {
    if (!visible) {
      setQuery('');
      setHits([]);
      setLoading(false);
    }
  }, [visible]);

  const runSearch = useCallback(
    async (q: string) => {
      const trimmed = q.trim();
      if (!trimmed) {
        setHits([]);
        return;
      }
      setLoading(true);
      try {
        const res = await ipcBridge.knowledge.search.invoke({
          kbIds: [knowledgeBaseId],
          query: trimmed,
          limit: 20,
        });
        setHits(res);
      } catch (e) {
        console.error('knowledge search failed', e);
        setHits([]);
      } finally {
        setLoading(false);
      }
    },
    [knowledgeBaseId]
  );

  useEffect(() => {
    if (!visible) return;
    const handle = window.setTimeout(() => {
      void runSearch(query);
    }, 280);
    return () => window.clearTimeout(handle);
  }, [query, visible, runSearch]);

  return (
    <Modal
      title={t('knowledge.detail.searchTitle', { defaultValue: '检索知识库' })}
      visible={visible}
      onCancel={onClose}
      footer={null}
      unmountOnExit
      style={{ width: 560 }}
    >
      <div className='flex flex-col gap-12px'>
        <Input
          allowClear
          autoFocus
          prefix={<Search theme='outline' size='14' />}
          placeholder={t('knowledge.detail.searchInputPlaceholder', {
            defaultValue: '输入关键词，检索标题与正文…',
          })}
          value={query}
          onChange={setQuery}
        />
        <Spin loading={loading} className='w-full'>
          <div className='knowledge-search-hits max-h-360px overflow-y-auto flex flex-col gap-8px'>
            {!query.trim() ? (
              <Empty
                description={t('knowledge.detail.searchHint', {
                  defaultValue: '输入关键词后即可看到命中文档',
                })}
              />
            ) : hits.length === 0 && !loading ? (
              <Empty
                description={t('knowledge.detail.searchEmpty', {
                  defaultValue: '未找到匹配内容',
                })}
              />
            ) : (
              hits.map((hit) => (
                <button
                  key={`${hit.kb_id}:${hit.rel_path}:${hit.heading}`}
                  type='button'
                  className='knowledge-search-hit flex w-full flex-col gap-4px rounded-10px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-12px py-10px text-left cursor-pointer transition-colors hover:border-[rgba(var(--primary-6),0.36)] hover:bg-[rgba(var(--primary-6),0.06)]'
                  onClick={() => {
                    onSelectHit(hit.rel_path);
                    onClose();
                  }}
                >
                  <span className='text-13px font-600 text-[var(--color-text-1)] truncate'>
                    {hit.heading || hit.rel_path}
                  </span>
                  <span className='text-11px text-[var(--color-text-3)] truncate'>{hit.rel_path}</span>
                  {hit.snippet ? (
                    <span className='text-12px leading-relaxed text-[var(--color-text-2)] line-clamp-2'>
                      {hit.snippet}
                    </span>
                  ) : null}
                </button>
              ))
            )}
          </div>
        </Spin>
      </div>
    </Modal>
  );
};

export default KnowledgeSearchPanel;
