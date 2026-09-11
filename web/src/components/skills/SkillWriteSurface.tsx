/**
 * W12 skill management surface (`16` R17) — the pieces that render.
 *
 * Two halves, same split as every other renderable surface in this app:
 *
 * - the `*View` components are pure props → markup, so a server render can
 *   assert exactly what the UI would show (including the states that must NOT
 *   offer a write: read-only origins, a failed write, an offline host);
 * - {@link SkillWriteDialogs} is the wired half: it owns the draft state, calls
 *   the {@link useSkillAdmin} store and asks the caller to refresh its list.
 *
 * The write actions appear on `writable === true` only. `writable` is the
 * server's own predicate (`origin === "user"` **and** the canonical directory),
 * so the UI cannot offer a write the write face would refuse; when it is not
 * true the row says *why* instead of hiding the fact (`16` R17: 只读来源不给写
 * 按钮并给出原因).
 */

import { useEffect, useState } from "react";
import { Copy, Pencil, Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SkillSummary } from "@flowy-agent-store/protocol";
import { DialogShell } from "../dialogs/DialogShell";
import type {
  SkillCreateInput,
  SkillOriginWire,
  SkillUpdateInput,
} from "../../lib/client";
import { useSkillAdmin } from "../../store/skillAdmin";
import type { SkillAdminClient } from "../../store/skillAdmin";

/**
 * Why a skill cannot be edited here, as an i18n key — derived from the origin
 * the read face reported, never from guesswork.
 *
 * A missing/unknown origin is *not* writable either: an older host that predates
 * the field must not make the UI offer a write it cannot justify.
 */
export function readOnlyReasonI18nKey(origin: SkillOriginWire | undefined): string {
  switch (origin) {
    case "builtin":
      return "skills.reasonBuiltin";
    case "marketplace":
      return "skills.reasonMarketplace";
    case "shared":
      return "skills.reasonShared";
    case "companion":
      return "skills.reasonCompanion";
    case "draft":
      return "skills.reasonDraft";
    case "unmanaged":
      return "skills.reasonUnmanaged";
    default:
      return "skills.reasonUnknown";
  }
}

/** Origin label key for the badge on every row. */
export function originLabelI18nKey(origin: SkillOriginWire | undefined): string {
  switch (origin) {
    case "user":
      return "skills.originUser";
    case "builtin":
      return "skills.originBuiltin";
    case "marketplace":
      return "skills.originMarketplace";
    case "shared":
      return "skills.originShared";
    case "companion":
      return "skills.originCompanion";
    case "draft":
      return "skills.originDraft";
    default:
      return "skills.originUnknown";
  }
}

export interface SkillWriteActionsViewProps {
  skill: Pick<SkillSummary, "id" | "writable" | "origin">;
  /** A write is in flight for this skill (or for the whole panel). */
  busy: boolean;
  onEdit: (skillId: string) => void;
  onCopy: (skillId: string) => void;
  onDelete: (skillId: string) => void;
}

/**
 * The per-row write affordances.
 *
 * `writable !== true` renders the reason only — no disabled buttons, because a
 * greyed-out control still reads as "this exists but you can't", while the truth
 * is "this skill belongs to another flow".
 */
export function SkillWriteActionsView({
  skill,
  busy,
  onEdit,
  onCopy,
  onDelete,
}: SkillWriteActionsViewProps) {
  const { t } = useTranslation();

  if (skill.writable !== true) {
    return (
      <span className="skill-write-reason">
        {t("skills.readOnly", { reason: t(readOnlyReasonI18nKey(skill.origin)) })}
      </span>
    );
  }

  return (
    <span className="skill-write-actions">
      <button
        className="quiet-button"
        type="button"
        disabled={busy}
        onClick={() => onEdit(skill.id)}
      >
        <Pencil size={14} strokeWidth={1.7} />
        {t("skills.edit")}
      </button>
      <button
        className="quiet-button"
        type="button"
        disabled={busy}
        onClick={() => onCopy(skill.id)}
      >
        <Copy size={14} strokeWidth={1.7} />
        {t("skills.copy")}
      </button>
      <button
        className="quiet-button is-destructive"
        type="button"
        disabled={busy}
        onClick={() => onDelete(skill.id)}
      >
        <Trash2 size={14} strokeWidth={1.7} />
        {t("skills.delete")}
      </button>
    </span>
  );
}

