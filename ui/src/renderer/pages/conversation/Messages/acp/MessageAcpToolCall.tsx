

import type { IMessageAcpToolCall } from '@/common/chat/chatLib';
import { toDisplayText } from '@/common/chat/displayText';
import { normalizeAcpToolCall } from '@/common/chat/normalizeToolCall';
import InlineDiff from '@renderer/components/beautifulUi/inlineDiff/InlineDiff';
import {
  INLINE_DIFF_COLLAPSE_LINE_THRESHOLD,
  countDiffLines,
  countDiffStats,
  hunksFromOldNew,
} from '@renderer/components/beautifulUi/inlineDiff/inlineDiffModel';
import { ToolChip } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import { chipDetailOmittingCommand, resolveToolChipStatus } from '@renderer/components/beautifulUi/toolChips/toolChipModel';
import React, { useMemo } from 'react';
import MarkdownView from '@renderer/components/Markdown';
import LocalImageView from '@/renderer/components/media/LocalImageView';
import { MESSAGE_BODY_FONT_SIZE, MESSAGE_BODY_LINE_HEIGHT } from '../typography';

const DiffContentView: React.FC<{ old_text: string; new_text: string; path: string }> = ({
  old_text,
  new_text,
  path,
}) => {
  const display_name = path.split(/[/\\]/).pop() || path || 'Unknown file';
  const hunks = useMemo(() => hunksFromOldNew(old_text, new_text), [old_text, new_text]);
  const { insertions, deletions } = countDiffStats(hunks);

  if (!hunks.length) return null;

  return (
    <InlineDiff
      filename={display_name}
      hunks={hunks}
      insertions={insertions}
      deletions={deletions}
      defaultExpanded={countDiffLines(hunks) <= INLINE_DIFF_COLLAPSE_LINE_THRESHOLD}
    />
  );
};

const ContentView: React.FC<{
  content: NonNullable<IMessageAcpToolCall['content']['update']['content']>[number];
  terminalSuccess: boolean;
}> = ({ content, terminalSuccess }) => {
  if (content.type === 'diff') {
    return (
      <DiffContentView
        old_text={toDisplayText(content.old_text)}
        new_text={toDisplayText(content.new_text)}
        path={toDisplayText(content.path)}
      />
    );
  }

  if (content.type === 'artifact') {
    if (!terminalSuccess) return null;
    const { artifact } = content;
    return (
      <div className='mt-3 rounded border overflow-hidden'>
        {artifact.kind === 'image' && (
          <LocalImageView
            src={artifact.path}
            alt={artifact.relative_path || 'Generated image'}
            className='block max-w-full max-h-420px object-contain'
          />
        )}
        <code className='block bg-1 p-2 text-xs break-all' title={artifact.path}>
          {artifact.path}
        </code>
      </div>
    );
  }

  if (content.type === 'resource_link') {
    if (!terminalSuccess) return null;
    return (
      <div className='mt-3 bg-1 p-3 rounded border break-all'>
        <a href={content.uri} title={content.description || content.uri}>
          {content.title || content.name || content.uri}
        </a>
      </div>
    );
  }

  if (content.type === 'artifact_error') {
    return <div className='mt-3 bg-red-1 color-red-6 p-3 rounded border'>{content.message}</div>;
  }

  if (content.type === 'terminal') {
    return <code className='block mt-3 bg-1 p-2 rounded'>Terminal: {content.terminal_id}</code>;
  }

  // 处理 content 类型，包含 text 内容
  if (content.type === 'content' && content.content && content.content.type === 'text' && content.content.text) {
    return (
      <div className='mt-3'>
        <div className='bg-1 p-3 rounded border overflow-hidden'>
          <div className='overflow-x-auto break-words'>
            <MarkdownView
              fontSize={MESSAGE_BODY_FONT_SIZE}
              lineHeight={MESSAGE_BODY_LINE_HEIGHT}
              allowUnverifiedImages={false}
            >
              {toDisplayText(content.content.text)}
            </MarkdownView>
          </div>
        </div>
      </div>
    );
  }

  return null;
};

const MessageAcpToolCall: React.FC<{ message: IMessageAcpToolCall }> = ({ message }) => {
  const { content } = message;
  if (!content?.update) {
    return null;
  }
  const { update } = content;
  const { rawInput, content: diffContent, status } = update;
  const normalized = normalizeAcpToolCall(message);
  if (!normalized) {
    return null;
  }

  return (
    <div className='flex flex-col gap-6px'>
      <ToolChip
        id={normalized.key || message.id}
        name={normalized.name}
        detail={chipDetailOmittingCommand(normalized.name, normalized.description)}
        status={resolveToolChipStatus({
          status: normalized.status,
          skipped: normalized.skipped,
          notExecutedReason: normalized.notExecutedReason,
          nonFatalFailure: normalized.nonFatalFailure,
        })}
      />
      {rawInput && (
        <div className='text-sm'>
          {typeof rawInput === 'string' ? (
            <MarkdownView fontSize={MESSAGE_BODY_FONT_SIZE} lineHeight={MESSAGE_BODY_LINE_HEIGHT}>
              {`\`\`\`\n${rawInput}\n\`\`\``}
            </MarkdownView>
          ) : (
            <pre className='bg-1 p-2 rounded text-xs overflow-x-auto'>{toDisplayText(rawInput)}</pre>
          )}
        </div>
      )}
      {diffContent && diffContent.length > 0 && (
        <div>
          {diffContent.map((item, index) => (
            <ContentView key={index} content={item} terminalSuccess={status === 'completed'} />
          ))}
        </div>
      )}
    </div>
  );
};

export default MessageAcpToolCall;
