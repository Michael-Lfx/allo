const PLAN_SUFFIX = /(?:\s+plan|计划|套餐)$/i;

/** Short pill label. "Pro Plan" and "FreePlan" both become "Pro" / "Free". */
export function shortPlanBadge(plan?: string | null, planCode?: string | null): string {
  const label = plan?.trim() ?? '';
  if (label) return label.replace(PLAN_SUFFIX, '').trim();
  const code = planCode?.trim() ?? '';
  if (!code) return '';
  return code.replace(/plan$/i, '').trim();
}

/** Front-face name. An email nickname becomes the mailbox name; the full address stays on the back. */
export function membershipDisplayName(label: string): string {
  const trimmed = label.trim();
  const at = trimmed.lastIndexOf('@');
  if (at > 0 && !trimmed.slice(at + 1).includes(' ')) return trimmed.slice(0, at);
  return trimmed;
}

/** Stamp date, local calendar day, zero-padded. `2026.11.03`. */
export function formatMembershipDate(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}.${month}.${day}`;
}