/** Draft of the create/edit form: every field is a string (empty = unset). */
export interface SkillFormDraft {
  name: string;
  description: string;
  when_to_use: string;
  allowed_tools: string;
  paths: string;
  body: string;
}

export const EMPTY_SKILL_DRAFT: SkillFormDraft = {
  name: "",
  description: "",
  when_to_use: "",
  allowed_tools: "",
  paths: "",
  body: "",
};

/**
 * Turn the form draft into the wire patch for `mode`.
 *
 * Two deliberate rules:
 * - `create` sends the optional fields **only when filled** (the server
 *   assembles the frontmatter; an empty `when-to-use` line would be noise);
 * - `edit` sends **only the fields the user touched**, which is why it takes the
 *   baseline draft as well: an untouched field stays `undefined` and is left
 *   alone server-side, while an emptied optional field becomes `""` and clears
 *   the key. The body is sent only when it differs from the baseline, because
 *   the read face never returned it — see the dialog's own prose.
 */
export function buildSkillCreateInput(draft: SkillFormDraft): SkillCreateInput {
  const input: SkillCreateInput = {
    name: draft.name.trim(),
    description: draft.description.trim(),
  };
  if (draft.when_to_use.trim()) input.when_to_use = draft.when_to_use.trim();
  if (draft.allowed_tools.trim()) input.allowed_tools = draft.allowed_tools.trim();
  if (draft.paths.trim()) input.paths = draft.paths.trim();
  if (draft.body.trim()) input.body = draft.body;
  return input;
}

export function buildSkillUpdateInput(
  skillId: string,
  draft: SkillFormDraft,
  baseline: SkillFormDraft,
): SkillUpdateInput {
  const input: SkillUpdateInput = { skill_id: skillId };
  if (draft.description !== baseline.description) input.description = draft.description;
  if (draft.when_to_use !== baseline.when_to_use) input.when_to_use = draft.when_to_use.trim();
  if (draft.allowed_tools !== baseline.allowed_tools) {
    input.allowed_tools = draft.allowed_tools.trim();
  }
  if (draft.paths !== baseline.paths) input.paths = draft.paths.trim();
  if (draft.body !== baseline.body) input.body = draft.body;
  return input;
}

/** True when an edit draft would send nothing at all (the server refuses that). */
export function skillUpdateIsEmpty(input: SkillUpdateInput): boolean {
  return (
    input.description === undefined &&
    input.when_to_use === undefined &&
    input.allowed_tools === undefined &&
    input.paths === undefined &&
    input.body === undefined
  );
}

export interface SkillFormViewProps {
  mode: "create" | "edit";
  draft: SkillFormDraft;
  /** For `edit`: the fields as loaded, used to detect what the user changed. */
  baseline?: SkillFormDraft;
  busy: boolean;
  error: string | null;
  onChange: (patch: Partial<SkillFormDraft>) => void;
  onSubmit: () => void;
  onCancel: () => void;
}

