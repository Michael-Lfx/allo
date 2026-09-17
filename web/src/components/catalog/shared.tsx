/**
 * Pieces shared by the catalog surfaces (`CatalogView`, `MarketSourcesPanel`,
 * `ImportPanel`).
 *
 * Two kinds of thing live here, and nothing else:
 *
 * - **semantic label resolvers** — a wire status string to our own i18n key.
 *   They are tables plus one lookup, so that an unknown status degrades to
 *   `catalog.stateOther` with the raw value instead of rendering nothing;
 * - **the small presentational pieces** those surfaces all repeat (initial
 *   badge, avatar badge, tag row, compatibility chips, meta rows).
 *
 * They were moved out of `CatalogView.tsx` when the page was split into three
 * surfaces; nothing here holds state, calls the client or reads the store.
 */

import { useTranslation } from "react-i18next";
import { pickLocalized, useLocalizedLang, type LocalizedLang } from "../../ui/localize";
import type { AgentSummary, CompatibilityTriple, InstallState, StoreItem } from "../../lib/protocol";

// ---------------------------------------------------------------------------
// wire status → i18n key
// ---------------------------------------------------------------------------

export const CONNECTOR_STATE_KEYS: Record<string, string> = {
  installed: "catalog.stateInstalled",
  configured: "catalog.stateConfigured",
  authorization_required: "catalog.stateAuthRequired",
  authenticated: "catalog.stateAuthenticated",
  connected: "catalog.stateConnected",
  degraded: "catalog.stateDegraded",
  error: "catalog.stateError",
  reauthorization_required: "catalog.stateReauthRequired",
};

export const AUTH_STATE_KEYS: Record<string, string> = {
  authenticated: "catalog.authAuthenticated",
  not_authenticated: "catalog.authNotAuthenticated",
  reauthorization_required: "catalog.authReauthRequired",
};

export const IMPORT_STATUS_KEYS: Record<string, string> = {
  completed: "catalog.importStatusCompleted",
  "completed-with-warnings": "catalog.importStatusWarnings",
  blocked: "catalog.importStatusBlocked",
  failed: "catalog.importStatusFailed",
};

export const SEMANTIC_KEYS: Record<string, string> = {
  compatible: "catalog.semantic_compatible",
  compatible_with_adapter: "catalog.semantic_compatible_with_adapter",
  manual_review: "catalog.semantic_manual_review",
  unsupported: "catalog.semantic_unsupported",
  pending_legal_review: "catalog.semantic_pending_legal_review",
};

/** `source_kind` is a wire enum (`directory` / `github` / `git` / `url`), not a
 *  label — the registry surfaces printed it raw, so a zh-CN panel read `url`. */
export const MARKET_KIND_KEYS: Record<string, string> = {
  directory: "catalog.marketKindDirectory",
  github: "catalog.marketKindGithub",
  git: "catalog.marketKindGit",
  url: "catalog.marketKindUrl",
};

type Translate = (key: string, opts?: Record<string, unknown>) => string;

export function stateLabel(t: Translate, keys: Record<string, string>, value: string): string {
  const key = keys[value];
  return key ? t(key) : t("catalog.stateOther", { status: value });
}

export function semanticLabel(t: Translate, value: string): string {
  const key = SEMANTIC_KEYS[value];
  return key ? t(key) : value;
}

export function importStatusLabel(t: Translate, status: string): string {
  const key = IMPORT_STATUS_KEYS[status];
  return key ? t(key) : t("catalog.stateOther", { status });
}

export function marketKindLabel(t: Translate, kind: string): string {
  const key = MARKET_KIND_KEYS[kind];
  return key ? t(key) : kind;
}

export function installStateLabel(t: ReturnType<typeof useTranslation>["t"], state: InstallState): string {
  switch (state) {
    case "installed": return t("catalog.installStateInstalled");
    case "disabled": return t("catalog.installStateDisabled");
    default: return t("catalog.installStateNotInstalled");
  }
}

/**
 * Whether a connector row should offer the OAuth 「连接」 action.
 *
 * The summary status already folds OAuth readiness in (`summary_status` in
 * `nomifun-app/src/app_server_catalog.rs`: oauth + most recent probe not
 * connected → `authorization_required`), so this needs no per-row
 * `connector/status` call — N rows must not become N requests.
 */
export function connectorNeedsAuth(connector: { auth_mode: string; status: string }): boolean {
  return connector.auth_mode === "oauth" && connector.status === "authorization_required";
}

/**
 * A connector row's second line (`status`, localized).
 *
 * A **disabled** connector reports `installed` on the wire, which would render
 * 「已安装」 while the switch beside it is off — so the disabled state wins.
 */
export function connectorRowStatusLabel(
  t: Translate,
  connector: { enabled: boolean; status: string },
): string {
  return connector.enabled ? stateLabel(t, CONNECTOR_STATE_KEYS, connector.status) : t("catalog.disabled");
}

/** Which install transition a component row offers. */
export type InstallToggleKind = "enable" | "disable" | "uninstall";

// ---------------------------------------------------------------------------
// status → CSS modifier
// ---------------------------------------------------------------------------

export function connectorStateClass(status: string): string {
  switch (status) {
    case "connected":
      return "is-success";
    case "error":
    case "degraded":
      return "is-error";
    case "authorization_required":
    case "reauthorization_required":
      return "is-warn";
    default:
      return "";
  }
}

