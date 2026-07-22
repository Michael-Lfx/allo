import{r,c as t}from"./index-C17WTOs5.js";function n(e){const c=r.useSyncExternalStore(s=>t.subscribe(e,s),()=>t.get(e)),o=r.useCallback(s=>t.set(e,s),[e]);return[c,o]}export{n as u};