/** The create/edit dialog body. Pure: no client, no store. */
export function SkillFormView({
  mode,
  draft,
  baseline,
  busy,
  error,
  onChange,
  onSubmit,
  onCancel,
}: SkillFormViewProps) {
  const { t } = useTranslation();
  const isEdit = mode === "edit";
  const patch = isEdit
    ? buildSkillUpdateInput(draft.name, draft, baseline ?? EMPTY_SKILL_DRAFT)
    : null;
  const canSubmit =
    !busy &&
    draft.name.trim().length > 0 &&
    draft.description.trim().length > 0 &&
    !(isEdit && patch !== null && skillUpdateIsEmpty(patch));

  return (
    <div className="settings-content">
      <div className="settings-card">
        <div className="settings-row settings-row-stack">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("skills.name")}</span>
            <span className="settings-row-desc">
              {isEdit ? t("skills.nameLocked") : t("skills.nameDesc")}
            </span>
          </div>
          <input
            className="settings-row-input"
            aria-label={t("skills.name")}
            value={draft.name}
            readOnly={isEdit}
            onChange={(event) => onChange({ name: event.target.value })}
          />
        </div>
        <div className="settings-row settings-row-stack">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("skills.description")}</span>
            <span className="settings-row-desc">{t("skills.descriptionDesc")}</span>
          </div>
          <input
            className="settings-row-input"
            aria-label={t("skills.description")}
            value={draft.description}
            onChange={(event) => onChange({ description: event.target.value })}
          />
        </div>
        <div className="settings-row settings-row-stack">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("skills.whenToUse")}</span>
            <span className="settings-row-desc">{t("skills.whenToUseDesc")}</span>
          </div>
          <input
            className="settings-row-input"
            aria-label={t("skills.whenToUse")}
            value={draft.when_to_use}
            onChange={(event) => onChange({ when_to_use: event.target.value })}
          />
        </div>
        <div className="settings-row settings-row-stack">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("skills.allowedTools")}</span>
            <span className="settings-row-desc">{t("skills.allowedToolsDesc")}</span>
          </div>
          <input
            className="settings-row-input"
            aria-label={t("skills.allowedTools")}
            value={draft.allowed_tools}
            onChange={(event) => onChange({ allowed_tools: event.target.value })}
          />
        </div>
        <div className="settings-row settings-row-stack">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("skills.paths")}</span>
            <span className="settings-row-desc">{t("skills.pathsDesc")}</span>
          </div>
          <input
            className="settings-row-input"
            aria-label={t("skills.paths")}
            value={draft.paths}
            onChange={(event) => onChange({ paths: event.target.value })}
          />
        </div>
        <div className="settings-row settings-row-stack">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("skills.body")}</span>
            <span className="settings-row-desc">
              {isEdit ? t("skills.bodyReplaceDesc") : t("skills.bodyDesc")}
            </span>
          </div>
          <textarea
            className="settings-row-input skill-body-input"
            aria-label={t("skills.body")}
            rows={8}
            value={draft.body}
            onChange={(event) => onChange({ body: event.target.value })}
          />
        </div>
        {error ? (
          <div className="settings-row">
            <div className="settings-row-text">
              <span className="settings-row-note is-error" role="alert">
                {t(error)}
              </span>
            </div>
          </div>
        ) : null}
      </div>
      <div className="dialog-actions">
        <button className="quiet-button" type="button" onClick={onCancel} disabled={busy}>
          {t("skills.cancel")}
        </button>
        <button className="primary-button" type="button" disabled={!canSubmit} onClick={onSubmit}>
          {busy ? t("skills.saving") : isEdit ? t("skills.save") : t("skills.create")}
        </button>
      </div>
    </div>
  );
}

export interface SkillCopyViewProps {
  sourceName: string;
  value: string;
  busy: boolean;
  error: string | null;
  onChange: (value: string) => void;
  onSubmit: () => void;
  onCancel: () => void;
}

/** The copy dialog body: a new name, and nothing else to decide. */
export function SkillCopyView({
  sourceName,
  value,
  busy,
  error,
  onChange,
  onSubmit,
  onCancel,
}: SkillCopyViewProps) {
  const { t } = useTranslation();
  return (
    <div className="settings-content">
      <div className="settings-card">
        <div className="settings-row settings-row-stack">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("skills.copyTitle", { name: sourceName })}</span>
            <span className="settings-row-desc">{t("skills.copyDesc")}</span>
          </div>
          <input
            className="settings-row-input"
            aria-label={t("skills.newName")}
            value={value}
            onChange={(event) => onChange(event.target.value)}
          />
        </div>
        {error ? (
          <div className="settings-row">
            <div className="settings-row-text">
              <span className="settings-row-note is-error" role="alert">
                {t(error)}
              </span>
            </div>
          </div>
        ) : null}
      </div>
      <div className="dialog-actions">
        <button className="quiet-button" type="button" onClick={onCancel} disabled={busy}>
          {t("skills.cancel")}
        </button>
        <button
          className="primary-button"
          type="button"
          disabled={busy || value.trim().length === 0}
          onClick={onSubmit}
        >
          {busy ? t("skills.copying") : t("skills.copy")}
        </button>
      </div>
    </div>
  );
}

