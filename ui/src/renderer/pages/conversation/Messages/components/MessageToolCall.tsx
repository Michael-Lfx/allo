
import type { IMessageToolCall } from '@/common/chat/chatLib';
import { toDisplayText } from '@/common/chat/displayText';
import { normalizeToolCall } from '@/common/chat/normalizeToolCall';
import type { NormalizedToolStatus } from '@/common/chat/normalizeToolCall';
import KnowledgeSearchChip from './KnowledgeSearchChip';
import { ToolChip } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import type { ToolChipStatus } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import { chipDetailOmittingCommand, resolveToolChipStatus } from '@renderer/components/beautifulUi/toolChips/toolChipModel';
import LocalImageView from '@renderer/components/media/LocalImageView';
import React, { useMemo, useState } from 'react';
import { buildEditDiffPreview } from './buildEditDiff';
import ToolEditDiff from './ToolEditDiff';
import './MessageToolDetails.css';

const chipStatusFromTool = (
  status: NormalizedToolStatus,
  skipped?: boolean,
  notExecutedReason?: 'invalid_arguments',
  nonFatalFailure?: boolean
): ToolChipStatus => resolveToolChipStatus({ status, skipped, notExecutedReason, nonFatalFailure });

const MessageToolCall: React.FC<{ message: IMessageToolCall }> = ({ message }) => {
  const { name, artifacts = [] } = message.content;
  const [expanded, setExpanded] = useState(false);
  const preview = useMemo(
    () => buildEditDiffPreview(message.content.args ?? message.content.input),
    [message.content.args, message.content.input]
  );

  if (name === 'knowledge_search') {
    return <KnowledgeSearchChip message={message} />;
  }

  const normalized = normalizeToolCall(message);
  if (!normalized) {
    return <div className='text-t-primary'>{toDisplayText(name)}</div>;
  }

  const visibleArtifacts = normalized.status === 'completed' ? artifacts : [];
  const chipStatus = chipStatusFromTool(
    normalized.status,
    normalized.skipped,
    normalized.notExecutedReason,
    normalized.nonFatalFailure
  );
  const chipDetail = preview?.filePath || chipDetailOmittingCommand(normalized.name, normalized.description);
  const showFailureOutput =
    Boolean(normalized.output) &&
    (chipStatus === 'error' ||
      chipStatus === 'canceled' ||
      Boolean(normalized.skipped || normalized.notExecutedReason));
  const hasDetail = !preview && Boolean(normalized.input || normalized.output || visibleArtifacts.length > 0);

  return (
    <div className='flex flex-col gap-6px'>
      <ToolChip
        id={normalized.key || message.id}
        name={normalized.name}
        detail={chipDetail}
        status={chipStatus}
        expandable={hasDetail}
        expanded={expanded}
        onToggle={hasDetail ? () => setExpanded((value) => !value) : undefined}
      />
      {preview ? <ToolEditDiff preview={preview} /> : null}
      {preview && showFailureOutput ? (
        <div className='tool-detail-panel m-l-20px'>
          <pre className='tool-detail-content'>{normalized.output}</pre>
        </div>
      ) : null}
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
