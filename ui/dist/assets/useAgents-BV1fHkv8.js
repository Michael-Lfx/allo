import{u as A,D as a,f as c,r as n,a as o,b as g}from"./index-C17WTOs5.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const d=3e4;let r=0,t=null;async function u(){await g.refreshCustomAgents.invoke(),await o(a)}async function E(){const e=Date.now();if(t)return t;if(!(r>0&&e-r<d))return r=e,t=u().catch(s=>{console.error("Failed to refresh detected agents:",s)}).finally(()=>{t=null}),t}const _=()=>{const{data:e,isLoading:s,error:f}=A(a,c),i=n.useCallback(()=>o(a),[]),l=n.useCallback(u,[]);return n.useEffect(()=>{E()},[]),{agents:e??[],isLoading:s,error:f,revalidate:i,refreshCustomAgents:l}};async function C(){const e=await c();return await o(a,e,{revalidate:!1}),e}export{C as g,_ as u};