/** What the delete confirmation needs to render, and nothing more. */
export interface SkillDeleteTarget {
  id: string;
  name: string;
  /** Skills that name this one, if the caller knows them. */
  dependents?: number;
}

export interface SkillDeleteViewProps {
  target: SkillDeleteTarget;
  busy: boolean;
  error: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

/** Delete confirmation, stating exactly what is removed (the skill's directory). */
export function SkillDeleteView({
  target,
  busy,
  error,
  onConfirm,
  onCancel,
}: SkillDeleteViewProps) {
  const { t } = useTranslation();
  return (
    <div className="settings-content">
      <div className="settings-card">
        <div className="settings-row settings-row-stack">
          <div className="settings-row-text">
            <span className="settings-row-title">{t("skills.deleteTitle", { name: target.name })}</span>
            <span className="settings-row-desc">{t("skills.deleteDesc")}</span>
            {typeof target.dependents === "number" && target.dependents > 0 ? (
              <span className="settings-row-desc">
                {t("skills.deleteDependents", { count: target.dependents })}
              </span>
            ) : null}
          </div>
        </div>
        {error ? (
          <div className="settings-row">
            <div className="settings-row-text">
              <span className="settings-row-note is-error" role="alert">
                {t(error)}
              </span>
            </div>
          </div>
        ) : null}
      </div>
      <div className="dialog-actions">
        <button className="quiet-button" type="button" onClick={onCancel} disabled={busy}>
          {t("skills.cancel")}
        </button>
        <button className="primary-button is-destructive" type="button" disabled={busy} onClick={onConfirm}>
          {busy ? t("skills.deleting") : t("skills.deleteConfirm")}
        </button>
      </div>
    </div>
  );
}

/** The create/edit form, the copy dialog and the delete confirmation. */
export type SkillWriteMode =
  | { kind: "create" }
  | { kind: "edit"; skill: { id: string; name: string } }
  | { kind: "copy"; skill: { id: string; name: string } }
  | { kind: "delete"; skill: { id: string; name: string } }
  | null;

export interface SkillWriteDialogsProps {
  client: SkillAdminClient | null;
  mode: SkillWriteMode;
  /** Called after a confirmed write so the caller can re-list from the host. */
  onChanged: () => void;
  onClose: () => void;
  /** Loads the editable fields for `edit`; the read face caps the body. */
  loadDraft?: (skillId: string) => Promise<Partial<SkillFormDraft>>;
}

/**
 * Wired dialogs. The store owns the wire calls and the confirmations; this
 * component owns only the in-progress text and the draft.
 *
 * After a successful write the caller's `onChanged` re-lists — the dialogs never
 * insert their own result into the list, so what the user sees next is what the
 * host answered.
 */
export function SkillWriteDialogs({
  client,
  mode,
  onChanged,
  onClose,
  loadDraft,
}: SkillWriteDialogsProps) {
  const { t } = useTranslation();
  const busy = useSkillAdmin((state) => state.busy);
  const error = useSkillAdmin((state) => state.error);
  const create = useSkillAdmin((state) => state.create);
  const update = useSkillAdmin((state) => state.update);
  const remove = useSkillAdmin((state) => state.remove);
  const copy = useSkillAdmin((state) => state.copy);

  const [draft, setDraft] = useState<SkillFormDraft>(EMPTY_SKILL_DRAFT);
  const [baseline, setBaseline] = useState<SkillFormDraft>(EMPTY_SKILL_DRAFT);
  const [copyName, setCopyName] = useState("");
  const [loadedFor, setLoadedFor] = useState<string | null>(null);

  // Load the editable fields once per edited skill. The body comes from the
  // caller's loader (the read face truncates it), and it starts empty for the
  // same reason: the dialog says "replace the body", it can never show what it
  // was not given.
  const editingId = mode?.kind === "edit" ? mode.skill.id : null;
  const editingName = mode?.kind === "edit" ? mode.skill.name : null;
  useEffect(() => {
    if (!editingId || !editingName) return;
    if (loadedFor === editingId) return;
    setLoadedFor(editingId);
    const seed: SkillFormDraft = { ...EMPTY_SKILL_DRAFT, name: editingName };
    setDraft(seed);
    setBaseline(seed);
    if (!loadDraft) return;
    let cancelled = false;
    void loadDraft(editingId).then((loaded) => {
      if (cancelled) return;
      const next: SkillFormDraft = { ...seed, ...loaded, name: editingName };
      setDraft(next);
      setBaseline(next);
    });
    return () => {
      cancelled = true;
    };
  }, [editingId, editingName, loadDraft, loadedFor]);

  if (!mode) return null;

  const close = () => {
    setDraft(EMPTY_SKILL_DRAFT);
    setBaseline(EMPTY_SKILL_DRAFT);
    setCopyName("");
    setLoadedFor(null);
    onClose();
  };

  if (mode.kind === "create") {
    return (
      <DialogShell
        onClose={close}
        labelledBy="skill-create-title"
        title={t("skills.createTitle")}
        titleId="skill-create-title"
        width="wide"
      >
        <SkillFormView
          mode="create"
          draft={draft}
          busy={busy === "create"}
          error={error}
          onChange={(patch) => setDraft((current) => ({ ...current, ...patch }))}
          onSubmit={() => {
            void create(client, buildSkillCreateInput(draft)).then((ok) => {
              if (ok) {
                onChanged();
                close();
              }
            });
          }}
          onCancel={close}
        />
      </DialogShell>
    );
  }

  if (mode.kind === "edit") {
    return (
      <DialogShell
        onClose={close}
        labelledBy="skill-edit-title"
        title={t("skills.editTitle", { name: mode.skill.name })}
        titleId="skill-edit-title"
        width="wide"
      >
        <SkillFormView
          mode="edit"
          draft={draft}
          baseline={baseline}
          busy={busy === mode.skill.id}
          error={error}
          onChange={(patch) => setDraft((current) => ({ ...current, ...patch }))}
          onSubmit={() => {
            const input = buildSkillUpdateInput(mode.skill.id, draft, baseline);
            if (skillUpdateIsEmpty(input)) return;
            void update(client, input).then((ok) => {
              if (ok) {
                onChanged();
                close();
              }
            });
          }}
          onCancel={close}
        />
      </DialogShell>
    );
  }

  if (mode.kind === "copy") {
    return (
      <DialogShell
        onClose={close}
        labelledBy="skill-copy-title"
        title={t("skills.copy")}
        titleId="skill-copy-title"
      >
        <SkillCopyView
          sourceName={mode.skill.name}
          value={copyName}
          busy={busy === mode.skill.id}
          error={error}
          onChange={setCopyName}
          onSubmit={() => {
            void copy(client, mode.skill.id, copyName.trim()).then((ok) => {
              if (ok) {
                onChanged();
                close();
              }
            });
          }}
          onCancel={close}
        />
      </DialogShell>
    );
  }

  return (
    <DialogShell
      onClose={close}
      labelledBy="skill-delete-title"
      title={t("skills.delete")}
      titleId="skill-delete-title"
    >
      <SkillDeleteView
        target={{ id: mode.skill.id, name: mode.skill.name }}
        busy={busy === mode.skill.id}
        error={error}
        onConfirm={() => {
          void remove(client, mode.skill.id).then((ok) => {
            if (ok) {
              onChanged();
              close();
            }
          });
        }}
        onCancel={close}
      />
    </DialogShell>
  );
}

/** The skills tab's own toolbar: one primary action. */
export function SkillWriteToolbarView({ onCreated }: { onCreated: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="market-toolbar">
      <button className="primary-button" type="button" onClick={onCreated}>
        <Plus size={15} strokeWidth={1.7} />
        {t("skills.createAction")}
      </button>
    </div>
  );
}
