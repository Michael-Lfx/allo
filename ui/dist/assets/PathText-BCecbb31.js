import{j as t,e as i}from"./index-C17WTOs5.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const r=a=>{if(!a)return{head:"",tail:""};const e=a.replace(/[\\/]+$/,""),s=Math.max(e.lastIndexOf("/"),e.lastIndexOf("\\"));return s<=0?{head:"",tail:e}:{head:e.slice(0,s),tail:e.slice(s)}},c=({path:a,className:e})=>{const{head:s,tail:n}=r(a);return s?t.jsxs("span",{className:i("flex items-center min-w-0 overflow-hidden",e),children:[t.jsx("span",{className:"overflow-hidden text-ellipsis whitespace-nowrap",children:s}),t.jsx("span",{className:"shrink-0 whitespace-nowrap",children:n})]}):t.jsx("span",{className:i("truncate",e),children:n||a})};export{c as P};
