import { useMemo } from "react";
import { Folder, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { IconButton } from "../IconButton";
import { useAppStore } from "../../store/appStore";
import { modelChipLabel } from "../../ui/format";
import type { ConversationView, ProviderWithModel } from "../../lib/protocol";

export function NewChatDialog() {
  const { t } = useTranslation();
  const newChatOpen = useAppStore((s) => s.newChatOpen);
  const workspaces = useAppStore((s) => s.workspaces);
  const newChatWorkspaceId = useAppStore((s) => s.newChatWorkspaceId);
  const newChatPath = useAppStore((s) => s.newChatPath);
  const workspaceError = useAppStore((s) => s.workspaceError);
  const workspaceCreating = useAppStore((s) => s.workspaceCreating);
  const conversations = useAppStore((s) => s.conversations);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  const providerId = useAppStore((s) => s.providerId);
  const model = useAppStore((s) => s.model);
  const modelOptions = useAppStore((s) => s.modelOptions);
  const selectedModelKey = useAppStore((s) => s.selectedModelKey);
  const selectNewChatWorkspace = useAppStore((s) => s.selectNewChatWorkspace);
  const setNewChatPath = useAppStore((s) => s.setNewChatPath);
  const closeNewChat = useAppStore((s) => s.closeNewChat);
  const createConversation = useAppStore((s) => s.createConversation);
  if (!newChatOpen) return null;

  const currentModel = useMemo<ProviderWithModel | null>(
    () => {
      const current = conversations.find((item) => item.conversation_id === selectedConversationId) as ConversationView | undefined;
      return current?.model ?? (providerId && model ? { provider_id: providerId, model } : null);
    },
    [conversations, selectedConversationId, providerId, model],
  );
  const modelLabel = modelChipLabel(currentModel, selectedModelKey, modelOptions, t("modelPicker.defaultModel"));

  return (
    <div className="settings-backdrop" role="presentation" onMouseDown={closeNewChat}>
      <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="new-chat-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="dialog-header">
          <div>
            <span className="eyebrow">ALLO APP SERVER</span>
            <h1 id="new-chat-title">新建会话</h1>
          </div>
          <IconButton label={t("newChat.close")} onClick={closeNewChat}><X size={19} strokeWidth={1.7} /></IconButton>
        </div>
        <p className="dialog-intro">{t("newChat.intro")}</p>
        {workspaces.length > 0 && (
          <div className="workspace-picker">
            <span className="workspace-picker-title">{t("newChat.existingWorkspaces")}</span>
            {workspaces.map((workspace) => (
              <button
                type="button"
                key={workspace.workspace_id}
                className={`workspace-option ${newChatWorkspaceId === workspace.workspace_id ? "is-active" : ""}`}
                onClick={() => selectNewChatWorkspace(workspace.workspace_id)}
                title={workspace.canonical_path}
              >
                <Folder aria-hidden="true" size={15} strokeWidth={1.7} />
                <span className="workspace-option-name">{workspace.name}</span>
                <span className="workspace-option-meta">{workspace.canonical_path}</span>
              </button>
            ))}
          </div>
        )}
        <div className="settings-grid">
          <label className="workspace-path-field">
            {t("newChat.pathLabel")} <span>{newChatWorkspaceId ? t("newChat.pathIgnored") : t("newChat.pathHint")}</span>
            <input
              value={newChatPath}
              onChange={(event) => setNewChatPath(event.target.value)}
              placeholder={workspaces.length === 0 ? t("newChat.pathPlaceholderEmpty") : t("newChat.pathPlaceholderOptional")}
              spellCheck={false}
              autoComplete="off"
              disabled={newChatWorkspaceId !== null || workspaceCreating}
            />
          </label>
        </div>
        <p className="dialog-intro">{t("newChat.modelLine", { model: modelLabel || t("modelPicker.defaultModel") })}</p>
        {workspaceError && <div className="workspace-error" role="alert">{workspaceError === "pathRequired" ? t("newChat.pathRequired") : workspaceError}</div>}
        <div className="dialog-actions">
          <button className="quiet-button" type="button" onClick={closeNewChat}>{t("common.cancel")}</button>
          <button className="primary-button" type="button" onClick={() => void createConversation()} disabled={workspaceCreating || (!newChatWorkspaceId && !newChatPath.trim())}>
            {workspaceCreating ? t("newChat.creating") : t("newChat.create")}
          </button>
        </div>
      </section>
    </div>
  );
}
