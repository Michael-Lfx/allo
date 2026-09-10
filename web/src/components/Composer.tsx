import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { useClickAway } from "ahooks";
import {
  ArrowUp,
  AtSign,
  Check,
  ChevronDown,
  ChevronRight,
  FileText,
  Folder,
  FolderPlus,
  Layers,
  Mic,
  Plus,
  Search,
  SlidersHorizontal,
  Wrench,
} from "lucide-react";
import { IconButton } from "./IconButton";
import { ContextIndicator } from "./ContextIndicator";
import { ModelPicker } from "./ModelPicker";
import { useTranslation } from "react-i18next";
import { ComposerCatalogMenu } from "./ComposerCatalogMenu";
import { CommandPalette, type PaletteItem, type PaletteItemKind } from "./CommandPalette";
import { useAppStore, registerComposerFocus } from "../store/appStore";
import { modelChipLabel } from "../ui/format";
import type {
  AgentSummary,
  ConnectorSummary,
  ConversationModelOptions,
  ConversationView,
  LocalizedText,
  MentionKind,
  MentionRef,
  ProviderWithModel,
  ReasoningEffort,
  SkillSummary,
} from "../lib/protocol";

/** Human label for a reasoning-effort value shown in the model chip. */
function effortLabelOf(effort: ReasoningEffort | ""): string {
  switch (effort) {
    case "low": return "低";
    case "medium": return "中";
    case "high": return "高";
    case "max": return "超高";
    case "xhigh": return "极高";
    default: return "";
  }
}

