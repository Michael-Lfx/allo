/**
 * A single, route-owned dictation intent. The settings page asks the home
 * composer to start the existing dictation control; only a mounted home
 * control may consume the intent, and leaving the route revokes it.
 */
export type DictationIntent = {
  id: string;
  route: string;
  created_at_ms: number;
};

const MAX_AGE_MS = 15_000;
let pending: DictationIntent | null = null;
const listeners = new Set<(intent: DictationIntent | null) => void>();

function emit(): void {
  for (const listener of listeners) listener(pending);
}

function live(intent: DictationIntent | null): DictationIntent | null {
  if (!intent) return null;
  if (Date.now() - intent.created_at_ms > MAX_AGE_MS) return null;
  return intent;
}

export function requestDictationIntent(route = '/'): DictationIntent {
  pending = { id: `dictation-${Date.now()}-${Math.random().toString(36).slice(2)}`, route, created_at_ms: Date.now() };
  emit();
  return pending;
}

export function pendingDictationIntent(): DictationIntent | null {
  return live(pending);
}

export function readDictationIntent(route: string): DictationIntent | null {
  const intent = live(pending);
  if (intent && intent.route !== route) return null;
  return intent;
}

export function subscribeDictationIntent(listener: (intent: DictationIntent | null) => void): () => void {
  listeners.add(listener);
  listener(live(pending));
  return () => listeners.delete(listener);
}

export function consumeDictationIntent(route: string, id: string): boolean {
  const intent = live(pending);
  if (!intent || intent.route !== route || intent.id !== id) return false;
  pending = null;
  emit();
  return true;
}

export function revokeDictationIntent(id: string): void {
  if (pending?.id !== id) return;
  pending = null;
  emit();
}
