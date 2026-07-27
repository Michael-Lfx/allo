
import type { IMessageToolCall } from '@/common/chat/chatLib';
import { normalizeToolCall } from '@/common/chat/normalizeToolCall';
import { IconDown, IconRight } from '@arco-design/web-react/icon';
import { BookOne } from '@icon-park/react';
import React, { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import './MessageToolDetails.css';

type ParsedHit = {
  path: string;
  heading?: string;
  snippet?: string;
  kbId?: string;
};

/** Parse a hit count from the knowledge_search output text
 *  ("N result(s) for …" / "No matches …"). Count, 0 for explicit no-match,
 *  or null when undeterminable (e.g. still running). */
function parseHitCount(output: string | undefined): number | null {
  if (!output) return null;
  const m = output.match(/^\s*(\d+)\s+result/);
  if (m) return Number(m[1]);
  if (/^\s*No matches/.test(output)) return 0;
  return null;
}

/** Best-effort extraction of hit rows from tool output for Grounded Trail cards. */
function parseHits(output: string | undefined): ParsedHit[] {
  if (!output) return [];
  const hits: ParsedHit[] = [];
  const blocks = output.split(/\n(?=[-*•]|\d+\.|\[)/);
  for (const block of blocks) {
    const pathMatch =
      block.match(/(?:path|file|rel_path)[:\s]+([^\s,]+\.md)/i) ||
      block.match(/([A-Za-z0-9_./-]+\.md)/);
    if (!pathMatch) continue;
    const headingMatch = block.match(/(?:heading|title)[:\s]+(.+)/i);
    const snippetMatch = block.match(/(?:snippet|excerpt)[:\s]+([\s\S]+)/i);
    const kbMatch = block.match(/(?:kb[_ ]?id)[:\s]+([0-9a-f-]{20,})/i);
    hits.push({
      path: pathMatch[1].trim(),
      heading: headingMatch?.[1]?.trim(),
      snippet: snippetMatch?.[1]?.trim().slice(0, 160),
      kbId: kbMatch?.[1]?.trim(),
    });
    if (hits.length >= 5) break;
  }
  if (hits.length === 0) {
    for (const m of output.matchAll(/([A-Za-z0-9_./-]+\.md)/g)) {
      hits.push({ path: m[1] });
      if (hits.length >= 5) break;
    }
  }
  return hits;
}

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

  const statusNode = (() => {
    if (status === 'running') return <span className='text-t-secondary'>{t('knowledge.searchChip.searching')}</span>;
    if (status === 'error') return <span className='text-t-secondary'>{t('knowledge.searchChip.error')}</span>;
    if (count === 0) return <span className='text-t-secondary'>{t('knowledge.searchChip.noHit')}</span>;
    if (count != null && count > 0) {
      return (
        <span className='knowledge-search-hit-pop text-brand font-600'>
          {t('knowledge.searchChip.hit', { count })}
        </span>
      );
    }
    return null;
  })();

  const canExpand = Boolean(output);

  return (
    <div className='knowledge-search-chip flex flex-col gap-6px'>
      <div
        className={
          'inline-flex items-center gap-6px px-8px py-2px rounded-6px bg-fill-2 text-13px max-w-full' +
          (canExpand ? ' cursor-pointer hover:bg-bg-3' : '')
        }
        onClick={canExpand ? () => setExpanded(!expanded) : undefined}
      >
        <span className='flex-shrink-0 inline-flex'>
          <BookOne theme='outline' size='14' fill='currentColor' />
        </span>
        <span className='font-medium text-t-primary flex-shrink-0'>{t('knowledge.searchChip.label')}</span>
        {query && (
          <span className='text-t-secondary truncate'>
            {t('knowledge.searchChip.query', { query: typedQuery || query })}
          </span>
        )}
        {statusNode && <span className='flex-shrink-0 m-l-2px'>{statusNode}</span>}
        {canExpand && (
          <span className='flex-shrink-0 text-t-secondary'>
            {expanded ? <IconDown style={{ fontSize: 12 }} /> : <IconRight style={{ fontSize: 12 }} />}
          </span>
        )}
      </div>
      {expanded && (
        <div className='knowledge-grounded-trail m-l-20px flex flex-col gap-6px'>
          {hits.length > 0
            ? hits.map((hit) => (
                <button
                  key={`${hit.kbId ?? ''}:${hit.path}:${hit.heading ?? ''}`}
                  type='button'
                  className='knowledge-grounded-hit flex w-full flex-col gap-2px rounded-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-10px py-8px text-left cursor-pointer hover:border-[rgba(var(--primary-6),0.36)]'
                  onClick={() => {
                    if (hit.kbId) {
                      navigate(`/knowledge/${hit.kbId}?highlight=${encodeURIComponent(hit.path)}`);
                    } else {
                      navigate('/knowledge');
                    }
                  }}
                >
                  <span className='text-12px font-600 text-[var(--color-text-1)] truncate'>
                    {hit.heading || hit.path}
                  </span>
                  <span className='text-11px text-[var(--color-text-3)] truncate'>{hit.path}</span>
                  {hit.snippet ? (
                    <span className='text-11px leading-relaxed text-[var(--color-text-2)] line-clamp-2'>
                      {hit.snippet}
                    </span>
                  ) : null}
                </button>
              ))
            : output
              ? (
                  <div className='tool-detail-panel'>
                    <pre className='tool-detail-content'>{output}</pre>
                  </div>
                )
              : null}
        </div>
      )}
    </div>
  );
};

export default KnowledgeSearchChip;
