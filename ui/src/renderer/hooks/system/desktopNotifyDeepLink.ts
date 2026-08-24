/** Build a `flowy://navigate?route=…` deep link for desktop notification clicks. */
export function navigateDeepLink(route: string): string {
  return `flowy://navigate?route=${encodeURIComponent(route)}`;
}

export function conversationNotifyDeepLink(conversationId: string): string {
  return navigateDeepLink(`/conversation/${conversationId}`);
}

export function requirementNotifyDeepLink(tag: string, requirementId: string): string {
  const params = new URLSearchParams({ tag, id: requirementId });
  return navigateDeepLink(`/requirements?${params.toString()}`);
}
