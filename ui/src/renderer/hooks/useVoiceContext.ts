import {useEffect} from 'react';
import {useLocation} from 'react-router-dom';
import {reportVoiceCandidate} from '@common/adapter/voiceCompanion';
import {parseSessionRoute} from '@renderer/utils/routes/sessionRoute';

export function useVoiceContext() {
  const {pathname}=useLocation();
  useEffect(()=>{
    const route=parseSessionRoute(pathname);
    // View presence controls empty-draft cleanup; it must not erase the last task workspace.
    const report=()=>{
      if(route?.kind==='conversation')void reportVoiceCandidate(route.id).catch(()=>{});
      else if(pathname!=='/guid')void reportVoiceCandidate(null,null,true).catch(()=>{});
    };
    report();
    window.addEventListener('focus',report);
    document.addEventListener('visibilitychange',report);
    return ()=>{window.removeEventListener('focus',report);document.removeEventListener('visibilitychange',report);};
  },[pathname]);
}
