
import type { IMessageToolCall } from '@/common/chat/chatLib';
import { toDisplayText } from '@/common/chat/displayText';
import { normalizeToolCall } from '@/common/chat/normalizeToolCall';
import type { NormalizedToolStatus } from '@/common/chat/normalizeToolCall';
import KnowledgeSearchChip from './KnowledgeSearchChip';
import FileChangesPanel from '@/renderer/components/base/FileChangesPanel';
import { ToolChip } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import type { ToolChipStatus } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import { chipDetailOmittingCommand, resolveToolChipStatus } from '@renderer/components/beautifulUi/toolChips/toolChipModel';
import LocalImageView from '@renderer/components/media/LocalImageView';
import { useDiffPreviewHandlers } from '@/renderer/hooks/file/useDiffPreviewHandlers';
import { parseDiff } from '@/renderer/utils/file/diffUtils';
import { createTwoFilesPatch } from 'diff';
import React, { useMemo, useState } from 'react';
import './MessageToolDetails.css';

const chipStatusFromTool = (
  status: NormalizedToolStatus,
  skipped?: boolean,
  notExecutedReason?: 'invalid_arguments'
): ToolChipStatus => resolveToolChipStatus({ status, skipped, notExecutedReason });

const ReplacePreview: React.FC<{ message: IMessageToolCall }> = ({ message }) => {
  const file_path = toDisplayText(message.content.args?.file_path ?? message.content.input?.file_path);
  const old_string = toDisplayText(message.content.args?.old_string ?? message.content.input?.old_string);
  const new_string = toDisplayText(message.content.args?.new_string ?? message.content.input?.new_string);

  const diffText = useMemo(() => {
    return createTwoFilesPatch(file_path, file_path, old_string, new_string, '', '', { context: 3 });
  }, [file_path, old_string, new_string]);

  const fileInfo = useMemo(() => parseDiff(diffText, file_path), [diffText, file_path]);
  const display_name = file_path.split(/[/\\]/).pop() || file_path;
  const { handleFileClick, handleDiffClick } = useDiffPreviewHandlers({ diffText, display_name, file_path });

  return (
    <div className='flex flex-col gap-6px'>
      <ToolChip
        id={message.id}
        name={toDisplayText(message.content.name, 'replace')}
        detail={file_path}
        status='completed'
      />
      <FileChangesPanel
        title={fileInfo.file_name}
        files={[fileInfo]}
        onFileClick={handleFileClick}
        onDiffClick={handleDiffClick}
        defaultExpanded={true}
      />
    </div>
  );
};

const MessageToolCall: React.FC<{ message: IMessageToolCall }> = ({ message }) => {
  const { name, artifacts = [] } = message.content;
  const [expanded, setExpanded] = useState(false);

  if (name === 'knowledge_search') {
    return <KnowledgeSearchChip message={message} />;
  }

  if ((name === 'replace' || name === 'Edit') && message.content.status === 'completed') {
    return <ReplacePreview message={message} />;
  }

  const normalized = normalizeToolCall(message);
  if (!normalized) {
    return <div className='text-t-primary'>{toDisplayText(name)}</div>;
  }

  const visibleArtifacts = normalized.status === 'completed' ? artifacts : [];
  const hasDetail = Boolean(normalized.input || normalized.output || visibleArtifacts.length > 0);
  const chipStatus = chipStatusFromTool(
    normalized.status,
    normalized.skipped,
    normalized.notExecutedReason
  );

  return (
    <div className='flex flex-col'>
      <ToolChip
        id={normalized.key || message.id}
        name={normalized.name}
        detail={chipDetailOmittingCommand(normalized.name, normalized.description)}
        status={chipStatus}
        expandable={hasDetail}
        expanded={expanded}
        onToggle={hasDetail ? () => setExpanded((value) => !value) : undefined}
      />
      {visibleArtifacts.length > 0 && (
        <div className='tool-artifacts m-l-20px m-t-6px'>
          {visibleArtifacts.map((artifact) => (
            <div className='tool-artifact' key={artifact.id} data-artifact-kind={artifact.kind}>
              {artifact.kind === 'image' ? (
                <LocalImageView
                  src={artifact.path}
                  alt={artifact.relative_path || 'Generated image'}
                  className='tool-artifact-image'
                />
              ) : (
                <div className='tool-artifact-file'>
                  <span>{artifact.kind}</span>
                  <code title={artifact.path}>{artifact.path}</code>
                </div>
              )}
              <code className='tool-artifact-path' title={artifact.path}>
                {artifact.path}
              </code>
            </div>
          ))}
        </div>
      )}
      {expanded && hasDetail && (
        <div className='tool-detail-panel m-l-20px m-t-4px'>
          {normalized.input && (
            <div className='tool-detail-section'>
              <div className='tool-detail-label'>Input</div>
              <pre className='tool-detail-content'>{normalized.input}</pre>
            </div>
          )}
          {normalized.output && (
            <div className='tool-detail-section'>
              <div className='tool-detail-label'>Output</div>
              <pre className='tool-detail-content'>{normalized.output}</pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
};

export default MessageToolCall;
