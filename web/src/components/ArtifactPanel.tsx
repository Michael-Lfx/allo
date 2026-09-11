import { useEffect, useState } from "react";
import { Download, FileText, RefreshCw, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { IconButton } from "./IconButton";
import { Markdown } from "./Markdown";
import { useAppStore } from "../store/appStore";
import type { ArtifactPreview } from "../store/appStore";
import { formatRelativeTime } from "../ui/format";

/**
 * W5 artifact panel (doc 19 §3 W5).
 *
 * Scope note: the Artifact protocol (`artifact/list` / `artifact/get`) is
 * defined in doc 05 §8 but explicitly deferred — `capabilities.artifacts` is
 * hard-coded `false` and arbitrary path reads are forbidden (TC-AS-008). So
 * this panel is scoped to the **selected conversation's workspace** and reads
 * the host file service instead (`/api/fs/list` + `/api/fs/read`); per-Run
 * attribution and accept/revert stay with the deferred Artifact phase
 * (deviation D-W5-1 in docs/agent-store/16).
 */
function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MB`;
}

/** Save the previewed text through a blob URL (no server-side download route). */
function downloadPreview(preview: ArtifactPreview): void {
  if (preview.content === null) return;
  const url = URL.createObjectURL(new Blob([preview.content], { type: "text/plain;charset=utf-8" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = preview.name;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  URL.revokeObjectURL(url);
}

export function ArtifactPanel() {
  const { t } = useTranslation();
  const open = useAppStore((s) => s.artifactPanelOpen);
  const root = useAppStore((s) => s.artifactsRoot);
  const files = useAppStore((s) => s.artifacts);
  const loading = useAppStore((s) => s.artifactsLoading);
  const error = useAppStore((s) => s.artifactsError);
  const preview = useAppStore((s) => s.artifactPreview);
  const meta = useAppStore((s) => s.artifactsMeta);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  const close = useAppStore((s) => s.closeArtifactPanel);
  const refresh = useAppStore((s) => s.refreshArtifacts);
  const openPreview = useAppStore((s) => s.openArtifactPreview);
  const closePreview = useAppStore((s) => s.closeArtifactPreview);
  const quoteIntoDraft = useAppStore((s) => s.quoteArtifactIntoDraft);

  const [comment, setComment] = useState("");

  // Single load point: opening the panel, or switching conversation while it
  // stays open, re-scopes the list to the new workspace root.
  useEffect(() => {
    if (!open) return;
    void refresh();
  }, [open, selectedConversationId, refresh]);

  // A fresh file starts with an empty comment box.
  useEffect(() => {
    setComment("");
  }, [preview?.path]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [open, close]);

  if (!open) return null;

  const isMarkdown = preview !== null && /\.(md|markdown)$/i.test(preview.name);
  const previewError = preview?.error === "offline" ? t("artifact.offline") : preview?.error ?? null;

  return (
    <div className="artifact-mask" onClick={close}>
      <aside
        className="artifact-panel"
        role="dialog"
        aria-label={t("artifact.title")}
        onClick={(event) => event.stopPropagation()}
      >
        <header className="artifact-head">
          <div className="artifact-head-text">
            <h2>{t("artifact.title")}</h2>
            <p className="artifact-root" title={root ?? undefined}>
              {root ?? t("artifact.noWorkspace")}
            </p>
          </div>
          <IconButton label={t("artifact.refresh")} onClick={() => void refresh()}>
            <RefreshCw aria-hidden="true" className={loading ? "is-spinning" : undefined} size={15} strokeWidth={1.8} />
          </IconButton>
          <IconButton label={t("artifact.close")} onClick={close}>
            <X aria-hidden="true" size={16} strokeWidth={1.8} />
          </IconButton>
        </header>

        <div className="artifact-body">
          {error && <p className="artifact-alert">{error}</p>}
          {!error && !root && <p className="artifact-hint">{t("artifact.noWorkspace")}</p>}
          {!error && root && loading && files.length === 0 && <p className="artifact-hint">{t("artifact.loading")}</p>}
          {!error && root && !loading && files.length === 0 && <p className="artifact-hint">{t("artifact.empty")}</p>}
          {files.length > 0 && (
            <ul className="artifact-list">
              {files.map((file) => (
                <li key={file.full_path}>
                  <button
                    className={`artifact-row${preview?.path === file.full_path ? " is-active" : ""}`}
                    type="button"
                    title={file.relative_path}
                    onClick={() => void openPreview(file)}
                  >
                    <FileText aria-hidden="true" size={14} strokeWidth={1.7} />
                    <span className="artifact-row-name">{file.name}</span>
                    <span className="artifact-row-path">{file.relative_path}</span>
                    {/* R20：size / MIME / mtime 来自宿主 `/api/fs/metadata`；
                        查不到就整段不渲染（不摆空占位，也不猜）。 */}
                    {meta[file.full_path] && (
                      <span className="artifact-row-meta">
                        {formatBytes(meta[file.full_path]!.size)}
                        {" · "}
                        {meta[file.full_path]!.type || t("artifact.unknownType")}
                        {" · "}
                        {formatRelativeTime(meta[file.full_path]!.last_modified)}
                      </span>
                    )}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        {preview && (
          <section className="artifact-preview">
            <header className="artifact-preview-head">
              <span className="artifact-preview-name" title={preview.path}>
                {preview.name}
              </span>
              {preview.content !== null && (
                <span className="artifact-preview-size">
                  {/* R20：服务端元数据优先；拿不到才退回按文本长度估算（估算值只
                      代表解码后的字符数，不是文件字节数）。 */}
                  {meta[preview.path]
                    ? `${formatBytes(meta[preview.path]!.size)} · ${meta[preview.path]!.type || t("artifact.unknownType")} · ${formatRelativeTime(meta[preview.path]!.last_modified)}`
                    : formatBytes(new Blob([preview.content]).size)}
                </span>
              )}
              <button
                className="artifact-download"
                type="button"
                disabled={preview.content === null}
                onClick={() => downloadPreview(preview)}
              >
                <Download aria-hidden="true" size={13} strokeWidth={1.8} /> {t("artifact.download")}
              </button>
              <IconButton label={t("artifact.closePreview")} onClick={closePreview}>
                <X aria-hidden="true" size={14} strokeWidth={1.8} />
              </IconButton>
            </header>

            {preview.loading && <p className="artifact-hint">{t("artifact.loadingFile")}</p>}
            {!preview.loading && previewError && <p className="artifact-alert">{previewError}</p>}
            {!preview.loading && !previewError && preview.content === null && (
              <p className="artifact-hint">{t("artifact.previewUnavailable")}</p>
            )}
            {preview.content !== null &&
              (isMarkdown ? (
                <div className="artifact-preview-markdown">
                  <Markdown source={preview.content} />
                </div>
              ) : (
                <pre className="artifact-preview-body">{preview.content}</pre>
              ))}

            <div className="artifact-comment">
              <textarea
                className="artifact-comment-input"
                rows={2}
                value={comment}
                placeholder={t("artifact.commentPlaceholder")}
                onChange={(event) => setComment(event.target.value)}
              />
              <button
                className="artifact-comment-send"
                type="button"
                disabled={!comment.trim()}
                onClick={() => {
                  // AC-5: the comment rides the composer draft into the next
                  // turn, so no protocol addition is needed to reference it.
                  quoteIntoDraft(`> [${preview.name}] ${comment.trim()}`);
                  setComment("");
                }}
              >
                {t("artifact.commentQueue")}
              </button>
            </div>
          </section>
        )}
      </aside>
    </div>
  );
}
