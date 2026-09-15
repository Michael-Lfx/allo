import { useTranslation } from "react-i18next";

import { conversationStatusTag } from "../lib/conversation-status";
import { followedRunStatus } from "../lib/conversation-status";
import type { RunEvent } from "../lib/protocol";

/**
 * The status tag on a sidebar conversation row.
 *
 * Renders `null` for a quiet row (only active conversations carry a tag), so
 * the caller can drop it in unconditionally. The label goes through
 * `run.statusValue.*` with the raw protocol value as the fallback — an
 * unrecognized status shows itself rather than pretending to be something else
 * (the same rule `RunDetail` follows).
 */
export function ConversationStatusTag({
  conversationId,
  isProcessing,
  runConversationId,
  activeRunId,
  runEvents,
}: {
  conversationId: string;
  isProcessing: boolean;
  runConversationId: string | null;
  activeRunId: string | null;
  runEvents: RunEvent[];
}) {
  const { t } = useTranslation();
  const tag = conversationStatusTag({
    isProcessing,
    runStatus: followedRunStatus(conversationId, runConversationId, activeRunId, runEvents),
  });
  if (!tag) return null;

  const label = t(tag.labelKey, { defaultValue: tag.labelKey });
  return (
    <span className={`conversation-status-tag is-${tag.tone}`} title={label}>
      {label}
    </span>
  );
}
