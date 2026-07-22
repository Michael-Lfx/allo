import{r,j as t,k as y}from"./index-C17WTOs5.js";import{L as q}from"./Left-CoE-e6NX.js";import{_ as J}from"./Right-C1ORDRw6.js";import{_ as Q}from"./Loading-B5o1W1Bp.js";import{_ as V}from"./Refresh-B3f8kkQu.js";const D=y(q),ee=y(J),re=y(V),te=y(Q),z=.75,O=1.5,be=({url:d,id:oe,showNavBar:m=!1,partition:ne,className:M,style:U,onDidFinishLoad:f,onDidFailLoad:h})=>{const I=r.useRef(null),W=r.useRef(null),x=r.useRef(null),[n,v]=r.useState(d),[j,c]=r.useState(d),[C,s]=r.useState(!0),[R,w]=r.useState(1),i=r.useRef([]),u=r.useRef([]),[G,g]=r.useState(!1),[Z,k]=r.useState(!1),a=r.useCallback(e=>{try{const o=new URL(e),l=o.hostname.toLowerCase(),b=l==="127.0.0.1"||l==="localhost",p=["18791","18888","19000"].includes(o.port);return b&&p}catch{return!1}},[])(n);r.useEffect(()=>{i.current=[],u.current=[],g(!1),k(!1),v(d),c(d),s(!0),w(1)},[d]),r.useEffect(()=>{const e=x.current;if(!e)return;const o=a?R:1;e.style.zoom=String(o)},[a,R]);const S=r.useCallback(e=>{!e||e===n||(n&&i.current.push(n),u.current=[],v(e),c(e),g(i.current.length>0),k(!1),s(!0))},[n]),F=r.useCallback(()=>{s(!1),f==null||f()},[f]),H=r.useCallback(()=>{s(!1),h==null||h(-1,"iframe failed to load")},[h]),K=r.useCallback(()=>{a&&w(1)},[a]),A=r.useCallback(()=>{const e=x.current,o=W.current;if(!(!a||!e||!o))try{const l=e.contentDocument,b=e.contentWindow;if(!l||!b)return;const p=l.getElementById("main-stage"),N=l.body,E=l.documentElement,_=Math.max((p==null?void 0:p.scrollWidth)||0,(N==null?void 0:N.scrollWidth)||0,(E==null?void 0:E.scrollWidth)||0,b.innerWidth||0);if(!_)return;const X=Number((o.clientWidth/_).toFixed(2));w(Math.max(z,Math.min(O,X)))}catch{}},[a]),B=r.useCallback(e=>{if(!a||!(e.ctrlKey||e.metaKey))return;e.preventDefault();const o=e.deltaY<0?.08:-.08;w(l=>{const b=Number((l+o).toFixed(2));return Math.max(z,Math.min(O,b))})},[a]),L=r.useCallback(()=>{if(i.current.length===0)return;const e=i.current.pop();u.current.push(n),g(i.current.length>0),k(!0),v(e),c(e),s(!0)},[n]),P=r.useCallback(()=>{if(u.current.length===0)return;const e=u.current.pop();i.current.push(n),g(!0),k(u.current.length>0),v(e),c(e),s(!0)},[n]),T=r.useCallback(()=>{const e=x.current;e&&(s(!0),e.src=n)},[n]),Y=r.useCallback(e=>{e.preventDefault();let o=j.trim();o&&(/^https?:\/\//i.test(o)||(o="https://"+o),S(o))},[j,S]),$=r.useCallback(e=>{e.key==="Escape"&&(c(n),e.target.blur())},[n]);return t.jsxs("div",{ref:I,className:`h-full w-full flex flex-col ${M??""}`,style:U,children:[m&&t.jsx("style",{children:`
            .nomi-url-viewer-toolbar {
              --viewer-border: var(--color-border-2);
              --viewer-border-hover: var(--color-border-3);
              --viewer-bg: var(--color-bg-3);
              --viewer-bg-hover: var(--color-fill-2);
              --viewer-text: var(--color-text-2);
              --viewer-text-muted: var(--color-text-3);
            }
            .nomi-url-viewer-toolbar .toolbar-btn {
              -webkit-appearance: none;
              appearance: none;
              display: inline-flex;
              align-items: center;
              justify-content: center;
              height: 30px;
              min-width: 30px;
              padding: 0 10px;
              border-radius: 10px;
              border: 1px solid var(--viewer-border);
              background: var(--viewer-bg);
              color: var(--viewer-text);
              line-height: 1;
              font-size: 12px;
              transition: all 150ms ease;
              cursor: pointer;
            }
            .nomi-url-viewer-toolbar .toolbar-btn.icon-btn {
              width: 30px;
              min-width: 30px;
              padding: 0;
            }
            .nomi-url-viewer-toolbar .toolbar-btn:hover:not(:disabled) {
              background: var(--viewer-bg-hover);
              border-color: var(--viewer-border-hover);
            }
            .nomi-url-viewer-toolbar .toolbar-btn:active:not(:disabled) {
              transform: translateY(0.5px);
            }
            .nomi-url-viewer-toolbar .toolbar-btn:focus-visible {
              outline: none;
              border-color: rgb(var(--primary-6));
              box-shadow: 0 0 0 2px rgba(var(--primary-6), 0.12);
            }
            .nomi-url-viewer-toolbar .toolbar-btn:disabled {
              opacity: 0.55;
              cursor: not-allowed;
              color: var(--viewer-text-muted);
              background: var(--color-bg-2);
            }
            .nomi-url-viewer-toolbar .toolbar-chip {
              display: inline-flex;
              align-items: center;
              justify-content: center;
              height: 30px;
              min-width: 48px;
              padding: 0 10px;
              border-radius: 10px;
              border: 1px solid var(--viewer-border);
              background: var(--color-bg-2);
              color: var(--viewer-text-muted);
              font-size: 11px;
              line-height: 1;
            }
            .nomi-url-viewer-toolbar .toolbar-input {
              -webkit-appearance: none;
              appearance: none;
              width: 100%;
              height: 30px;
              padding: 0 12px;
              border-radius: 10px;
              border: 1px solid var(--viewer-border);
              background: var(--viewer-bg);
              color: var(--color-text-1);
              font-size: 12px;
              line-height: 30px;
              transition: all 150ms ease;
            }
            .nomi-url-viewer-toolbar .toolbar-input:hover {
              border-color: var(--viewer-border-hover);
            }
            .nomi-url-viewer-toolbar .toolbar-input:focus {
              outline: none;
              border-color: rgb(var(--primary-6));
              box-shadow: 0 0 0 2px rgba(var(--primary-6), 0.12);
            }
          `}),m&&t.jsxs("div",{className:"nomi-url-viewer-toolbar flex items-center gap-6px h-40px px-10px bg-bg-2 border-b border-border-1 flex-shrink-0",children:[t.jsx("button",{onClick:L,disabled:!G,className:"toolbar-btn icon-btn",title:"Back",children:t.jsx(D,{theme:"outline",size:16})}),t.jsx("button",{onClick:P,disabled:!Z,className:"toolbar-btn icon-btn",title:"Forward",children:t.jsx(ee,{theme:"outline",size:16})}),t.jsx("button",{onClick:T,className:"toolbar-btn icon-btn",title:"Refresh",children:C?t.jsx(te,{theme:"outline",size:16,className:"animate-spin"}):t.jsx(re,{theme:"outline",size:16})}),a&&t.jsxs("div",{className:"flex items-center gap-6px ml-2px",children:[t.jsx("button",{onClick:K,className:"toolbar-btn",title:"Reset zoom",children:"100%"}),t.jsx("button",{onClick:A,className:"toolbar-btn",title:"Fit",children:"Fit"}),t.jsxs("span",{className:"toolbar-chip",children:[Math.round(R*100),"%"]})]}),t.jsx("form",{onSubmit:Y,className:"flex-1 ml-2px",children:t.jsx("input",{type:"text",value:j,onChange:e=>c(e.target.value),onKeyDown:$,onFocus:e=>e.target.select(),className:"toolbar-input",placeholder:"Enter URL..."})})]}),!m&&C&&t.jsx("div",{className:"absolute inset-0 flex items-center justify-center text-t-secondary text-14px z-10 pointer-events-none",children:t.jsx("span",{className:"animate-pulse",children:"Loading…"})}),t.jsx("div",{ref:W,className:"flex-1 overflow-hidden relative",style:{minHeight:0},onWheel:B,children:t.jsx("iframe",{ref:x,src:n,sandbox:"allow-scripts allow-same-origin allow-forms allow-popups allow-popups-to-escape-sandbox",referrerPolicy:"no-referrer-when-downgrade",onLoad:F,onError:H,className:"w-full h-full border-0 absolute left-0 top-0",style:{opacity:!m&&C?0:1,transition:"opacity 150ms ease-in"},title:"Embedded content"})})]})};export{be as W};
