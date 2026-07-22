import{r as a,c as e,s as o,t as u}from"./index-C17WTOs5.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const n=()=>e.get("system.keepAwake")??!0;function i(){const[k,r]=a.useState(n);a.useEffect(()=>e.subscribe("system.keepAwake",s=>r(s==null?!0:!!s)),[]);const p=a.useCallback(async t=>{const s=e.get("system.keepAwake");e.setLocal("system.keepAwake",t);try{await o.applyKeepAwake.invoke({enabled:t}),await u.setKeepAwake.invoke({enabled:t})}catch(c){throw e.setLocal("system.keepAwake",s??!0),c}},[]);return{keepAwake:k,setKeepAwake:p}}export{i as u};
