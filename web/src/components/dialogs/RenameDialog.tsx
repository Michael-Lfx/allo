import { useTranslation } from "react-i18next";
import { useAppStore } from "../../store/appStore";
import { DialogShell } from "./DialogShell";

export function RenameDialog() {
  const { t } = useTranslation();
  const renameFor = useAppStore((s) => s.renameFor);
  const conversations = useAppStore((s) => s.conversations);
  const renameValue = useAppStore((s) => s.renameValue);
  const renameBusy = useAppStore((s) => s.renameBusy);
  const setRenameValue = useAppStore((s) => s.setRenameValue);
  const cancelRename = useAppStore((s) => s.cancelRename);
  const submitRename = useAppStore((s) => s.submitRename);
  if (!renameFor) return null;
  const conversation = conversations.find((item) => item.conversation_id === renameFor) ?? null;
  return (
    <DialogShell onClose={cancelRename} labelledBy="rename-title" titleId="rename-title" title={t("rename.title")}>
      <div className="settings-grid">
        <label className="workspace-path-field">
          {t("rename.nameLabel")}
          <input
            value={renameValue}
            onChange={(event) => setRenameValue(event.target.value)}
            placeholder={t("rename.namePlaceholder")}
            autoFocus
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                void submitRename();
              }
            }}
          />
        </label>
      </div>
      <div className="dialog-actions">
        <button className="quiet-button" type="button" onClick={cancelRename}>{t("common.cancel")}</button>
        <button className="primary-button" type="button" onClick={() => void submitRename()} disabled={renameBusy || !renameValue.trim()}>
          {renameBusy ? t("rename.saving") : t("rename.save")}
        </button>
      </div>
    </DialogShell>
  );
}
