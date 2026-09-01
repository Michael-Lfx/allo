

import { ipcBridge } from '@/common';
import { parseConfirmationCorrelationId, type IMessageToolGroup } from '@/common/chat/chatLib';
import { optionalDisplayText, toDisplayText } from '@/common/chat/displayText';
import { iconColors } from '@/renderer/styles/colors';
import { Button, Tooltip } from '@arco-design/web-react';
import { Download, LoadingOne } from '@icon-park/react';
import ApprovalCard from '@renderer/components/beautifulUi/approvalCard/ApprovalCard';
import { kindFromConfirmationType } from '@renderer/components/beautifulUi/approvalCard/approvalCardModel';
import { ToolChip } from '@renderer/components/beautifulUi/toolChips/ToolChips';
import { resolveToolChipStatusFromToolGroup } from '@renderer/components/beautifulUi/toolChips/toolChipModel';
import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import FeedbackButton from '@/renderer/components/base/FeedbackButton';
import FileChangesPanel from '@/renderer/components/base/FileChangesPanel';
import { useDiffPreviewHandlers } from '@/renderer/hooks/file/useDiffPreviewHandlers';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { parseDiff } from '@/renderer/utils/file/diffUtils';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import MessageFileChanges from '../MessageFileChanges';
import { BriefingToolCard, isBriefingToolName } from './BriefingToolCard';
import CollapsibleContent from '@renderer/components/chat/CollapsibleContent';
import LocalImageView from '@renderer/components/media/LocalImageView';
import MarkdownView from '@renderer/components/Markdown';
import { ToolConfirmationOutcome } from '@renderer/utils/common';
import { COLLAPSE_CONFIG, TEXT_CONFIG } from '../constants';
import { MESSAGE_BODY_FONT_SIZE, MESSAGE_BODY_LINE_HEIGHT } from '../typography';
import type { VideoGenerationResult, WriteFileResult } from '../types';
import {
  enforceToolGroupArtifactTrust,
  getSuccessfulLegacyImage,
  isSuccessfulWriteFileResult,
} from './toolGroupArtifactVisibility';

const CODE_STYLE = { marginTop: 4, marginBottom: 4 };

const VIDEO_TOOL_NAMES = new Set(['VideoGeneration', 'video_generate']);

const resolveVideoUrl = (result: VideoGenerationResult): string | undefined => {
  if (result.video) return result.video;
  const asset = result.assets?.find((a) => a.kind === 'video' || a.url || a.local_path);
  return asset?.url ?? asset?.local_path;
};

const isVideoGenerationTool = (name: string) => VIDEO_TOOL_NAMES.has(name);

// CollapsibleContent 高度常量 CollapsibleContent height constants
const RESULT_MAX_HEIGHT = COLLAPSE_CONFIG.MAX_HEIGHT;

interface IMessageToolGroupProps {
  message: IMessageToolGroup;
}

