import{r as n}from"./index-C17WTOs5.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */function c(){const r=n.useRef(null),[i,o]=n.useState(0);return n.useEffect(()=>{const t=r.current;if(!t)return;const e=()=>o(t.getBoundingClientRect().width);e();const s=new ResizeObserver(e);return s.observe(t),window.addEventListener("resize",e),()=>{s.disconnect(),window.removeEventListener("resize",e)}},[]),{ref:r,width:i}}export{c as u};
