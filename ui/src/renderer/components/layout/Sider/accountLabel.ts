export type SiderAccountIdentity = {
  username?: string | null;
  email?: string | null;
};

export function formatSiderAccountLabel(identity: SiderAccountIdentity): string {
  const username = identity.username?.trim() ?? '';
  const email = identity.email?.trim() ?? '';
  if (username && username !== email) return username;
  const atIndex = email.lastIndexOf('@');
  if (atIndex > 0) return email.slice(0, atIndex);
  return username || email;
}
