export type SiderAccountIdentity = {
  nickname?: string | null;
  username?: string | null;
  email?: string | null;
};

export function formatSiderAccountLabel(identity: SiderAccountIdentity): string {
  const nickname = identity.nickname?.trim();
  if (nickname) return nickname;
  const candidate = identity.username?.trim() || identity.email?.trim() || '';
  const atIndex = candidate.lastIndexOf('@');
  return atIndex > 0 ? candidate.slice(0, atIndex) : candidate;
}
