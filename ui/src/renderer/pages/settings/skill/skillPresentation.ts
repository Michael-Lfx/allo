/**
 * Shared presentation helpers for the Skills Hub. Extracted from the old
 * flat-list SkillsHubSettings so the card grid and the page share one source.
 */

/** Normalize a skill name for use in a stable data-testid. */
export const normalizeTestId = (name: string): string => name.replace(/[:/\s<>"'|?*]/g, '-');

/** Public `<img>` path served by `GET /api/skills/{name}/icon`. */
export const skillIconPublicPath = (name: string): string =>
  `/api/skills/${encodeURIComponent(name.trim())}/icon`;

/**
 * Prefer the catalog/list `avatar` field; for builtins, fall back to the
 * public icon route so the session picker still shows packaged stills when
 * an older catalog payload omits the field.
 */
export const resolveCatalogSkillAvatar = (
  name: string,
  avatar: string | null | undefined,
  source?: string
): string | undefined => {
  const fromApi = avatar?.trim();
  if (fromApi) return fromApi;
  if (source === 'builtin' && name.trim()) return skillIconPublicPath(name);
  return undefined;
};

/** Resolve a skill list `avatar` field into an `<img src>`. */
export const resolveSkillAvatarSrc = (
  avatar: string | null | undefined,
  baseUrl: string
): string | undefined => {
  const value = avatar?.trim();
  if (!value) return undefined;
  if (/^(https?:|data:|blob:)/i.test(value)) return value;
  if (value.startsWith('/')) return `${baseUrl}${value}`;
  return value;
};

/**
 * Deterministic letter-avatar color class keyed off the skill name. These
 * fixed hexes are an intentional, pre-existing exception to the theme-variable
 * rule (the avatar palette must stay legible across all themes); carried over
 * verbatim from the previous SkillsHubSettings implementation.
 */
export const getAvatarColorClass = (name: string): string => {
  if (!name) return 'bg-[var(--color-primary)] text-white';
  const colors = [
    'bg-[#F53F3F] text-white', // Red
    'bg-[#F77234] text-white', // Orange
    'bg-[#B8860B] text-white', // Gold
    'bg-[#F5319D] text-white', // Pink
    'bg-[#C41D7F] text-white', // Raspberry
    'bg-[#722ED1] text-white', // Purple
  ];
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return colors[Math.abs(hash) % colors.length];
};
