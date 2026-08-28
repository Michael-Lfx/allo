/** Build a `flowy://navigate?route=…` deep link for desktop notification clicks. */
export function navigateDeepLink(route: string): string {
  return `flowy://navigate?route=${encodeURIComponent(route)}`;
}

export function conversationAttentionId(conversationId: string, turnId?: string | null): string {
  return `conversation:${conversationId}:turn:${turnId ?? 'latest'}`;
}

export function requirementAttentionId(requirementId: string): string {
  return `requirement:${requirementId}`;
}

export function supportAttentionId(serverMessageSeq: number | string): string {
  return `support:${serverMessageSeq}`;
}

export function supportNotifyDeepLink(attentionId?: string): string {
  return attentionId ? `flowy://support?attention_id=${encodeURIComponent(attentionId)}` : 'flowy://support';
}

export function conversationNotifyDeepLink(conversationId: string, attentionId?: string): string {
  const route = attentionId
    ? `/conversation/${conversationId}?attention_id=${encodeURIComponent(attentionId)}`
    : `/conversation/${conversationId}`;
  return navigateDeepLink(route);
}

export function requirementNotifyDeepLink(tag: string, requirementId: string): string {
  const params = new URLSearchParams({ tag, id: requirementId });
  return navigateDeepLink(`/requirements?${params.toString()}`);
}