const useConfirmationButtons = (
  confirmationDetails: IMessageToolGroupProps['message']['content'][number]['confirmationDetails'],
  t: (key: string, options?: any) => string
) => {
  return useMemo(() => {
    if (!confirmationDetails) return {};
    let question: string;
    const options: Array<{ label: string; value: ToolConfirmationOutcome }> = [];
    switch (confirmationDetails.type) {
      case 'edit':
        {
          question = t('messages.confirmation.applyChange');
          options.push(
            {
              label: t('messages.confirmation.yesAllowOnce'),
              value: ToolConfirmationOutcome.ProceedOnce,
            },
            {
              label: t('messages.confirmation.yesAllowAlways'),
              value: ToolConfirmationOutcome.ProceedAlways,
            },
            { label: t('messages.confirmation.no'), value: ToolConfirmationOutcome.Cancel }
          );
        }
        break;
      case 'exec':
        {
          question = t('messages.confirmation.allowExecution');
          options.push(
            {
              label: t('messages.confirmation.yesAllowOnce'),
              value: ToolConfirmationOutcome.ProceedOnce,
            },
            {
              label: t('messages.confirmation.yesAllowAlways'),
              value: ToolConfirmationOutcome.ProceedAlways,
            },
            { label: t('messages.confirmation.no'), value: ToolConfirmationOutcome.Cancel }
          );
        }
        break;
      case 'info':
        {
          question = t('messages.confirmation.proceed');
          options.push(
            {
              label: t('messages.confirmation.yesAllowOnce'),
              value: ToolConfirmationOutcome.ProceedOnce,
            },
            {
              label: t('messages.confirmation.yesAllowAlways'),
              value: ToolConfirmationOutcome.ProceedAlways,
            },
            { label: t('messages.confirmation.no'), value: ToolConfirmationOutcome.Cancel }
          );
        }
        break;
      default: {
        const mcpProps = confirmationDetails;
        question = t('messages.confirmation.allowMCPTool', {
          toolName: toDisplayText(mcpProps.tool_name),
          serverName: toDisplayText(mcpProps.server_name),
        });
        options.push(
          {
            label: t('messages.confirmation.yesAllowOnce'),
            value: ToolConfirmationOutcome.ProceedOnce,
          },
          {
            label: t('messages.confirmation.yesAlwaysAllowTool', {
              toolName: toDisplayText(mcpProps.tool_name),
              serverName: toDisplayText(mcpProps.server_name),
            }),
            value: ToolConfirmationOutcome.ProceedAlwaysTool,
          },
          {
            label: t('messages.confirmation.yesAlwaysAllowServer', {
              serverName: toDisplayText(mcpProps.server_name),
            }),
            value: ToolConfirmationOutcome.ProceedAlwaysServer,
          },
          { label: t('messages.confirmation.no'), value: ToolConfirmationOutcome.Cancel }
        );
      }
    }
    return {
      question,
      options,
    };
  }, [confirmationDetails, t]);
};

const EditConfirmationDiff: React.FC<{ diff: string; file_name: string; title: string }> = ({
  diff,
  file_name,
  title,
}) => {
  const fileInfo = useMemo(() => parseDiff(diff, file_name), [diff, file_name]);
  const display_name = file_name.split(/[/\\]/).pop() || file_name;
  const { handleFileClick, handleDiffClick } = useDiffPreviewHandlers({
    diffText: diff,
    display_name,
    file_path: file_name,
    title,
  });

  return (
    <FileChangesPanel
      title={title}
      files={[fileInfo]}
      onFileClick={handleFileClick}
      onDiffClick={handleDiffClick}
      defaultExpanded={true}
    />
  );
};

