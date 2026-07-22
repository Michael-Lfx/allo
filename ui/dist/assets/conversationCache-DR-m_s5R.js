import{y as n,ao as e,a as r}from"./index-C17WTOs5.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */async function o(a){try{return await n.get.invoke({id:a})}catch(t){if(e(t)&&t.status===404&&t.code==="NOT_FOUND")return null;throw t}}async function i(a){const t=await o(a);t&&await r(`conversation/${a}`,t,!1)}function c(a){r(`conversation/${a.id}`,a,!1)}export{o as g,i as r,c as s};
