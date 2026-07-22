import{C as n}from"./CapabilityIcon-B6HAKjdZ.js";import{j as m,e as S,k,r as d,P as f,bo as _}from"./index-C17WTOs5.js";import{_ as b}from"./Brain-DvT9Gp6b.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const I={off:n.off,idle:n.idle,active:n.active},M={off:n.off,armed:n.armed,intervening:n.active},C=k(b),T=({size:e,className:t,spinning:s=!1})=>m.jsx(C,{theme:"outline",size:e,fill:"currentColor",className:S("block",s&&"autowork-spin",t)});/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const c=(e,t)=>`${e}:${t}`,a=new Map,i=new Map,r=new Set;let p=!1;const l=()=>r.forEach(e=>e()),u=()=>({autowork:new Map(a),idmm:new Map(i)}),g=e=>{const t=c(e.kind,e.target_id);e.enabled?a.set(t,e.run_state):a.delete(t),l()},y=e=>{const t=c(e.kind,e.target_id);e.enabled?i.set(t,e.run_state):i.delete(t),l()},O=()=>{p||(p=!0,f.tagBindings.invoke().then(e=>{let t=!1;for(const s of e??[])for(const o of s.bindings)a.set(c(o.kind,o.target_id),o.run_state),t=!0;t&&l()}).catch(()=>{}),f.onAutoWork.on(e=>{g(e)}),_.onStatus.on(e=>{y(e)}))};function v(){const[e,t]=d.useState(u);return d.useEffect(()=>{O();const s=()=>t(u());return r.add(s),s(),()=>{r.delete(s)}},[]),e}export{I as A,M as I,y as a,c,T as r,v as u};