const ConfirmationDetails: React.FC<{
  content: IMessageToolGroupProps['message']['content'][number];
  onConfirm: (outcome: ToolConfirmationOutcome) => void;
  readOnly?: boolean;
}> = ({ content, onConfirm, readOnly }) => {
  const { t } = useTranslation();
  const { confirmationDetails } = content;
  if (!confirmationDetails) return;
  const node = useMemo(() => {
    if (!confirmationDetails) return null;
    switch (confirmationDetails.type) {
      case 'edit':
        return null; // Rendered separately below with hooks support
      case 'exec': {
        const bashSnippet = `\`\`\`bash\n${toDisplayText(confirmationDetails.command)}\n\`\`\``;
        return (
          <div className='w-full max-w-100% min-w-0'>
            <MarkdownView codeStyle={CODE_STYLE} fontSize={MESSAGE_BODY_FONT_SIZE} lineHeight={MESSAGE_BODY_LINE_HEIGHT}>
              {bashSnippet}
            </MarkdownView>
          </div>
        );
      }
      case 'info':
        return <span className='text-t-primary'>{toDisplayText(confirmationDetails.prompt)}</span>;
      case 'mcp':
        return <span className='text-t-primary'>{toDisplayText(confirmationDetails.tool_display_name)}</span>;
      default: {
        const exhaustive: never = confirmationDetails;
        return exhaustive;
      }
    }
  }, [confirmationDetails]);

  const { question = '', options = [] } = useConfirmationButtons(confirmationDetails, t);

  const [selected, setSelected] = useState<ToolConfirmationOutcome | null>(null);

  const isConfirm = content.status === 'Confirming';
  const details =
    confirmationDetails.type === 'edit' ? (
      <EditConfirmationDiff
        diff={toDisplayText(confirmationDetails?.file_diff)}
        file_name={toDisplayText(confirmationDetails.file_name)}
        title={isConfirm ? toDisplayText(confirmationDetails.title) : toDisplayText(content.description)}
      />
    ) : (
      node
    );

  if (!readOnly && content.status === 'Confirming') {
    return (
      <ApprovalCard
        title={question}
        kind={kindFromConfirmationType(confirmationDetails.type)}
        options={options.map((item) => ({ id: item.value, label: item.label }))}
        selectedId={selected}
        onSelect={(id) => setSelected(id as ToolConfirmationOutcome)}
        onConfirm={() => {
          // The button is disabled while `selected` is null, so this guard
          // never blocks a real click — it only narrows the type for onConfirm.
          if (selected) onConfirm(selected);
        }}
        confirmLabel={t('messages.confirm')}
      >
        {details}
      </ApprovalCard>
    );
  }

  return <div>{details}</div>;
};

// Legacy tool-group image display. LocalImageView owns source-generation
// cancellation and stale/error clearing for local, remote, data and blob URLs.
const ImageDisplay: React.FC<{
  imgUrl: string;
  relativePath?: string;
}> = ({ imgUrl, relativePath }) => (
  <div className='my-8px' style={{ maxWidth: '197px' }}>
    <LocalImageView
      src={imgUrl}
      alt={relativePath || 'Generated image'}
      className='block max-w-full max-h-320px object-contain rd-8px'
    />
  </div>
);

// VideoDisplay: 视频生成结果展示组件 Video generation result display component
const VideoDisplay: React.FC<{
  videoUrl: string;
  relativePath?: string;
}> = ({ videoUrl, relativePath }) => {
  const { t } = useTranslation();
  const [messageApi, messageContext] = useArcoMessage();
  const [resolvedUrl, setResolvedUrl] = useState(videoUrl);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);

  React.useEffect(() => {
    if (videoUrl.startsWith('http') || videoUrl.startsWith('blob:') || videoUrl.startsWith('data:')) {
      setResolvedUrl(videoUrl);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(false);
    ipcBridge.fs.getImageBase64
      .invoke({ path: videoUrl })
      .then((base64) => {
        if (!base64) throw new Error('Video file not found');
        setResolvedUrl(base64);
        setLoading(false);
      })
      .catch((err) => {
        console.error('Failed to load video:', err);
        setError(true);
        setLoading(false);
      });
  }, [videoUrl]);

  const handleDownload = useCallback(async () => {
    try {
      const response = await fetch(resolvedUrl);
      const blob = await response.blob();
      const file_name = relativePath?.split(/[\\/]/).pop() || 'video.mp4';
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = file_name;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
      messageApi.success(t('messages.downloadSuccess', { defaultValue: 'Download successful' }));
    } catch (err) {
      console.error('Failed to download video:', err);
      messageApi.error(t('messages.downloadFailed', { defaultValue: 'Failed to download' }));
    }
  }, [resolvedUrl, relativePath, t, messageApi]);

  if (loading) {
    return (
      <div className='flex items-center gap-8px my-8px'>
        <LoadingOne className='loading' theme='outline' size='14' fill={iconColors.primary} />
        <span className='text-t-secondary text-sm'>{t('common.loading', { defaultValue: 'Loading...' })}</span>
      </div>
    );
  }

  if (error || !resolvedUrl) {
    return (
      <div className='flex items-center gap-8px my-8px text-t-secondary text-sm'>
        <span>{t('messages.videoLoadFailed', { defaultValue: 'Failed to load video' })}</span>
      </div>
    );
  }

  return (
    <>
      {messageContext}
      <div className='flex flex-col gap-8px my-8px' style={{ maxWidth: '360px' }}>
        <video src={resolvedUrl} controls className='max-w-100% rd-8px' />
        <div className='flex gap-8px'>
          <Tooltip content={t('common.download', { defaultValue: 'Download' })}>
            <Button
              type='secondary'
              size='small'
              shape='circle'
              icon={<Download theme='outline' size='14' fill={iconColors.primary} />}
              onClick={handleDownload}
            />
          </Tooltip>
        </div>
      </div>
    </>
  );
};

