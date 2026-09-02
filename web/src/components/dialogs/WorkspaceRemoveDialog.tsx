import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { IconButton } from "../IconButton";
import { useAppStore } from "../../store/appStore";

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
    <div className="settings-backdrop" role="presentation" onMouseDown={cancelRevoke}>
      <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="workspace-remove-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="dialog-header">
          <div>
            <span className="eyebrow">ALLO APP SERVER</span>
            <h1 id="workspace-remove-title">{t("workspaceRemove.title")}</h1>
          </div>
          <IconButton label={t("workspaceRemove.close")} onClick={cancelRevoke}><X size={19} strokeWidth={1.7} /></IconButton>
        </div>
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
      </section>
    </div>
  );
}
