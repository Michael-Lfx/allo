import {
  AtSign,
  Brain,
  Copy,
  Files,
  FolderPlus,
  Lamp,
  Layers,
  MessageSquarePlus,
  Pencil,
  Share2,
  Settings,
  SlidersHorizontal,
  Sparkles,
  Terminal,
  Trash2,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import { useTranslation } from "react-i18next";

import type { PaletteItem, PaletteItemKind } from "../lib/palette-model";

/**
 * W1 command palette V1 + V2 (doc 19 §3 W1 / W1b).
 *
 * Presentational only: the composer keeps DOM focus in the textarea and owns
 * the query (derived from the draft after the `/` or `@` trigger) plus the
 * keyboard cursor, so ↑/↓/Enter/Esc work without moving focus. Rendering a
 * separate input here would break IME composition and the "delete the token →
 * drop the structured mention" invariant.
 *
 * V2 (R19) adds three sections — session actions, models, reasoning levels —
 * which arrive as ordinary rows: this component only renders `groupKey` headers
 * and per-kind icons, so a row the data layer adds tomorrow needs no change here.
 */
export type { PaletteItem, PaletteItemKind };

const ICONS: Record<PaletteItemKind, LucideIcon> = {
  action: Terminal,
  prompt: Sparkles,
  agent: Lamp,
  skill: Wrench,
  connector: AtSign,
  session: Layers,
  model: SlidersHorizontal,
  effort: Brain,
};

/** Per-action icons, so the built-in list does not read as one grey column. */
const ACTION_ICONS: Record<string, LucideIcon> = {
  newChat: MessageSquarePlus,
  newChatFolder: FolderPlus,
  store: Layers,
  settings: Settings,
  artifacts: Files,
  share: Share2,
  "session.rename": Pencil,
  "session.delete": Trash2,
  "session.share": Share2,
  "session.copyId": Copy,
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
            const Icon = ACTION_ICONS[item.actionId ?? item.id] ?? ICONS[item.kind];
            // A section header is emitted whenever the group changes, so the
            // cursor stays a flat index over `items` (keyboard needs no tree).
            const startsGroup = index === 0 || items[index - 1].groupKey !== item.groupKey;
            return (
              <div key={`${item.kind}-${item.id}`} className="command-palette-entry">
                {startsGroup && item.groupKey && (
                  <div className="command-palette-group">{t(item.groupKey)}</div>
                )}
                <button
                  type="button"
                  role="option"
                  aria-selected={index === activeIndex}
                  aria-disabled={item.disabled}
                  disabled={item.disabled}
                  className={`command-palette-item${index === activeIndex ? " is-active" : ""}`}
                  onMouseEnter={() => onHover(index)}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => onPick(item)}
                >
                  <Icon aria-hidden="true" size={15} strokeWidth={1.8} />
                  <span className="command-palette-label">{item.label}</span>
                  {item.disabled && item.disabledReasonKey ? (
                    <span className="command-palette-item-hint">{t(item.disabledReasonKey)}</span>
                  ) : (
                    item.hint && <span className="command-palette-item-hint">{item.hint}</span>
                  )}
                </button>
              </div>
            );
          })}
      </div>
    </div>
  );
}
