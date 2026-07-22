import{h as c}from"./useModelProviderList-yjK9FvvY.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const s=new Map,m=e=>{var o;const d=e.model_enabled?JSON.stringify(e.model_enabled):"all-enabled",n=`${e.id}-${(e.models||[]).join(",")}-${d}`;if(s.has(n))return s.get(n);const a=[];for(const l of e.models||[]){if(!(((o=e.model_enabled)==null?void 0:o[l])!==!1))continue;const t=c(e,l,"function_calling"),i=c(e,l,"excludeFromPrimary");(t===!0||t===void 0)&&i!==!0&&a.push(l)}return s.set(n,a),a},f=e=>m(e).length>0;export{m as g,f as h};
