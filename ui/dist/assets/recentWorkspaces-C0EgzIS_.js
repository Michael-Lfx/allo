/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const c="nomifun:recent-workspaces";const o=(t=c)=>{try{return JSON.parse(localStorage.getItem(t)??"[]")}catch{return[]}},a=(t,e=c)=>{try{const s=o(e),r=[t,...s.filter(n=>n!==t)].slice(0,5);localStorage.setItem(e,JSON.stringify(r))}catch{}};export{c as D,a,o as g};
