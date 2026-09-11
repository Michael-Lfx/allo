import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FileImage } from "lucide-react";

import { useAppStore } from "../store/appStore";
import type { WorkspaceFlatFile } from "../lib/protocol";
import { MAX_ATTACHMENTS, attachmentCandidates } from "../lib/attachments";

/**
 * R15（W10）附件选择面板。
 *
 * 只列**会话工作区**里的文件（`/api/fs/list`，与产物面板同一个宿主面端点）：载体是
 * 路径引用，服务端只接受会话工作区内的真实文件，所以「能从这里选到的」与「发得出去
 * 的」才是同一集合——不给用户一个选完必然被拒的入口。
 *
 * 不可附的文件**照样列出来并说明原因**（运行时拒绝的图片格式 / 非图片），因为把一个
 * 行藏起来只会让人觉得「我的文件不见了」。
 */
export function AttachmentPicker({
  root,
  picked,
  onPick,
  onClose,
}: {
  /** 会话工作区的绝对路径；`null` = 该会话没有工作区。 */
  root: string | null;
  /** 已选中的绝对路径（会在候选里排除，并参与名额计算）。 */
  picked: string[];
  onPick: (paths: string[]) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const client = useAppStore((s) => s.client);
  const [files, setFiles] = useState<WorkspaceFlatFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!client || !root) {
      setFiles([]);
      setFailed(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setFailed(false);
    void client
      .listWorkspaceFiles(root)
      .then((list) => {
        if (!cancelled) setFiles(list);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [client, root]);

  const { pickable, skipped, remaining } = attachmentCandidates(files, picked);
  // 工作区里可能上千个不可附的文件，只列前几个作为「为什么不能附」的示范。
  const shownSkipped = skipped.slice(0, 5);
  const hasWorkspace = root !== null;

  return (
    <div className="composer-attach-pop" role="dialog" aria-label={t("composer.attachTitle")}>
      <div className="composer-attach-head">
        <span>{t("composer.attachTitle")}</span>
        <span className="composer-attach-count">
          {t("composer.attachCount", { count: picked.length, max: MAX_ATTACHMENTS })}
        </span>
      </div>

      {!hasWorkspace && <p className="composer-attach-empty">{t("composer.attachNoWorkspace")}</p>}
      {hasWorkspace && loading && <p className="composer-attach-empty">{t("composer.attachLoading")}</p>}
      {hasWorkspace && !loading && failed && (
        <p className="composer-attach-empty">{t("composer.attachUnavailable")}</p>
      )}
      {hasWorkspace && !loading && !failed && files.length === 0 && (
        <p className="composer-attach-empty">{t("composer.attachEmpty")}</p>
      )}

      {hasWorkspace && !loading && !failed && files.length > 0 && (
        <>
          {pickable.length === 0 && (
            <p className="composer-attach-empty">
              {remaining === 0 ? t("composer.attachLimit", { max: MAX_ATTACHMENTS }) : t("composer.attachNoPickable")}
            </p>
          )}
          <ul className="composer-attach-list">
            {pickable.map((entry) => (
              <li key={entry.path}>
                <button
                  className="composer-attach-item"
                  type="button"
                  title={entry.path}
                  disabled={remaining === 0}
                  onClick={() => onPick([entry.path])}
                >
                  <FileImage aria-hidden="true" size={14} strokeWidth={1.7} />
                  <span className="composer-attach-name">{entry.name}</span>
                </button>
              </li>
            ))}
          </ul>
          {shownSkipped.length > 0 && (
            <>
              <p className="composer-attach-note">
                {t("composer.attachSkipped", { count: skipped.length })}
              </p>
              <ul className="composer-attach-list is-skipped">
                {shownSkipped.map((entry) => (
                  <li key={entry.path}>
                    <span className="composer-attach-item is-disabled" title={entry.path}>
                      <span className="composer-attach-name">{entry.name}</span>
                      <span className="composer-attach-reason">
                        {entry.kind === "rejected-image"
                          ? t("composer.attachRejectedImage", { extension: entry.extension })
                          : t("composer.attachNotImage")}
                      </span>
                    </span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </>
      )}

      <button className="composer-attach-done" type="button" onClick={onClose}>
        {t("composer.attachDone")}
      </button>
    </div>
  );
}