const ToolResultDisplay: React.FC<{
  content: IMessageToolGroupProps['message']['content'][number];
}> = ({ content }) => {
  const { result_display, name } = content;
  const toolName = toDisplayText(name);

  // 图片生成特殊处理 Special handling for image generation
  const successfulImage = getSuccessfulLegacyImage(content);
  if (toolName === 'ImageGeneration' && successfulImage) {
    return (
      <LocalImageView
        src={successfulImage.imgUrl}
        alt={successfulImage.relativePath || successfulImage.imgUrl}
        className='max-w-100% max-h-100%'
      />
    );
  }

  // 视频生成特殊处理 Special handling for video generation
  if (isVideoGenerationTool(name) && typeof result_display === 'object') {
    const result = result_display as VideoGenerationResult;
    const videoUrl = resolveVideoUrl(result);
    if (videoUrl) {
      return <VideoDisplay videoUrl={videoUrl} relativePath={result.local_path} />;
    }
  }

  if (isBriefingToolName(toolName)) {
    const card = <BriefingToolCard result={result_display} />;
    if (card) return card;
  }

  // 将结果转换为字符串 Convert result to string
  const display = toDisplayText(result_display);

  // 使用 CollapsibleContent 包装长内容
  // Wrap long content with CollapsibleContent
  return (
    <CollapsibleContent maxHeight={RESULT_MAX_HEIGHT} defaultCollapsed={true} useMask={false}>
      <pre
        className='text-t-primary whitespace-pre-wrap break-words m-0'
        style={{ fontSize: `${TEXT_CONFIG.FONT_SIZE}px`, lineHeight: TEXT_CONFIG.LINE_HEIGHT }}
      >
        {display}
      </pre>
    </CollapsibleContent>
  );
};