export function importStatusClass(status: string): string {
  switch (status) {
    case "completed":
      return "is-success";
    case "completed-with-warnings":
      return "is-warn";
    case "blocked":
    case "failed":
      return "is-error";
    default:
      return "";
  }
}

export function installStateClass(state: InstallState): string {
  switch (state) {
    case "installed": return "is-success";
    case "disabled": return "is-warn";
    default: return "";
  }
}

// ---------------------------------------------------------------------------
// badges
// ---------------------------------------------------------------------------

/** Deterministic badge hues so the same item keeps its color across renders. */
const BADGE_HUES = [
  "#3b6ea5", "#2f7d5a", "#8a5a2e", "#5b5f7d", "#9a3f6b", "#4d7f8a",
];

export function badgeColor(seed: string): string {
  let hash = 0;
  for (const char of seed) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return BADGE_HUES[hash % BADGE_HUES.length];
}

/** Icon to the left of each card row. */
export function InitialBadge({ name, size = 40 }: { name: string; size?: number }) {
  const initial = (name.trim()[0] ?? "?").toUpperCase();
  return (
    <span
      className="market-badge"
      style={{ width: size, height: size, background: badgeColor(name), fontSize: size * 0.4 }}
      aria-hidden="true"
    >
      {initial}
    </span>
  );
}

/** Market display name for an agent (resolved by UI language, D8=A). */
export function agentDisplayName(agent: AgentSummary, lang: LocalizedLang): string {
  return pickLocalized(agent.display_name, lang) || agent.name;
}

/** Badge with the agent avatar when declared, else the initial. */
export function AgentBadge({ agent, size = 40 }: { agent: AgentSummary; size?: number }) {
  const lang = useLocalizedLang();
  const name = agentDisplayName(agent, lang);
  if (agent.avatar_url) {
    return (
      <img className="market-badge market-avatar" width={size} height={size} src={agent.avatar_url} alt={name} loading="lazy" />
    );
  }
  return <InitialBadge name={name} size={size} />;
}

/** Badge from an absolute/relative avatar URL, else the initial letter. */
export function AvatarBadge({
  name,
  avatarUrl,
  size = 40,
  rootUrl,
}: {
  name: string;
  avatarUrl?: string | null;
  size?: number;
  rootUrl?: string;
}) {
  const avatar = avatarUrl
    ? avatarUrl.startsWith("http://") || avatarUrl.startsWith("https://")
      ? avatarUrl
      : rootUrl
        ? `${rootUrl}${avatarUrl.startsWith("/") ? "" : "/"}${avatarUrl}`
        : avatarUrl
    : null;
  if (avatar) {
    return (
      <img
        className="market-badge market-avatar"
        width={size}
        height={size}
        src={avatar}
        alt={name}
        loading="lazy"
      />
    );
  }
  return <InitialBadge name={name} size={size} />;
}

/** Store item badge: avatar when declared, else the initial. */
export function StoreBadge({ item, size = 40, rootUrl }: { item: StoreItem; size?: number; rootUrl?: string }) {
  const lang = useLocalizedLang();
  const name = pickLocalized(item.display_name, lang) || item.name;
  return <AvatarBadge name={name} avatarUrl={item.avatar_url} size={size} rootUrl={rootUrl} />;
}

// ---------------------------------------------------------------------------
// rows
// ---------------------------------------------------------------------------

/** The tag strip on a card: at most two tags, then a `+N` overflow chip. */
export function Tags({ tags }: { tags: string[] }) {
  const visible = tags.slice(0, 2);
  const extra = tags.length - visible.length;
  return (
    <div className="market-tags">
      {visible.map((tag) => (
        <span className="market-tag" key={tag}>{tag}</span>
      ))}
      {extra > 0 && <span className="market-tag is-extra">+{extra}</span>}
    </div>
  );
}

export function CompatChips({ triple }: { triple: CompatibilityTriple }) {
  const { t } = useTranslation();
  if (!triple) return null;
  return (
    <div className="market-compat">
      <span className="market-tag">{semanticLabel(t, triple.semantic_status)}</span>
      <span className="market-tag is-muted">{t("catalog.compatRuntime")}: {triple.runtime_status}</span>
      <span className="market-tag is-muted">{t("catalog.compatDistribution")}: {triple.distribution_status}</span>
      {triple.reasons.length > 0 && (
        <details className="market-reasons">
          <summary>{t("catalog.compatReasons")}</summary>
          <ul>{triple.reasons.map((reason, index) => <li key={index}>{reason}</li>)}</ul>
        </details>
      )}
    </div>
  );
}

export function MetaRow({ label, value, mono }: { label: string; value?: string | null; mono?: boolean }) {
  if (value === undefined || value === null || value === "") return null;
  return (
    <div className="market-meta-row">
      <dt>{label}</dt>
      <dd className={mono ? "is-mono" : ""}>{value}</dd>
    </div>
  );
}

export function MetaList({ label, values }: { label: string; values: string[] }) {
  if (values.length === 0) return null;
  return (
    <div className="market-meta-row">
      <dt>{label}</dt>
      <dd>{values.join(", ")}</dd>
    </div>
  );
}
