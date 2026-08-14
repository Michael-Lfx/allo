
import type { IMessageToolCall } from '@/common/chat/chatLib';
import { normalizeToolCall } from '@/common/chat/normalizeToolCall';
import type { NormalizedToolStatus } from '@/common/chat/normalizeToolCall';
import ContextCards, { sourceKindFromPath } from '@renderer/components/beautifulUi/contextCards/ContextCards';
import { ToolChip } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import type { ToolChipStatus } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import React, { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { parseHitCount, parseHits } from './KnowledgeSearchChip.parse';
import './MessageToolDetails.css';

const knowledgeChipStatus = (status: NormalizedToolStatus | undefined): ToolChipStatus => {
  switch (status) {
    case 'pending':
      return 'pending';
    case 'running':
      return 'running';
    case 'error':
      return 'error';
    case 'canceled':
      return 'canceled';
    case 'completed':
    case undefined:
      return 'completed';
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

const KnowledgeSearchChip: React.FC<{ message: IMessageToolCall }> = ({ message }) => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [expanded, setExpanded] = useState(false);
  const [typedQuery, setTypedQuery] = useState('');

  const query = String(message.content.args?.query ?? message.content.input?.query ?? '').trim();
  const normalized = normalizeToolCall(message);
  const output = normalized?.output;
  const status = normalized?.status;
  const count = useMemo(() => parseHitCount(output), [output]);
  const hits = useMemo(() => parseHits(output), [output]);

  useEffect(() => {
    if (!query) {
      setTypedQuery('');
      return;
    }
    setTypedQuery('');
    let i = 0;
    const id = window.setInterval(() => {
      i += 1;
      setTypedQuery(query.slice(0, i));
      if (i >= query.length) window.clearInterval(id);
    }, 18);
    return () => window.clearInterval(id);
  }, [query]);

  useEffect(() => {
    if (status === 'completed' && count != null && count > 0) {
      setExpanded(true);
    }
  }, [status, count]);

  const statusText = (() => {
    if (status === 'running') return t('knowledge.searchChip.searching');
    if (status === 'error') return t('knowledge.searchChip.error');
    if (count === 0) return t('knowledge.searchChip.noHit');
    if (count != null && count > 0) return t('knowledge.searchChip.hit', { count });
    return undefined;
  })();
  const queryText = query ? t('knowledge.searchChip.query', { query: typedQuery || query }) : undefined;
  const detail = [queryText, statusText].filter(Boolean).join(' · ') || undefined;
  const canExpand = Boolean(output);

  return (
    <div className='knowledge-search-chip flex flex-col gap-6px'>
      <ToolChip
        id={message.id}
        name={t('knowledge.searchChip.label')}
        detail={detail}
        status={knowledgeChipStatus(status)}
        expandable={canExpand}
        expanded={expanded}
        onToggle={canExpand ? () => setExpanded((value) => !value) : undefined}
      />
      {expanded && (
        <div className='knowledge-grounded-trail m-l-20px flex flex-col gap-6px'>
          {hits.length > 0 ? (
            <ContextCards
              items={hits.map((hit, index) => ({
                id: `${hit.path}-${index}`,
                title: hit.heading || hit.path,
                snippet: hit.snippet,
                sourceKind: sourceKindFromPath(hit.path),
                sourceLabel: hit.path,
                onOpen: () => {
                  if (hit.kbId) {
                    navigate(`/knowledge/${hit.kbId}?highlight=${encodeURIComponent(hit.path)}`);
                  } else {
                    navigate('/knowledge');
                  }
                },
              }))}
            />
          ) : output ? (
            <div className='tool-detail-panel'>
              <pre className='tool-detail-content'>{output}</pre>
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
};

export default KnowledgeSearchChip;

