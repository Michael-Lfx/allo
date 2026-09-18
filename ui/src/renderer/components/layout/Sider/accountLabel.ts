export type SiderAccountIdentity = {
  username?: string | null;
  email?: string | null;
};

export function formatSiderAccountLabel(identity: SiderAccountIdentity): string {
  const candidate = identity.username?.trim() || identity.email?.trim() || '';
  const atIndex = candidate.lastIndexOf('@');
  return atIndex > 0 ? candidate.slice(0, atIndex) : candidate;
}
