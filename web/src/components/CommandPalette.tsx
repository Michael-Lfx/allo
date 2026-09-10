import {
  AtSign,
  FolderPlus,
  Lamp,
  Layers,
  type LucideIcon,
  MessageSquarePlus,
  Settings,
  Files,
  Share2,
  Sparkles,
  Terminal,
  Wrench,
} from "lucide-react";
import { useTranslation } from "react-i18next";

/**
 * W1 command palette V1 (doc 19 §3 W1).
 *
 * Presentational only: the composer keeps DOM focus in the textarea and owns
 * the query (derived from the draft after the `/` or `@` trigger) plus the
 * keyboard cursor, so ↑/↓/Enter/Esc work without moving focus. Rendering a
 * separate input here would break IME composition and the "delete the token →
 * drop the structured mention" invariant.
 */
export type PaletteItemKind = "action" | "prompt" | "agent" | "skill" | "connector";

export interface PaletteItem {
  /** Stable key; for catalog items this is the mention id. */
  id: string;
  kind: PaletteItemKind;
  label: string;
  hint?: string;
}

const ICONS: Record<PaletteItemKind, LucideIcon> = {
  action: Terminal,
  prompt: Sparkles,
  agent: Lamp,
  skill: Wrench,
  connector: AtSign,
};

/** Per-action icons, so the built-in list does not read as one grey column. */
const ACTION_ICONS: Record<string, LucideIcon> = {
  newChat: MessageSquarePlus,
  newChatFolder: FolderPlus,
  store: Layers,
  settings: Settings,
  artifacts: Files,
  share: Share2,
};

export function CommandPalette({
  mode,
  items,
  activeIndex,
  loading,
  note,
  onPick,
  onHover,
}: {
  mode: "command" | "mention";
  items: PaletteItem[];
  activeIndex: number;
  loading: boolean;
  /** Empty/loading/offline copy; null when there is nothing to say. */
  note: string | null;
  onPick: (item: PaletteItem) => void;
  onHover: (index: number) => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="command-palette" role="listbox" aria-label={t(mode === "command" ? "palette.commandTitle" : "palette.mentionTitle")}>
      <div className="command-palette-head">
        <span className="command-palette-title">
          {t(mode === "command" ? "palette.commandTitle" : "palette.mentionTitle")}
        </span>
        <span className="command-palette-hint">{t("palette.keyHint")}</span>
      </div>
      <div className="command-palette-body">
        {note && <p className="command-palette-note">{note}</p>}
        {!note && loading && <p className="command-palette-note">{t("palette.loading")}</p>}
        {!note &&
          !loading &&
          items.map((item, index) => {
            const Icon = item.kind === "action" ? ACTION_ICONS[item.id] ?? Terminal : ICONS[item.kind];
            return (
              <button
                key={`${item.kind}-${item.id}`}
                type="button"
                role="option"
                aria-selected={index === activeIndex}
                className={`command-palette-item${index === activeIndex ? " is-active" : ""}`}
                onMouseEnter={() => onHover(index)}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => onPick(item)}
              >
                <Icon aria-hidden="true" size={15} strokeWidth={1.8} />
                <span className="command-palette-label">{item.label}</span>
                {item.hint && <span className="command-palette-item-hint">{item.hint}</span>}
              </button>
            );
          })}
      </div>
    </div>
  );
}
