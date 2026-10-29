import { useEffect } from 'react';
import { useLocation } from 'react-router-dom';
import { pendingDictationIntent, revokeDictationIntent } from '@renderer/pages/guid/dictationIntent';

/**
 * The intent is tied to the history entry that was created for the home
 * navigation. If the app leaves before the home control mounts, or returns
 * through an unrelated history entry, this always-mounted guard revokes it.
 */
export function DictationIntentRouteGuard() {
  const location = useLocation();
  useEffect(() => {
    const intent = pendingDictationIntent();
    if (!intent) return;
    const stateId = (location.state as { dictationIntentId?: string } | null)?.dictationIntentId;
    if (location.pathname !== intent.route || stateId !== intent.id) {
      revokeDictationIntent(intent.id);
    }
  }, [location.key, location.pathname, location.state]);
  return null;
}
