import { useTranslation } from "react-i18next";
import { useAppStore } from "../../store/appStore";
import { DialogShell } from "./DialogShell";

export function WorkspaceRemoveDialog() {
  const { t } = useTranslation();
  const revokeFor = useAppStore((s) => s.revokeFor);
  const workspaces = useAppStore((s) => s.workspaces);
  const revokeBusy = useAppStore((s) => s.revokeBusy);
  const cancelRevoke = useAppStore((s) => s.cancelRevoke);
  const confirmRevoke = useAppStore((s) => s.confirmRevoke);
  if (!revokeFor) return null;
  const workspace = workspaces.find((item) => item.workspace_id === revokeFor) ?? null;
  const name = workspace?.name || t("common.untitled");
  return (
    <DialogShell onClose={cancelRevoke} labelledBy="workspace-remove-title" titleId="workspace-remove-title" title={t("workspaceRemove.title")}>
      <p className="dialog-intro">
        {t("workspaceRemove.text", { name })}
      </p>
      {revokeBusy && <p className="dialog-intro">{t("workspaceRemove.removing")}</p>}
      <div className="dialog-actions">
        <button className="quiet-button" type="button" onClick={cancelRevoke}>{t("common.cancel")}</button>
        <button className="danger-button" type="button" onClick={() => void confirmRevoke()} disabled={revokeBusy}>
          {revokeBusy ? t("workspaceRemove.removing") : t("workspaceRemove.confirm")}
        </button>
      </div>
    </DialogShell>
  );
}
