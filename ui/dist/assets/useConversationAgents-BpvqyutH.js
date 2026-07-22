import{u as t,D as c,f as l,G as d}from"./index-C17WTOs5.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const u=()=>{const{data:e,isLoading:a,mutate:n}=t(c,l),{data:r,isLoading:i}=t("assistants.presets",async()=>{try{return(await d.list.invoke()).filter(o=>o.enabled!==!1)}catch(s){return console.error("Failed to load assistants for conversation selector:",s),[]}});return{cliAgents:e||[],presetAssistants:r||[],isLoading:a||i,refresh:async()=>{await n()}}};export{u};
