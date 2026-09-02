import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { IconButton } from "../IconButton";
import { useAppStore } from "../../store/appStore";

export function DeleteDialog() {
  const { t } = useTranslation();
  const deleteFor = useAppStore((s) => s.deleteFor);
  const conversations = useAppStore((s) => s.conversations);
  const deleteBusy = useAppStore((s) => s.deleteBusy);
  const cancelDelete = useAppStore((s) => s.cancelDelete);
  const confirmDelete = useAppStore((s) => s.confirmDelete);
  if (!deleteFor) return null;
  const conversation = conversations.find((item) => item.conversation_id === deleteFor) ?? null;
  const name = conversation?.name || t("common.untitled");
  return (
    <div className="settings-backdrop" role="presentation" onMouseDown={cancelDelete}>
      <section className="settings-dialog" role="dialog" aria-modal="true" aria-labelledby="delete-title" onMouseDown={(event) => event.stopPropagation()}>
        <div className="dialog-header">
          <div>
            <span className="eyebrow">ALLO APP SERVER</span>
            <h1 id="delete-title">{t("delete.title")}</h1>
          </div>
          <IconButton label={t("delete.close")} onClick={cancelDelete}><X size={19} strokeWidth={1.7} /></IconButton>
        </div>
        <p className="dialog-intro">
          {t("delete.confirmPrefix")}
          <strong>“{name}”</strong>
          {t("delete.confirmSuffix")}
          {conversation?.is_processing ? t("delete.processingNote") : ""}
          {t("delete.irreversible")}
        </p>
        {deleteBusy && <p className="dialog-intro">{t("delete.deleting")}</p>}
        <div className="dialog-actions">
          <button className="quiet-button" type="button" onClick={cancelDelete}>{t("common.cancel")}</button>
          <button className="danger-button" type="button" onClick={() => void confirmDelete()} disabled={deleteBusy}>
            {deleteBusy ? t("delete.deleting") : t("delete.confirm")}
          </button>
        </div>
      </section>
    </div>
  );
}
