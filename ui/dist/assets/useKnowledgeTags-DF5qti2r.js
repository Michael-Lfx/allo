import{r as t,af as s}from"./index-C17WTOs5.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */function T(){const[c,l]=t.useState([]),[u,o]=t.useState(!1),[i,r]=t.useState(null),e=t.useCallback(async()=>{o(!0);try{const a=await s.listTags.invoke();l(a),r(null)}catch(a){console.error("Failed to load knowledge tags",a),r(String(a))}finally{o(!1)}},[]);t.useEffect(()=>{e()},[e]),t.useEffect(()=>{const a=s.onTagChanged.on(()=>{e()});return()=>a()},[e]);const g=t.useCallback(async(a,n)=>{const k=await s.createTag.invoke({label:a,color:n});return await e(),k},[e]),d=t.useCallback(async(a,n)=>{await s.updateTag.invoke({key:a,...n}),await e()},[e]),f=t.useCallback(async a=>{await s.deleteTag.invoke({key:a}),await e()},[e]);return{tags:c,loading:u,error:i,createTag:g,updateTag:d,deleteTag:f,refresh:e}}export{T as u};
