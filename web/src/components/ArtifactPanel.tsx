import { useEffect, useState } from "react";
import { ArrowUpRight, Check, Download, FileText, RefreshCw, Undo2, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { IconButton } from "./IconButton";
import { Markdown } from "./Markdown";
import { useAppStore } from "../store/appStore";
import type { ArtifactPreview } from "../store/appStore";
import { formatRelativeTime } from "../ui/format";
import { normalizeArtifactKey, type ArtifactOwner } from "../lib/artifact-owners";
import { changeCount, changeKey, type FileChangeOperation } from "../lib/artifact-changes";

/** 归属表不属于当前会话时复用的空表：引用稳定，避免选择器每次返回新对象。 */
const EMPTY_OWNERS: Record<string, ArtifactOwner> = {};

/**
 * W5 artifact panel (doc 19 §3 W5)。
 *
 * Scope note: the Artifact protocol (`artifact/list` / `artifact/get`) is
 * defined in doc 05 §8 but explicitly deferred — `capabilities.artifacts` is
 * hard-coded `false` and arbitrary path reads are forbidden (TC-AS-008). So
 * this panel is scoped to the **selected conversation's workspace** and reads
 * the host file service instead (`/api/fs/list` + `/api/fs/read`)。
 *
 * R20a（2026-09-11）：**按 Run 归属已交付**——归属不由文件服务提供，而是
 * `lib/artifact-owners.ts` 从已加载的 `run/plan` 快照（`attempt.output_files`）
 * 做的纯投影。
 *
 * R20b（2026-09-11）：**接受 / 回退已交付**——仍不碰延后的 Artifact 协议，改用宿主
 * 快照服务（`POST /api/fs/snapshot/*`）：`compare` 出「待处理 / 已接受」两组变更，
 * `stage` = 接受、`discard` = 回退。Artifact 协议本身照旧延后（`05` §8 /
 * TC-AS-008，deviation D-W5-1）。
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
  // R20a：归属按会话累积，只在它属于当前选中会话时才用（否则会贴错会话）。
  const owners = useAppStore((s) =>
    s.artifactOwners.conversationId !== null && s.artifactOwners.conversationId === s.selectedConversationId
      ? s.artifactOwners.byPath
      : EMPTY_OWNERS,
  );
  const focusOwner = useAppStore((s) => s.focusArtifactOwner);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  const close = useAppStore((s) => s.closeArtifactPanel);
  const refresh = useAppStore((s) => s.refreshArtifacts);
  const openPreview = useAppStore((s) => s.openArtifactPreview);
  const closePreview = useAppStore((s) => s.closeArtifactPreview);
  const quoteIntoDraft = useAppStore((s) => s.quoteArtifactIntoDraft);
  // R20b：变更切片（`/api/fs/snapshot/*`）。
  const changes = useAppStore((s) => s.artifactChanges);
  const changesLoading = useAppStore((s) => s.artifactChangesLoading);
  const changesError = useAppStore((s) => s.artifactChangesError);
  const snapshot = useAppStore((s) => s.artifactSnapshot);
  const changeBusy = useAppStore((s) => s.artifactChangeBusy);
  const refreshChanges = useAppStore((s) => s.refreshArtifactChanges);
  const acceptChange = useAppStore((s) => s.acceptArtifactChange);
  const revertChange = useAppStore((s) => s.revertArtifactChange);
  const acceptAllChanges = useAppStore((s) => s.acceptAllArtifactChanges);
  const unstageChange = useAppStore((s) => s.unstageArtifactChange);

  const [comment, setComment] = useState("");
  const [tab, setTab] = useState<"files" | "changes">("files");

  // R20b：显式映射，避免动态拼 i18n 键（类型与检索都更稳）。
  const operationLabel = (operation: FileChangeOperation): string =>
    operation === "create"
      ? t("artifact.changeCreate")
      : operation === "modify"
        ? t("artifact.changeModify")
        : t("artifact.changeDelete");

  // Single load point: opening the panel, or switching conversation while it
  // stays open, re-scopes both the file list and the change list to the new
  // workspace root. `refresh` sets `artifactsRoot` synchronously, so the change
  // fetch immediately below always reads the right root.
  useEffect(() => {
    if (!open) return;
    void refresh();
    void refreshChanges();
  }, [open, selectedConversationId, refresh, refreshChanges]);

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

        <div className="artifact-tabs" role="tablist">
          <button
            className={`artifact-tab${tab === "files" ? " is-active" : ""}`}
            type="button"
            role="tab"
            aria-selected={tab === "files"}
            onClick={() => setTab("files")}
          >
            {t("artifact.tabFiles")}
          </button>
          <button
            className={`artifact-tab${tab === "changes" ? " is-active" : ""}`}
            type="button"
            role="tab"
            aria-selected={tab === "changes"}
            onClick={() => setTab("changes")}
          >
            {t("artifact.tabChanges", { count: changeCount(changes) })}
          </button>
        </div>

        {tab === "files" && (
        <div className="artifact-body">
          {error && <p className="artifact-alert">{error}</p>}
          {!error && !root && <p className="artifact-hint">{t("artifact.noWorkspace")}</p>}
          {!error && root && loading && files.length === 0 && <p className="artifact-hint">{t("artifact.loading")}</p>}
          {!error && root && !loading && files.length === 0 && <p className="artifact-hint">{t("artifact.empty")}</p>}
          {files.length > 0 && (
            <ul className="artifact-list">
              {files.map((file) => {
                // R20a：归属来自 `run/plan` 快照投影（不是猜的）。没有就说没有——
                // 不渲染「未知归属」的空标签。
                const ownerKey = normalizeArtifactKey(file.relative_path);
                const owner = ownerKey ? owners[ownerKey] : undefined;
                return (
                  <li className="artifact-item" key={file.full_path}>
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
                    {/* R20a：点它 = 关抽屉 + 跟随该 Run + 滚到那一步的锚点
                        （`RunDetail` 的待办行同锚点，见 `lib/run-plan.ts`）。 */}
                    {owner && (
                      <button
                        className="artifact-row-owner"
                        type="button"
                        title={t("artifact.ownerTitle", { run: owner.runId, attempt: owner.attemptNo })}
                        onClick={() => void focusOwner(owner)}
                      >
                        <ArrowUpRight aria-hidden="true" size={11} strokeWidth={1.9} />
                        {t("artifact.ownerLabel", { step: owner.stepTitle })}
                      </button>
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
        )}

        {tab === "changes" && (
          <div className="artifact-body">
            {snapshot?.mode === "disabled" ? (
              // 后端明确说「这个工作区不跟踪」——照实说明原因，不做禁用占位。
              <p className="artifact-hint">{t("artifact.snapshotDisabled", { reason: snapshot.reason ?? "" })}</p>
            ) : (
              <>
                {changesError && <p className="artifact-alert">{changesError}</p>}
                {changesLoading && changeCount(changes) === 0 && (
                  <p className="artifact-hint">{t("artifact.changesLoading")}</p>
                )}
                {!changesLoading && !changesError && changeCount(changes) === 0 && (
                  <p className="artifact-hint">{t("artifact.changesEmpty")}</p>
                )}
                {changes.unstaged.length > 0 && (
                  <>
                    <div className="artifact-changes-head">
                      <span>{t("artifact.changesPending", { count: changes.unstaged.length })}</span>
                      <button
                        className="artifact-changes-all"
                        type="button"
                        disabled={changeBusy !== null}
                        onClick={() => void acceptAllChanges()}
                      >
                        {t("artifact.acceptAll")}
                      </button>
                    </div>
                    <ul className="artifact-list">
                      {changes.unstaged.map((change) => (
                        <li key={changeKey(change)}>
                          <div className="artifact-change-row">
                            <span className={`artifact-change-op is-${change.operation}`}>
                              {operationLabel(change.operation)}
                            </span>
                            <span className="artifact-change-path" title={change.relative_path}>
                              {change.relative_path}
                            </span>
                            <button
                              className="artifact-change-action"
                              type="button"
                              disabled={changeBusy !== null}
                              onClick={() => void acceptChange(change)}
                            >
                              <Check aria-hidden="true" size={12} strokeWidth={2} /> {t("artifact.accept")}
                            </button>
                            <button
                              className="artifact-change-action is-revert"
                              type="button"
                              disabled={changeBusy !== null}
                              onClick={() => void revertChange(change)}
                            >
                              <Undo2 aria-hidden="true" size={12} strokeWidth={2} /> {t("artifact.revert")}
                            </button>
                          </div>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
                {changes.staged.length > 0 && (
                  <>
                    <div className="artifact-changes-head">
                      <span>{t("artifact.changesStaged", { count: changes.staged.length })}</span>
                    </div>
                    <ul className="artifact-list">
                      {changes.staged.map((change) => (
                        <li key={changeKey(change)}>
                          <div className="artifact-change-row">
                            <span className={`artifact-change-op is-${change.operation}`}>
                              {operationLabel(change.operation)}
                            </span>
                            <span className="artifact-change-path" title={change.relative_path}>
                              {change.relative_path}
                            </span>
                            <button
                              className="artifact-change-action"
                              type="button"
                              disabled={changeBusy !== null}
                              onClick={() => void unstageChange(change)}
                            >
                              <Undo2 aria-hidden="true" size={12} strokeWidth={2} /> {t("artifact.unstage")}
                            </button>
                          </div>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </>
            )}
          </div>
        )}

        {tab === "files" && preview && (
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