export function Composer(props: {
  composerRef: RefObject<HTMLTextAreaElement | null>;
}) {
  const composerRef = props.composerRef;
  const { t } = useTranslation();
  const draft = useAppStore((s) => s.draft);
  const phase = useAppStore((s) => s.phase);
  const connected = useAppStore((s) => s.phase === "online");
  const isSending = useAppStore((s) => s.isSending);
  const isProcessing = useAppStore((s) => s.stream.isProcessing);
  const conversations = useAppStore((s) => s.conversations);
  const selectedConversationId = useAppStore((s) => s.selectedConversationId);
  const providerId = useAppStore((s) => s.providerId);
  const model = useAppStore((s) => s.model);
  const modelOptions = useAppStore((s) => s.modelOptions);
  const modelDirectory = useAppStore((s) => s.modelDirectory);
  const selectedModelKey = useAppStore((s) => s.selectedModelKey);
  const selectedEffort = useAppStore((s) => s.selectedEffort);
  const hasConversation = useAppStore((s) => s.selectedConversationId !== null);
  const composerMenuOpen = useAppStore((s) => s.composerMenuOpen);
  const modelPickerOpen = useAppStore((s) => s.modelPickerOpen);
  const workspaces = useAppStore((s) => s.workspaces);
  const openNewChatDialog = useAppStore((s) => s.openNewChatDialog);

  const setDraft = useAppStore((s) => s.setDraft);
  const send = useAppStore((s) => s.send);
  const newChat = useAppStore((s) => s.newChat);
  const openSettings = useAppStore((s) => s.openSettings);
  const toggleComposerMenu = useAppStore((s) => s.toggleComposerMenu);
  const closeComposerMenu = useAppStore((s) => s.closeComposerMenu);
  const toggleModelPicker = useAppStore((s) => s.toggleModelPicker);
  const chooseModel = useAppStore((s) => s.chooseModel);
  const chooseEffort = useAppStore((s) => s.chooseEffort);
  const closeModelPicker = useAppStore((s) => s.closeModelPicker);

  /** Which catalog submenu (agents/skills/connectors) is expanded, if any. */
  const [catalogMenu, setCatalogMenu] = useState<"agents" | "skills" | "connectors" | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);

  /** Workspace picker under the composer (Kimi-style). */
  const [workspacePickerOpen, setWorkspacePickerOpen] = useState(false);
  const workspacePickerRef = useRef<HTMLDivElement | null>(null);
  useClickAway(() => setWorkspacePickerOpen(false), workspacePickerRef, ["mousedown", "touchstart"]);

  const setComposerMentions = useAppStore((s) => s.setComposerMentions);
  const composerMentions = useAppStore((s) => s.composerMentions);

  /** Pick a catalog entry: record it as a structured mention and backfill the
   *  draft with `@name` so the user sees the reference (docs/agent-store/05
   *  §4.7). The whole pick is resolved on send. */
  const pickCatalogItem = (kind: "agents" | "skills" | "connectors", item: { id: string; name: string }) => {
    const mentionKind: MentionKind = kind === "agents" ? "agent" : kind === "skills" ? "skill" : "connector";
    const mentions = composerMentions ?? [];
    const existing = mentions.findIndex((m) => m.id === item.id && m.kind === mentionKind);
    const next =
      existing >= 0
        ? [...mentions.slice(0, existing), ...mentions.slice(existing + 1)]
        : [...mentions, { kind: mentionKind, id: item.id }];
    setComposerMentions(next);
    const token = `@${item.name}`;
    const current = draft.trim();
    setDraft(current ? `${current} ${token}` : token);
    // Track the token so deleting it also drops the structured mention (AC-1).
    mentionTokensRef.current.set(token, { kind: mentionKind, id: item.id });
    setCatalogMenu(null);
    toggleComposerMenu();
  };

  /** ── W1 command palette (doc 19 §3 W1) ─────────────────────────────────
   *  The palette never takes focus: its query is derived from the draft text
   *  after the `/` or `@` trigger, so ordinary typing and IME composition keep
   *  working, and deleting the inserted token can drop the structured mention.
   */
  const client = useAppStore((s) => s.client);
  const toggleCatalog = useAppStore((s) => s.toggleCatalog);
  const shareConversation = useAppStore((s) => s.shareConversation);
  const toggleArtifactPanel = useAppStore((s) => s.toggleArtifactPanel);
  const [palette, setPalette] = useState<{ mode: "command" | "mention"; start: number; active: number } | null>(null);
  const [catalogs, setCatalogs] = useState<{
    agents: AgentSummary[];
    skills: SkillSummary[];
    connectors: ConnectorSummary[];
  } | null>(null);
  const [palettePrompts, setPalettePrompts] = useState<PaletteItem[]>([]);
  const [paletteLoading, setPaletteLoading] = useState(false);
  /** Caret index, tracked outside React state (read synchronously on keydown). */
  const cursorRef = useRef(0);
  /** `@name` token → structured mention, so token deletion is observable. */
  const mentionTokensRef = useRef(new Map<string, MentionRef>());

  const localize = (text: LocalizedText | null | undefined): string => (text ? text.zh || text.en || "" : "");

  // Mention mode: load the three catalogs once per connection.
  useEffect(() => {
    if (palette?.mode !== "mention" || catalogs || !client) return;
    let cancelled = false;
    setPaletteLoading(true);
    void Promise.all([client.agents.list(), client.skills.list(), client.connectors.list()])
      .then(([agents, skills, connectorList]) => {
        if (!cancelled) setCatalogs({ agents, skills, connectors: connectorList });
      })
      .catch(() => {
        if (!cancelled) setCatalogs({ agents: [], skills: [], connectors: [] });
      })
      .finally(() => {
        if (!cancelled) setPaletteLoading(false);
      });
    return () => { cancelled = true; };
  }, [palette?.mode, catalogs, client]);

  // Command mode: offer the quick prompts / default init prompt of the experts
  // already mentioned in the draft — that mention is the composer's only
  // expert binding (docs/agent-store/05 §4.7).
  useEffect(() => {
    if (palette?.mode !== "command" || !client) return;
    const agentIds = (composerMentions ?? []).filter((m) => m.kind === "agent").map((m) => m.id);
    if (agentIds.length === 0) {
      setPalettePrompts([]);
      return;
    }
    let cancelled = false;
    void Promise.all(agentIds.map((id) => client.agents.get(id).catch(() => null))).then((details) => {
      if (cancelled) return;
      const items: PaletteItem[] = [];
      for (const detail of details) {
        if (!detail) continue;
        const init = localize(detail.default_init_prompt);
        if (init) items.push({ id: `init:${detail.id}`, kind: "prompt", label: init, hint: detail.name });
        (detail.quick_prompts ?? []).forEach((prompt, index) => {
          const text = localize(prompt);
          if (text) items.push({ id: `qp:${detail.id}:${index}`, kind: "prompt", label: text, hint: detail.name });
        });
      }
      setPalettePrompts(items);
    });
    return () => { cancelled = true; };
  }, [palette?.mode, composerMentions, client]);

  // The store clears mentions after a send / draft reset: drop the token map too.
  useEffect(() => {
    if (!composerMentions || composerMentions.length === 0) mentionTokensRef.current.clear();
  }, [composerMentions]);

  /** Drop mentions whose `@token` is no longer present in the draft (AC-1). */
  const syncMentionTokens = (nextDraft: string) => {
    const tokens = mentionTokensRef.current;
    if (tokens.size === 0) return;
    const alive = new Set<MentionRef>();
    let removed = false;
    for (const [token, ref] of [...tokens]) {
      if (nextDraft.includes(token)) {
        alive.add(ref);
      } else {
        tokens.delete(token);
        removed = true;
      }
    }
    if (!removed) return;
    const next = (composerMentions ?? []).filter((m) => [...alive].some((r) => r.id === m.id && r.kind === m.kind));
    setComposerMentions(next.length > 0 ? next : null);
  };

  /** Re-derive the open palette (if any) from the draft up to the caret. */
  const syncPalette = (value: string, cursor: number) => {
    const before = value.slice(0, cursor);
    const at = Math.max(before.lastIndexOf("/"), before.lastIndexOf("@"));
    if (at < 0) {
      setPalette(null);
      return;
    }
    const preceding = at === 0 ? "" : before[at - 1];
    // Only a line-leading or whitespace-preceded trigger counts, and a query
    // containing whitespace means ordinary prose — not a palette.
    if ((preceding && !/\s/.test(preceding)) || /\s/.test(before.slice(at + 1))) {
      setPalette(null);
      return;
    }
    setPalette({ mode: before[at] === "/" ? "command" : "mention", start: at, active: 0 });
  };

  const paletteQuery = palette ? draft.slice(palette.start + 1, cursorRef.current).trim().toLowerCase() : "";

  const paletteItems = useMemo<PaletteItem[]>(() => {
    if (!palette) return [];
    const matches = (label: string) => !paletteQuery || label.toLowerCase().includes(paletteQuery);
    if (palette.mode === "command") {
      const actions: PaletteItem[] = [
        { id: "newChat", kind: "action", label: t("palette.newChat") },
        { id: "newChatFolder", kind: "action", label: t("palette.newChatFolder") },
        { id: "store", kind: "action", label: t("palette.store") },
        { id: "artifacts", kind: "action", label: t("palette.artifacts") },
        { id: "settings", kind: "action", label: t("palette.settings") },
        { id: "share", kind: "action", label: t("palette.share") },
      ];
      return [
        ...actions.filter((item) => matches(item.label)),
        ...palettePrompts.filter((item) => matches(item.label)),
      ];
    }
    if (!catalogs) return [];
    const rows = (kind: PaletteItemKind, list: Array<{ id: string; name: string }>): PaletteItem[] =>
      list
        .map((row) => ({
          id: row.id,
          kind,
          label: localize((row as { display_name?: LocalizedText | null }).display_name) || row.name,
        }))
        .filter((item) => matches(item.label));
    return [
      ...rows("agent", catalogs.agents),
      ...rows("skill", catalogs.skills),
      ...rows("connector", catalogs.connectors),
    ];
  }, [palette, paletteQuery, palettePrompts, catalogs, t]);

  /** Swap the typed `/query` or `@query` (trigger → caret) for `inserted`. */
  const replaceTrigger = (inserted: string) => {
    if (!palette) return;
    const cursor = cursorRef.current;
    setDraft(`${draft.slice(0, palette.start)}${inserted}${draft.slice(cursor)}`);
  };

  const runPaletteAction = (id: string) => {
    switch (id) {
      case "newChat": void newChat(); break;
      case "newChatFolder": openNewChatDialog(); break;
      case "store": toggleCatalog(); break;
      case "artifacts": toggleArtifactPanel(); break;
      case "settings": openSettings(); break;
      case "share": void shareConversation(); break;
      default: break;
    }
  };

  const pickPaletteItem = (item: PaletteItem) => {
    const current = palette;
    if (!current) return;
    if (current.mode === "command") {
      replaceTrigger(item.kind === "action" ? "" : item.label);
      setPalette(null);
      if (item.kind === "action") runPaletteAction(item.id);
      return;
    }
    const kind: MentionKind = item.kind === "agent" ? "agent" : item.kind === "skill" ? "skill" : "connector";
    const token = `@${item.label}`;
    replaceTrigger(`${token} `);
    const ref: MentionRef = { kind, id: item.id };
    mentionTokensRef.current.set(token, ref);
    setComposerMentions([...(composerMentions ?? []).filter((m) => !(m.id === ref.id && m.kind === ref.kind)), ref]);
    setPalette(null);
  };

  /** Escape: close and drop the half-typed trigger so the draft stays clean. */
  const dismissPalette = () => {
    replaceTrigger("");
    setPalette(null);
  };

  /** `Cmd/Ctrl+K` summons the same panel (doc 19 §3 W1 ④). */
  const summonPalette = () => {
    const cursor = composerRef.current?.selectionStart ?? draft.length;
    cursorRef.current = cursor + 1;
    setDraft(`${draft.slice(0, cursor)}/${draft.slice(cursor)}`);
    setPalette({ mode: "command", start: cursor, active: 0 });
  };

  const paletteNote =
    paletteItems.length > 0 ? null : !client ? t("palette.offline") : paletteLoading ? null : t("palette.empty");

  // Clicking outside the composer menu closes it (whole popover).
  useClickAway(() => { closeComposerMenu(); setCatalogMenu(null); }, popoverRef, ["mousedown", "touchstart"]);

  const currentConversation = useMemo<ConversationView | null>(
    () => conversations.find((item) => item.conversation_id === selectedConversationId) ?? null,
    [conversations, selectedConversationId],
  );
  const currentModel = useMemo<ProviderWithModel | null>(
    () => currentConversation?.model ?? (providerId && model ? { provider_id: providerId, model } : null),
    [currentConversation, providerId, model],
  );

  const currentWorkspace = useMemo(
    () => currentConversation == null || !currentConversation.workspace_id ? null : workspaces.find((w) => w.workspace_id === currentConversation.workspace_id) ?? null,
    [currentConversation, workspaces],
  );

  /** Register this textarea so the store's send / create flows can refocus it
   *  after a conversation opens (they call `composerFocusRequest`). */
  useEffect(() => {
    registerComposerFocus(() => composerRef.current?.focus());
  }, [composerRef]);

  /** True while an IME composition is active: Enter then commits a candidate,
   *  it must not submit the message. Never lifted to React state — this flag
   *  is read synchronously inside the key handler. */
  const composingRef = useRef(false);

  /** Keep the composer textarea at 3 visible rows minimum, growing with the
   *  draft up to a viewport-relative cap, and shrinking when emptied. */
  useEffect(() => {
    const element = composerRef.current;
    if (!element) return;
    element.style.height = "auto";
    const capped = Math.min(element.scrollHeight, Math.max(96, Math.round(window.innerHeight * 0.35)));
    element.style.height = `${capped}px`;
  }, [draft, composerRef]);

  return <div className="composer-area">
    <div className="composer-shell">
      <textarea
        ref={composerRef}
        value={draft}
        onChange={(event) => {
          const next = event.target.value;
          cursorRef.current = event.target.selectionStart ?? next.length;
          setDraft(next);
          syncMentionTokens(next);
          syncPalette(next, cursorRef.current);
        }}
        onSelect={(event) => { cursorRef.current = event.currentTarget.selectionStart ?? cursorRef.current; }}
        onKeyDown={(event) => {
          // `Cmd/Ctrl+K` summons the palette while typing (W1 ④).
          if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
            event.preventDefault();
            summonPalette();
            return;
          }
          // An active IME composition owns Enter and the arrow keys.
          if (composingRef.current) return;
          if (palette) {
            const count = paletteItems.length;
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
              event.preventDefault();
              if (count > 0) {
                const delta = event.key === "ArrowDown" ? 1 : -1;
                setPalette((p) => (p ? { ...p, active: (p.active + delta + count) % count } : p));
              }
              return;
            }
            if (event.key === "Escape") {
              event.preventDefault();
              dismissPalette();
              return;
            }
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              const item = paletteItems[palette.active];
              if (item) pickPaletteItem(item);
              return;
            }
          }
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            void send();
          }
        }}
        onCompositionStart={() => { composingRef.current = true; }}
        onCompositionEnd={() => { composingRef.current = false; }}
        placeholder={connected ? t("composer.placeholderConnected") : t("composer.placeholderDisconnected")}
        disabled={!connected || isSending || isProcessing}
        rows={3}
        aria-label="消息内容"
        aria-keyshortcuts="Enter"
      />
      {palette && (
        <CommandPalette
          mode={palette.mode}
          items={paletteItems}
          activeIndex={palette.active}
          loading={paletteLoading}
          note={paletteNote}
          onPick={pickPaletteItem}
          onHover={(index) => setPalette((p) => (p ? { ...p, active: index } : p))}
        />
      )}
      <div className="composer-footer">
        <div className="composer-footer-left">
          <div className="composer-add-menu">
            <IconButton label={t("composer.addMenu")} className="composer-add" onClick={toggleComposerMenu} onMouseDown={(event) => event.stopPropagation()} aria-expanded={composerMenuOpen}>
              <Plus size={19} strokeWidth={1.8} />
            </IconButton>
            {composerMenuOpen && <div className={`composer-popover ${catalogMenu ? "has-submenu" : ""}`} role="menu" ref={popoverRef}>
              <div className="composer-menu">
                <div className="composer-menu-search">
                  <Search aria-hidden="true" size={15} />
                  <input type="text" placeholder={t("composer.menuSearch")} aria-label={t("composer.menuSearch")} />
                </div>
                <div className="composer-menu-items">
                  <button type="button" role="menuitem" className={catalogMenu === null ? "is-active" : ""} onClick={() => setCatalogMenu(null)}><FileText aria-hidden="true" size={17} /> <span>{t("composer.addFile")}</span> <ChevronRight aria-hidden="true" size={15} /></button>
                  <button type="button" role="menuitem" onClick={() => void newChat()}><Layers aria-hidden="true" size={17} /> <span>{t("composer.modeOption")}</span> <ChevronRight aria-hidden="true" size={15} /></button>
                  <button type="button" role="menuitem" className={catalogMenu === "agents" ? "is-active" : ""} onClick={() => setCatalogMenu("agents")}><AtSign aria-hidden="true" size={17} /> <span>{t("composer.expert")}</span> <ChevronRight aria-hidden="true" size={15} /></button>
                  <button type="button" role="menuitem" className={catalogMenu === "skills" ? "is-active" : ""} onClick={() => setCatalogMenu("skills")}><SlidersHorizontal aria-hidden="true" size={17} /> <span>{t("composer.skill")}</span> <ChevronRight aria-hidden="true" size={15} /></button>
                  <button type="button" role="menuitem" className={catalogMenu === "connectors" ? "is-active" : ""} onClick={() => setCatalogMenu("connectors")}><Wrench aria-hidden="true" size={17} /> <span>{t("composer.connector")}</span> <ChevronRight aria-hidden="true" size={15} /></button>
                </div>
              </div>
              {catalogMenu && (
                <ComposerCatalogMenu
                  kind={catalogMenu}
                  onClose={() => setCatalogMenu(null)}
                  onPick={pickCatalogItem}
                />
              )}
            </div>}
          </div>
        </div>
        <div className="composer-footer-right">
          {currentConversation && <ContextIndicator usage={currentConversation.context_usage ?? null} compact />}
          <div className="model-picker-wrap">
            <button className="model-chip" type="button" onClick={toggleModelPicker} onMouseDown={(event) => event.stopPropagation()} title={t("composer.modelPickerTitle")} aria-expanded={modelPickerOpen}>
              <span className={`composer-model-dot ${phase}`} aria-hidden="true" />
              <span>{modelChipLabel(currentModel, selectedModelKey, modelOptions, t("modelPicker.defaultModel"), effortLabelOf(selectedEffort))}</span>
              <ChevronDown aria-hidden="true" size={13} strokeWidth={1.8} />
            </button>
            {modelPickerOpen && (
              <ModelPicker
                options={modelOptions}
                directory={modelDirectory}
                selectedKey={selectedModelKey}
                effort={selectedEffort}
                hasConversation={hasConversation}
                onSelectModel={chooseModel}
                onSelectEffort={chooseEffort}
                onClose={closeModelPicker}
              />
            )}
          </div>
          <button className="voice-button" type="button" aria-label={t("composer.voice")} title={t("composer.voice")}>
            <Mic size={17} strokeWidth={1.8} />
          </button>
          <button className="send-button" type="button" onClick={() => void send()} disabled={!connected || !draft.trim() || isSending || isProcessing} aria-label={t("composer.send")}>
            <ArrowUp size={18} strokeWidth={2} />
          </button>
        </div>
      </div>
    </div>
    {!hasConversation && (
    <div className={`composer-workspace ${workspacePickerOpen ? "is-open" : ""}`} ref={workspacePickerRef}>
      <button className="composer-workspace-trigger" type="button" onClick={() => setWorkspacePickerOpen((v) => !v)} aria-expanded={workspacePickerOpen}>
        <Folder size={15} strokeWidth={1.7} aria-hidden="true" />
        <span className="composer-workspace-name">{currentWorkspace?.name ?? workspaces[0]?.name ?? t("composer.noWorkspace")}</span>
        {workspacePickerOpen ? <ChevronDown size={14} className="is-up" /> : <ChevronRight size={14} />}
      </button>
      {workspacePickerOpen && (
        <div className="composer-workspace-pop" role="listbox" aria-label={t("composer.workspacePicker")}>
          <div className="composer-workspace-pop-title">{t("composer.recentFolders")}</div>
          {workspaces.map((workspace) => {
            const isCurrent = currentWorkspace?.workspace_id === workspace.workspace_id;
            return (
              <button key={workspace.workspace_id} className={`composer-workspace-item ${isCurrent ? "is-current" : ""}`} type="button" onClick={() => { setWorkspacePickerOpen(false); void newChat(workspace.workspace_id); }}>
                <Folder size={15} strokeWidth={1.7} aria-hidden="true" />
                <span className="composer-workspace-item-main">
                  <span className="composer-workspace-item-name">{workspace.name}</span>
                  <span className="composer-workspace-item-path">{workspace.canonical_path?.startsWith("\\\\?\\") ? workspace.canonical_path.slice(4) : (workspace.canonical_path ?? "")}</span>
                </span>
                {isCurrent && <Check size={14} strokeWidth={2.2} className="composer-workspace-item-check" />}
              </button>
            );
          })}
          <button className="composer-workspace-item composer-workspace-add" type="button" onClick={() => { setWorkspacePickerOpen(false); openNewChatDialog(); }}>
            <FolderPlus size={15} strokeWidth={1.7} aria-hidden="true" />
            <span className="composer-workspace-item-name">{t("composer.chooseFolder")}</span>
          </button>
        </div>
      )}
    </div>
    )}
    {hasConversation && <p className="composer-disclaimer">{t("composer.disclaimer")}</p>}
  </div>;
}
