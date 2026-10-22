import {useEffect} from 'react';
import {useLocation} from 'react-router-dom';
import {reportVoiceCandidate} from '@common/adapter/voiceCompanion';
import {parseSessionRoute} from '@renderer/utils/routes/sessionRoute';

export function useVoiceContext() {
  const {pathname}=useLocation();
  useEffect(()=>{
    const route=parseSessionRoute(pathname);
    const report=()=>{void reportVoiceCandidate(route?.kind==='conversation'?route.id:null).catch(()=>{});};
    report();
    window.addEventListener('focus',report);
    document.addEventListener('visibilitychange',report);
    return ()=>{window.removeEventListener('focus',report);document.removeEventListener('visibilitychange',report);void reportVoiceCandidate(null).catch(()=>{});};
  },[pathname]);
}
