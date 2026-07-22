import{ai as i,aj as u}from"./index-C17WTOs5.js";const F=(r,s)=>{const t=s.map(e=>typeof e=="string"?e:e.path).filter(Boolean);return Array.from(new Set([...r,...t]))},g=(r,s,t)=>{if(!s.length)return r;const e=t==null?void 0:t.replace(/[\\/]+$/,""),c=s.map(n=>{const a=n.replace(i,"$1");if(!e)return a;if(n.startsWith("/")||/^[A-Za-z]:/.test(n)){const o=n.replace(/\\/g,"/"),l=e.replace(/\\/g,"/");if(o.startsWith(l+"/")){const $=o.slice(l.length+1);return`${e}/${$.replace(i,"$1")}`}return a}return`${e}/${a}`});return`${r}

${u}
${c.join(`
`)}`};export{g as b,F as c};