const MessageToolGroup: React.FC<IMessageToolGroupProps> = ({ message }) => {
  const { t } = useTranslation();
  const readOnly = useConversationContextSafe()?.readOnly === true;
  const toolContent = useMemo(
    () =>
      Array.isArray(message.content)
        ? message.content.map(enforceToolGroupArtifactTrust)
        : [],
    [message.content]
  );

  // 收集所有 WriteFile 结果用于汇总显示 / Collect all WriteFile results for summary display
  const writeFileResults = useMemo(() => {
    return toolContent
      .filter(
        (item) => isSuccessfulWriteFileResult(item)
      )
      .map((item) => {
        const result = item.result_display as WriteFileResult;
        return {
          file_diff: toDisplayText(result.file_diff),
          file_name: toDisplayText(result.file_name),
        };
      });
  }, [toolContent]);

  // 找到第一个 WriteFile 的索引 / Find the index of first WriteFile
  const firstWriteFileIndex = useMemo(() => {
    return toolContent.findIndex(
      (item) => isSuccessfulWriteFileResult(item)
    );
  }, [toolContent]);

  return (
    <div>
      {toolContent.map((content, index) => {
        const { status, call_id, name, description, result_display, confirmationDetails } = content;
        const statusText = toDisplayText(status);
        const callIdText = toDisplayText(call_id, `tool-${index}`);
        const nameText = toDisplayText(name, 'Tool');
        const descriptionText = optionalDisplayText(description);
        // status === "Confirming" &&
        if (confirmationDetails) {
          return (
            <ConfirmationDetails
              key={callIdText}
              content={content}
              readOnly={readOnly}
              onConfirm={(outcome) => {
                if (readOnly) return;
                ipcBridge.conversation.confirmMessage
                  .invoke({
                    confirm_key: outcome,
                    msg_id: message.msg_id ?? parseConfirmationCorrelationId(callIdText),
                    call_id: callIdText,
                    conversation_id: message.conversation_id,
                  })
                  .then(() => {
                    // confirmation sent successfully
                  })
                  .catch((error) => {
                    console.error('Failed to confirm message:', error);
                  });
              }}
            ></ConfirmationDetails>
          );
        }

        // WriteFile 特殊处理：使用 MessageFileChanges 汇总显示 / WriteFile special handling: use MessageFileChanges for summary display
        if (statusText === 'Success' && nameText === 'WriteFile' && typeof result_display !== 'string') {
          if (result_display && typeof result_display === 'object' && 'file_diff' in result_display) {
            // 只在第一个 WriteFile 位置显示汇总组件 / Only show summary component at first WriteFile position
            if (index === firstWriteFileIndex && writeFileResults.length > 0) {
              return (
                <div className='w-full min-w-0' key={callIdText}>
                  <MessageFileChanges writeFileChanges={writeFileResults} />
                </div>
              );
            }
            // 跳过其他 WriteFile / Skip other WriteFile
            return null;
          }
        }

        // ImageGeneration 特殊处理：单独展示图片，不用 Alert 包裹 Special handling for ImageGeneration: display image separately without Alert wrapper
        const successfulImage = getSuccessfulLegacyImage(content);
        if (successfulImage) {
          return (
            <ImageDisplay
              key={callIdText}
              imgUrl={successfulImage.imgUrl}
              relativePath={successfulImage.relativePath}
            />
          );
        }

        // VideoGeneration 特殊处理：单独展示视频 Special handling for VideoGeneration: display video separately without Alert wrapper
        if (isVideoGenerationTool(name) && typeof result_display === 'object') {
          const result = result_display as VideoGenerationResult;
          const videoUrl = resolveVideoUrl(result);
          if (videoUrl) {
            return <VideoDisplay key={call_id} videoUrl={videoUrl} relativePath={result.local_path} />;
          }
        }

        if (isBriefingToolName(nameText) && result_display) {
          return (
            <div key={callIdText}>
              <ToolChip
                id={callIdText}
                name={nameText}
                detail={statusText === 'Canceled' ? t('messages.canceledExecution') : undefined}
                status={resolveToolChipStatusFromToolGroup(status)}
              />
              <div className='mt-8px'>
                <BriefingToolCard result={result_display} />
              </div>
            </div>
          );
        }

        // 通用工具调用展示 Generic tool call display
        return (
          <div key={callIdText}>
            <ToolChip
              id={callIdText}
              name={nameText}
              detail={statusText === 'Canceled' ? t('messages.canceledExecution') : undefined}
              status={resolveToolChipStatusFromToolGroup(status)}
            />

            {(descriptionText || result_display || statusText === 'Error') && (
              <div className='mt-8px'>
                {descriptionText && (
                  <div
                    className={`text-12px text-t-secondary mb-2 ${statusText === 'Error' ? 'whitespace-pre-wrap break-words' : 'truncate'}`}
                  >
                    {descriptionText}
                  </div>
                )}
                {result_display && (
                  <div>
                    {/* ToolResultDisplay 内部已包含 CollapsibleContent，避免嵌套 */}
                    <ToolResultDisplay content={content} />
                  </div>
                )}
                {statusText === 'Error' && (
                  <div className='mt-4px flex justify-end'>
                    <FeedbackButton />
                  </div>
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
};

export default MessageToolGroup;
