import {useEffect} from 'react';
import {useLocation} from 'react-router-dom';
import {reportVoiceCandidate} from '@common/adapter/voiceCompanion';
import {parseSessionRoute} from '@renderer/utils/routes/sessionRoute';

export function useVoiceContext() {
  const {pathname}=useLocation();
  useEffect(()=>{
    const route=parseSessionRoute(pathname);
    // Guid owns its explicit workspace selection; unrelated pages keep the last selection.
    const report=()=>{if(route?.kind==='conversation')void reportVoiceCandidate(route.id).catch(()=>{});};
    report();
    window.addEventListener('focus',report);
    document.addEventListener('visibilitychange',report);
    return ()=>{window.removeEventListener('focus',report);document.removeEventListener('visibilitychange',report);};
  },[pathname]);
}
