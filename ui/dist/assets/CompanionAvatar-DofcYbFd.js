import{j as i,r as Y,aK as L}from"./index-C17WTOs5.js";/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const N=({mood:o,activity:e,size:n=150})=>{const t=o==="sleepy",a=o==="worried",s=o==="excited",c=o==="happy",r=e==="thinking";return i.jsxs("div",{className:`nomi-ch nomi-mochi nomi-mochi--${o} nomi-mochi--${e}`,style:{width:n,height:n},children:[i.jsx("style",{children:`
.nomi-mochi { position: relative; display: inline-block; }
.nomi-mochi svg { display: block; overflow: visible; }
.nomi-mochi * { transform-box: view-box; }

/* ground shadow — counter-scales against body lift */
.nomi-mochi__shadow { transform-origin: 80px 147px; animation: nomi-mochi-shadow 3.4s ease-in-out infinite; }
@keyframes nomi-mochi-shadow { 0%,100%{ transform: scale(1); opacity:.13 } 50%{ transform: scale(.9); opacity:.09 } }

/* whole-body mochi squash & breathe */
.nomi-mochi__body-g { transform-origin: 80px 138px; animation: nomi-mochi-breathe 3.4s ease-in-out infinite; }
@keyframes nomi-mochi-breathe {
  0%,100%{ transform: translateY(0) scaleY(1) scaleX(1) }
  50%{ transform: translateY(-2px) scaleY(1.035) scaleX(.975) }
}

/* ears — soft alternate sway */
.nomi-mochi__ear-l { transform-origin: 64px 44px; animation: nomi-mochi-earL 3.6s ease-in-out infinite; }
.nomi-mochi__ear-r { transform-origin: 96px 44px; animation: nomi-mochi-earR 3.6s ease-in-out infinite; }
@keyframes nomi-mochi-earL { 0%,100%{ transform: rotate(2deg) } 50%{ transform: rotate(-5deg) } }
@keyframes nomi-mochi-earR { 0%,100%{ transform: rotate(-2deg) } 50%{ transform: rotate(5deg) } }

/* blink — periodic squash */
.nomi-mochi__eyes { transform-origin: 80px 92px; animation: nomi-mochi-blink 5s ease-in-out infinite; }
@keyframes nomi-mochi-blink { 0%,92%,100%{ transform: scaleY(1) } 95%{ transform: scaleY(.08) } }
.nomi-mochi--sleepy .nomi-mochi__eyes { animation: none; }

/* mouth chew while thinking */
.nomi-mochi__mouth { transform-origin: 80px 104px; }
.nomi-mochi--thinking .nomi-mochi__mouth { animation: nomi-mochi-chew .55s ease-in-out infinite; }
@keyframes nomi-mochi-chew { 0%,100%{ transform: scaleY(1) translateY(0) } 50%{ transform: scaleY(.6) translateY(1px) } }

/* happy / excited bounce overrides */
.nomi-mochi--happy .nomi-mochi__body-g { animation: nomi-mochi-hop 1.5s ease-in-out infinite; }
@keyframes nomi-mochi-hop {
  0%,100%{ transform: translateY(0) scaleY(1) } 30%{ transform: translateY(-6px) scaleY(1.04) }
  55%{ transform: translateY(0) scaleY(.94) } 70%{ transform: translateY(-1px) scaleY(1.01) }
}
.nomi-mochi--excited .nomi-mochi__body-g { animation: nomi-mochi-jump 1s ease-in-out infinite; }
@keyframes nomi-mochi-jump {
  0%,100%{ transform: translateY(0) scaleY(.95) scaleX(1.04) } 40%{ transform: translateY(-12px) scaleY(1.08) scaleX(.94) }
  60%{ transform: translateY(-10px) scaleY(1.05) } 80%{ transform: translateY(2px) scaleY(.9) scaleX(1.06) }
}
.nomi-mochi--sleepy .nomi-mochi__body-g { animation: nomi-mochi-doze 4s ease-in-out infinite; }
@keyframes nomi-mochi-doze { 0%,100%{ transform: translateY(2px) scaleY(.985) } 50%{ transform: translateY(4px) scaleY(.965) } }

/* sleepy Z's */
.nomi-mochi__z { opacity: 0; }
.nomi-mochi--sleepy .nomi-mochi__z1 { animation: nomi-mochi-z 3s ease-in-out infinite; }
.nomi-mochi--sleepy .nomi-mochi__z2 { animation: nomi-mochi-z 3s ease-in-out infinite 1.5s; }
@keyframes nomi-mochi-z {
  0%{ opacity:0; transform: translate(0,0) scale(.6) } 25%{ opacity:.9 }
  70%{ opacity:.7 } 100%{ opacity:0; transform: translate(7px,-22px) scale(1.1) }
}

/* worried sweat drop */
.nomi-mochi__sweat { opacity: 0; }
.nomi-mochi--worried .nomi-mochi__sweat { animation: nomi-mochi-sweat 2.8s ease-in-out infinite; }
@keyframes nomi-mochi-sweat {
  0%,12%{ opacity:0; transform: translateY(0) }
  22%{ opacity:.85 } 70%{ opacity:.85 } 100%{ opacity:0; transform: translateY(16px) }
}

/* excited sparkle particles */
.nomi-mochi__spark { opacity: 0; transform-origin: center; }
.nomi-mochi--excited .nomi-mochi__spark1 { animation: nomi-mochi-spark 1.1s ease-in-out infinite; }
.nomi-mochi--excited .nomi-mochi__spark2 { animation: nomi-mochi-spark 1.1s ease-in-out infinite .4s; }
.nomi-mochi--excited .nomi-mochi__spark3 { animation: nomi-mochi-spark 1.1s ease-in-out infinite .75s; }
@keyframes nomi-mochi-spark { 0%,100%{ opacity:0; transform: scale(.4) } 45%{ opacity:1; transform: scale(1.1) } }

/* thinking bubbles — small to large, serial float */
.nomi-mochi__tb { opacity: 0; }
.nomi-mochi--thinking .nomi-mochi__tb1 { animation: nomi-mochi-tb 2.4s ease-in-out infinite; }
.nomi-mochi--thinking .nomi-mochi__tb2 { animation: nomi-mochi-tb 2.4s ease-in-out infinite .35s; }
.nomi-mochi--thinking .nomi-mochi__tb3 { animation: nomi-mochi-tb 2.4s ease-in-out infinite .7s; }
@keyframes nomi-mochi-tb {
  0%{ opacity:0; transform: translateY(4px) scale(.5) } 30%{ opacity:.95 }
  75%{ opacity:.8 } 100%{ opacity:0; transform: translateY(-9px) scale(1) }
}
      `}),i.jsxs("svg",{viewBox:"0 0 160 160",width:n,height:n,children:[i.jsxs("defs",{children:[i.jsxs("radialGradient",{id:"mochiBody",cx:"42%",cy:"34%",r:"72%",children:[i.jsx("stop",{offset:"0%",stopColor:"#fffdfb"}),i.jsx("stop",{offset:"55%",stopColor:"#fff9f4"}),i.jsx("stop",{offset:"100%",stopColor:"#ffeede"})]}),i.jsxs("linearGradient",{id:"mochiEarIn",x1:"0",y1:"0",x2:"0",y2:"1",children:[i.jsx("stop",{offset:"0%",stopColor:"#ffd8e2"}),i.jsx("stop",{offset:"100%",stopColor:"#ffc0d0"})]}),i.jsxs("radialGradient",{id:"mochiBlush",cx:"50%",cy:"50%",r:"50%",children:[i.jsx("stop",{offset:"0%",stopColor:"#ffb6c9",stopOpacity:"0.85"}),i.jsx("stop",{offset:"100%",stopColor:"#ffc9d6",stopOpacity:"0"})]}),i.jsxs("radialGradient",{id:"mochiBubble",cx:"38%",cy:"32%",r:"70%",children:[i.jsx("stop",{offset:"0%",stopColor:"#fff2f6"}),i.jsx("stop",{offset:"100%",stopColor:"#ffcad9"})]})]}),i.jsx("ellipse",{className:"nomi-mochi__shadow",cx:"80",cy:"147",rx:"38",ry:"8",fill:"#000000",opacity:"0.12"}),i.jsxs("g",{className:"nomi-mochi__body-g",children:[i.jsxs("g",{className:"nomi-mochi__ear-l",children:[i.jsx("path",{d:"M62 60 C50 50 48 26 54 14 C58 6 68 8 70 20 C72 34 71 50 68 60 Z",fill:"url(#mochiBody)",stroke:"#dba6aa",strokeWidth:"2.3",strokeLinejoin:"round"}),i.jsx("path",{d:"M62 54 C55 46 54 28 58 18 C61 12 66 15 66 24 C66 36 65 48 63 55 Z",fill:"url(#mochiEarIn)"})]}),i.jsxs("g",{className:"nomi-mochi__ear-r",children:[i.jsx("path",{d:"M98 60 C110 50 112 26 106 14 C102 6 92 8 90 20 C88 34 89 50 92 60 Z",fill:"url(#mochiBody)",stroke:"#dba6aa",strokeWidth:"2.3",strokeLinejoin:"round"}),i.jsx("path",{d:"M98 54 C105 46 106 28 102 18 C99 12 94 15 94 24 C94 36 95 48 97 55 Z",fill:"url(#mochiEarIn)"})]}),i.jsx("ellipse",{cx:"64",cy:"139",rx:"11",ry:"8",fill:"url(#mochiBody)",stroke:"#dba6aa",strokeWidth:"2.2",strokeLinejoin:"round"}),i.jsx("ellipse",{cx:"96",cy:"139",rx:"11",ry:"8",fill:"url(#mochiBody)",stroke:"#dba6aa",strokeWidth:"2.2",strokeLinejoin:"round"}),i.jsx("path",{d:"M80 50 C116 50 132 76 132 102 C132 130 110 142 80 142 C50 142 28 130 28 102 C28 76 44 50 80 50 Z",fill:"url(#mochiBody)",stroke:"#dba6aa",strokeWidth:"2.4",strokeLinejoin:"round"}),i.jsx("path",{d:"M34 110 C46 134 70 140 80 140 C90 140 114 134 126 110 C120 132 102 141 80 141 C58 141 40 132 34 110 Z",fill:"#ffe6d2",opacity:"0.55"}),i.jsx("ellipse",{cx:"62",cy:"72",rx:"20",ry:"13",fill:"#ffffff",opacity:"0.55"}),i.jsx("ellipse",{cx:"104",cy:"66",rx:"7",ry:"5",fill:"#ffffff",opacity:"0.6"}),i.jsx("ellipse",{cx:"52",cy:"102",rx:"11",ry:"7",fill:"url(#mochiBlush)"}),i.jsx("ellipse",{cx:"108",cy:"102",rx:"11",ry:"7",fill:"url(#mochiBlush)"}),i.jsx("g",{className:"nomi-mochi__eyes",children:t?i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M56 92 Q63 99 70 92",fill:"none",stroke:"#7a5a52",strokeWidth:"3",strokeLinecap:"round"}),i.jsx("path",{d:"M90 92 Q97 99 104 92",fill:"none",stroke:"#7a5a52",strokeWidth:"3",strokeLinecap:"round"})]}):c?i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M55 95 Q63 86 71 95",fill:"none",stroke:"#5a3d38",strokeWidth:"3.4",strokeLinecap:"round"}),i.jsx("path",{d:"M89 95 Q97 86 105 95",fill:"none",stroke:"#5a3d38",strokeWidth:"3.4",strokeLinecap:"round"})]}):s?i.jsxs(i.Fragment,{children:[i.jsx("circle",{cx:"63",cy:"92",r:"8",fill:"#4a322d"}),i.jsx("circle",{cx:"97",cy:"92",r:"8",fill:"#4a322d"}),i.jsx("circle",{cx:"60",cy:"89",r:"2.8",fill:"#fff"}),i.jsx("circle",{cx:"94",cy:"89",r:"2.8",fill:"#fff"}),i.jsx("circle",{cx:"65",cy:"95",r:"1.6",fill:"#fff",opacity:"0.8"}),i.jsx("circle",{cx:"99",cy:"95",r:"1.6",fill:"#fff",opacity:"0.8"})]}):i.jsxs(i.Fragment,{children:[i.jsx("ellipse",{cx:"63",cy:"92",rx:"6",ry:"7.5",fill:"#4a322d"}),i.jsx("ellipse",{cx:"97",cy:"92",rx:"6",ry:"7.5",fill:"#4a322d"}),i.jsx("circle",{cx:"61",cy:"89",r:"2.4",fill:"#fff"}),i.jsx("circle",{cx:"95",cy:"89",r:"2.4",fill:"#fff"}),i.jsx("circle",{cx:"65",cy:"94",r:"1.2",fill:"#fff",opacity:"0.7"}),i.jsx("circle",{cx:"99",cy:"94",r:"1.2",fill:"#fff",opacity:"0.7"})]})}),a&&i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M55 82 Q63 80 70 84",fill:"none",stroke:"#c79a93",strokeWidth:"2.4",strokeLinecap:"round"}),i.jsx("path",{d:"M90 84 Q97 80 105 82",fill:"none",stroke:"#c79a93",strokeWidth:"2.4",strokeLinecap:"round"})]}),i.jsx("ellipse",{cx:"80",cy:"100",rx:"3",ry:"2.2",fill:"#e89aa8"}),i.jsx("g",{className:"nomi-mochi__mouth",children:t?i.jsx("path",{d:"M74 106 Q80 109 86 106",fill:"none",stroke:"#a86a64",strokeWidth:"2.2",strokeLinecap:"round"}):a?i.jsx("path",{d:"M73 108 Q80 102 87 108",fill:"none",stroke:"#a86a64",strokeWidth:"2.4",strokeLinecap:"round"}):s?i.jsx("path",{d:"M72 104 Q80 114 88 104 Q80 109 72 104 Z",fill:"#e98a98",stroke:"#a86a64",strokeWidth:"2",strokeLinejoin:"round"}):i.jsx("path",{d:"M74 104 Q80 110 86 104",fill:"none",stroke:"#a86a64",strokeWidth:"2.4",strokeLinecap:"round"})}),i.jsxs("g",{className:"nomi-mochi__sweat",children:[i.jsx("path",{d:"M114 80 C114 80 109 87 109 91 a5 5 0 0 0 10 0 C119 87 114 80 114 80 Z",fill:"#9fd9ef",stroke:"#7cc4e0",strokeWidth:"1"}),i.jsx("ellipse",{cx:"112",cy:"88",rx:"1.5",ry:"2",fill:"#fff",opacity:"0.7"})]})]}),t&&i.jsxs("g",{fill:"#b79be0",fontFamily:"sans-serif",fontWeight:"700",children:[i.jsx("text",{className:"nomi-mochi__z nomi-mochi__z1",x:"104",y:"48",fontSize:"13",children:"z"}),i.jsx("text",{className:"nomi-mochi__z nomi-mochi__z2",x:"112",y:"38",fontSize:"17",children:"Z"})]}),s&&i.jsxs("g",{fill:"#ffd84d",children:[i.jsx("path",{className:"nomi-mochi__spark nomi-mochi__spark1",d:"M118 60 l2 5 5 2 -5 2 -2 5 -2 -5 -5 -2 5 -2 Z"}),i.jsx("path",{className:"nomi-mochi__spark nomi-mochi__spark2",d:"M40 56 l1.6 4 4 1.6 -4 1.6 -1.6 4 -1.6 -4 -4 -1.6 4 -1.6 Z"}),i.jsx("path",{className:"nomi-mochi__spark nomi-mochi__spark3",d:"M128 88 l1.4 3.5 3.5 1.4 -3.5 1.4 -1.4 3.5 -1.4 -3.5 -3.5 -1.4 3.5 -1.4 Z"})]}),r&&i.jsxs("g",{children:[i.jsx("circle",{className:"nomi-mochi__tb nomi-mochi__tb1",cx:"112",cy:"52",r:"3",fill:"url(#mochiBubble)",stroke:"#f3b9cb",strokeWidth:"0.8"}),i.jsx("circle",{className:"nomi-mochi__tb nomi-mochi__tb2",cx:"120",cy:"42",r:"4.5",fill:"url(#mochiBubble)",stroke:"#f3b9cb",strokeWidth:"0.8"}),i.jsx("circle",{className:"nomi-mochi__tb nomi-mochi__tb3",cx:"130",cy:"30",r:"6.5",fill:"url(#mochiBubble)",stroke:"#f3b9cb",strokeWidth:"0.9"})]})]})]})};/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const W=({mood:o,activity:e,size:n=150})=>{const t=o==="sleepy";return i.jsxs("div",{className:`nomi-ch nomi-ink nomi-ink--${o} nomi-ink--${e}`,style:{width:n,height:n},children:[i.jsx("style",{children:`
.nomi-ink{position:relative;line-height:0}
.nomi-ink svg{display:block;overflow:visible}
.nomi-ink *{transform-box:view-box}

/* ground shadow — inverse-scales with the body lift */
.nomi-ink__shadow{transform-origin:80px 147px;animation:nomi-ink-shadow 3.4s ease-in-out infinite}
@keyframes nomi-ink-shadow{0%,100%{transform:scale(1);opacity:.13}50%{transform:scale(.9);opacity:.09}}

/* whole body: calm breathing + gentle lift */
.nomi-ink__body-g{transform-origin:80px 142px;animation:nomi-ink-breathe 3.4s ease-in-out infinite}
@keyframes nomi-ink-breathe{0%,100%{transform:translateY(0) scaleY(1)}50%{transform:translateY(-1.5px) scaleY(1.03)}}

/* tail — elegant slow sway from the root */
.nomi-ink__tail{transform-origin:104px 134px;animation:nomi-ink-tail 4.6s ease-in-out infinite}
@keyframes nomi-ink-tail{0%,100%{transform:rotate(-2.5deg)}50%{transform:rotate(2.5deg)}}

/* ears — occasional quick flick */
.nomi-ink__ear-l{transform-origin:60px 44px;animation:nomi-ink-ear 5.2s ease-in-out infinite}
.nomi-ink__ear-r{transform-origin:100px 44px;animation:nomi-ink-ear 5.2s ease-in-out infinite .4s}
@keyframes nomi-ink-ear{0%,86%,100%{transform:rotate(0)}90%{transform:rotate(-7deg)}94%{transform:rotate(4deg)}}

/* auto blink */
.nomi-ink__eyes{transform-origin:80px 80px;animation:nomi-ink-blink 5s ease-in-out infinite}
@keyframes nomi-ink-blink{0%,92%,100%{transform:scaleY(1)}96%{transform:scaleY(.08)}}

.nomi-ink__blush{opacity:0;animation:nomi-ink-blush 3.4s ease-in-out infinite}
@keyframes nomi-ink-blush{0%,100%{opacity:.32}50%{opacity:.5}}

/* === mood overrides === */
.nomi-ink--happy .nomi-ink__body-g{animation:nomi-ink-hop 1.7s ease-in-out infinite}
@keyframes nomi-ink-hop{0%,100%{transform:translateY(0)}50%{transform:translateY(-4px)}}

.nomi-ink--excited .nomi-ink__body-g{animation:nomi-ink-bounce 1s ease-in-out infinite}
@keyframes nomi-ink-bounce{0%,100%{transform:translateY(0) scaleY(1)}45%{transform:translateY(-9px) scaleY(1.05)}70%{transform:translateY(1px) scaleY(.96)}}
.nomi-ink--excited .nomi-ink__shadow{animation:nomi-ink-shadow 1s ease-in-out infinite}
.nomi-ink--excited .nomi-ink__eyes{animation:nomi-ink-blink 6s ease-in-out infinite}

.nomi-ink--sleepy .nomi-ink__body-g{animation:nomi-ink-sink 4.4s ease-in-out infinite}
@keyframes nomi-ink-sink{0%,100%{transform:translateY(2px) scaleY(.99)}50%{transform:translateY(3.5px) scaleY(1.01)}}
.nomi-ink--sleepy .nomi-ink__tail{animation:none;transform:rotate(0)}
.nomi-ink--sleepy .nomi-ink__eyes{animation:none}

/* z's drift up serially */
.nomi-ink__z{opacity:0}
.nomi-ink__z1{animation:nomi-ink-z 3.2s ease-in-out infinite}
.nomi-ink__z2{animation:nomi-ink-z 3.2s ease-in-out infinite 1.6s}
@keyframes nomi-ink-z{0%{opacity:0;transform:translate(0,0) scale(.7)}25%{opacity:.85}100%{opacity:0;transform:translate(7px,-20px) scale(1.1)}}

/* worried sweat drop slides + ears droop */
.nomi-ink__sweat{opacity:0;animation:nomi-ink-sweat 2.8s ease-in-out infinite}
@keyframes nomi-ink-sweat{0%,18%{opacity:0;transform:translateY(0)}30%{opacity:.85}70%{opacity:.8}100%{opacity:0;transform:translateY(15px)}}
.nomi-ink--worried .nomi-ink__ear-l{animation:none;transform:rotate(11deg)}
.nomi-ink--worried .nomi-ink__ear-r{animation:none;transform:rotate(-11deg)}

/* excited sparkles */
.nomi-ink__spark{opacity:0;transform-origin:center}
.nomi-ink__spark1{animation:nomi-ink-spark 1.3s ease-in-out infinite}
.nomi-ink__spark2{animation:nomi-ink-spark 1.3s ease-in-out infinite .45s}
.nomi-ink__spark3{animation:nomi-ink-spark 1.3s ease-in-out infinite .85s}
@keyframes nomi-ink-spark{0%,100%{opacity:0;transform:scale(.3)}50%{opacity:1;transform:scale(1)}}

/* thinking: ink droplets rise and bloom away */
.nomi-ink__drop{opacity:0}
.nomi-ink--thinking .nomi-ink__drop1{animation:nomi-ink-drop 2.6s ease-in-out infinite}
.nomi-ink--thinking .nomi-ink__drop2{animation:nomi-ink-drop 2.6s ease-in-out infinite .85s}
.nomi-ink--thinking .nomi-ink__drop3{animation:nomi-ink-drop 2.6s ease-in-out infinite 1.7s}
@keyframes nomi-ink-drop{0%{opacity:0;transform:translateY(0) scale(.5)}20%{opacity:.9;transform:translateY(-4px) scale(1)}70%{opacity:.55;transform:translateY(-16px) scale(1.15)}100%{opacity:0;transform:translateY(-24px) scale(2.1)}}

@media(prefers-reduced-motion:reduce){.nomi-ink *{animation-duration:6s!important}}
      `}),i.jsxs("svg",{viewBox:"0 0 160 160",width:n,height:n,children:[i.jsxs("defs",{children:[i.jsxs("radialGradient",{id:"inkBody",cx:"42%",cy:"30%",r:"78%",children:[i.jsx("stop",{offset:"0%",stopColor:"#52525f"}),i.jsx("stop",{offset:"42%",stopColor:"#3a3a44"}),i.jsx("stop",{offset:"100%",stopColor:"#1f1f26"})]}),i.jsxs("linearGradient",{id:"inkTail",x1:"0",y1:"0",x2:"1",y2:"1",children:[i.jsx("stop",{offset:"0%",stopColor:"#3c3c46"}),i.jsx("stop",{offset:"100%",stopColor:"#222229"})]}),i.jsxs("radialGradient",{id:"inkEye",cx:"42%",cy:"34%",r:"72%",children:[i.jsx("stop",{offset:"0%",stopColor:"#ffe39a"}),i.jsx("stop",{offset:"48%",stopColor:"#f0b346"}),i.jsx("stop",{offset:"100%",stopColor:"#d4902a"})]}),i.jsxs("radialGradient",{id:"inkBlush",cx:"50%",cy:"50%",r:"50%",children:[i.jsx("stop",{offset:"0%",stopColor:"#e9849a",stopOpacity:".85"}),i.jsx("stop",{offset:"100%",stopColor:"#e9849a",stopOpacity:"0"})]}),i.jsxs("radialGradient",{id:"inkEarIn",cx:"50%",cy:"40%",r:"65%",children:[i.jsx("stop",{offset:"0%",stopColor:"#4a4a58"}),i.jsx("stop",{offset:"100%",stopColor:"#2a2a32"})]})]}),i.jsx("ellipse",{className:"nomi-ink__shadow",cx:"80",cy:"147",rx:"34",ry:"7",fill:"#000",opacity:".13"}),i.jsxs("g",{className:"nomi-ink__body-g",children:[i.jsxs("g",{className:"nomi-ink__tail",children:[i.jsx("path",{d:"M104 134 C122 132 128 122 124 112 C120 124 108 130 92 132 C70 142 48 142 42 134 C44 144 70 148 92 142",fill:"none",stroke:"url(#inkTail)",strokeWidth:"12",strokeLinecap:"round"}),i.jsx("circle",{cx:"44",cy:"137",r:"6.5",fill:"#f4f1ea"}),i.jsx("circle",{cx:"42.5",cy:"135",r:"2.4",fill:"#fff",opacity:".7"})]}),i.jsx("path",{d:"M80 60 C58 60 47 78 46 100 C45 116 49 132 56 138 C64 145 96 145 104 138 C111 132 115 116 114 100 C113 78 102 60 80 60 Z",fill:"url(#inkBody)",stroke:"#15151b",strokeWidth:"2.4",strokeLinejoin:"round"}),i.jsx("path",{d:"M80 63 C60 63 50 80 49 100 C48 114 52 129 58 135",fill:"none",stroke:"#5a5a6e",strokeWidth:"2.2",strokeLinecap:"round",opacity:".6"}),i.jsx("path",{d:"M73 116 C73 126 87 126 87 116 C84 122 76 122 73 116 Z",fill:"#f4f1ea",opacity:".92"}),i.jsxs("g",{children:[i.jsxs("g",{className:"nomi-ink__ear-l",children:[i.jsx("path",{d:"M52 52 L58 26 L74 46 Z",fill:"url(#inkBody)",stroke:"#15151b",strokeWidth:"2.3",strokeLinejoin:"round"}),i.jsx("path",{d:"M58 46 L61 33 L69 45 Z",fill:"url(#inkEarIn)"})]}),i.jsxs("g",{className:"nomi-ink__ear-r",children:[i.jsx("path",{d:"M108 52 L102 26 L86 46 Z",fill:"url(#inkBody)",stroke:"#15151b",strokeWidth:"2.3",strokeLinejoin:"round"}),i.jsx("path",{d:"M102 46 L99 33 L91 45 Z",fill:"url(#inkEarIn)"})]}),i.jsx("path",{d:"M80 36 C57 36 44 52 44 72 C44 92 60 102 80 102 C100 102 116 92 116 72 C116 52 103 36 80 36 Z",fill:"url(#inkBody)",stroke:"#15151b",strokeWidth:"2.4",strokeLinejoin:"round"}),i.jsx("path",{d:"M80 39 C60 39 47 53 47 71",fill:"none",stroke:"#5a5a6e",strokeWidth:"2.2",strokeLinecap:"round",opacity:".55"}),i.jsx("ellipse",{cx:"72",cy:"50",rx:"15",ry:"8",fill:"#fff",opacity:".08"}),i.jsxs("g",{className:"nomi-ink__blush",children:[i.jsx("ellipse",{cx:"56",cy:"80",rx:"8",ry:"5",fill:"url(#inkBlush)"}),i.jsx("ellipse",{cx:"104",cy:"80",rx:"8",ry:"5",fill:"url(#inkBlush)"})]}),i.jsx("g",{className:"nomi-ink__eyes",children:t?i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M55 74 C60 80 70 80 75 74",fill:"none",stroke:"#f0b346",strokeWidth:"3",strokeLinecap:"round"}),i.jsx("path",{d:"M85 74 C90 80 100 80 105 74",fill:"none",stroke:"#f0b346",strokeWidth:"3",strokeLinecap:"round"})]}):o==="happy"?i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M54 76 C60 68 70 68 76 76",fill:"none",stroke:"#f0b346",strokeWidth:"4.5",strokeLinecap:"round"}),i.jsx("path",{d:"M84 76 C90 68 100 68 106 76",fill:"none",stroke:"#f0b346",strokeWidth:"4.5",strokeLinecap:"round"})]}):o==="worried"?i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M53 67 L73 71",fill:"none",stroke:"#15151b",strokeWidth:"2.4",strokeLinecap:"round"}),i.jsx("path",{d:"M107 67 L87 71",fill:"none",stroke:"#15151b",strokeWidth:"2.4",strokeLinecap:"round"}),i.jsx("ellipse",{cx:"65",cy:"78",rx:"6.5",ry:"8.5",fill:"url(#inkEye)"}),i.jsx("ellipse",{cx:"95",cy:"78",rx:"6.5",ry:"8.5",fill:"url(#inkEye)"}),i.jsx("ellipse",{cx:"65",cy:"79",rx:"2.3",ry:"6.5",fill:"#1a1208"}),i.jsx("ellipse",{cx:"95",cy:"79",rx:"2.3",ry:"6.5",fill:"#1a1208"}),i.jsx("circle",{cx:"63",cy:"75",r:"1.9",fill:"#fff"}),i.jsx("circle",{cx:"93",cy:"75",r:"1.9",fill:"#fff"})]}):o==="excited"?i.jsxs(i.Fragment,{children:[i.jsx("ellipse",{cx:"65",cy:"78",rx:"9.5",ry:"11",fill:"url(#inkEye)"}),i.jsx("ellipse",{cx:"95",cy:"78",rx:"9.5",ry:"11",fill:"url(#inkEye)"}),i.jsx("ellipse",{cx:"65",cy:"79",rx:"5",ry:"8.5",fill:"#1a1208"}),i.jsx("ellipse",{cx:"95",cy:"79",rx:"5",ry:"8.5",fill:"#1a1208"}),i.jsx("circle",{cx:"62",cy:"74",r:"3",fill:"#fff"}),i.jsx("circle",{cx:"92",cy:"74",r:"3",fill:"#fff"}),i.jsx("circle",{cx:"68",cy:"82",r:"1.6",fill:"#fff",opacity:".85"}),i.jsx("circle",{cx:"98",cy:"82",r:"1.6",fill:"#fff",opacity:".85"})]}):i.jsxs(i.Fragment,{children:[i.jsx("ellipse",{cx:"65",cy:"78",rx:"8",ry:"10.5",fill:"url(#inkEye)"}),i.jsx("ellipse",{cx:"95",cy:"78",rx:"8",ry:"10.5",fill:"url(#inkEye)"}),i.jsx("ellipse",{cx:"65",cy:"79",rx:"2.6",ry:"8",fill:"#1a1208"}),i.jsx("ellipse",{cx:"95",cy:"79",rx:"2.6",ry:"8",fill:"#1a1208"}),i.jsx("circle",{cx:"62",cy:"74",r:"2.2",fill:"#fff"}),i.jsx("circle",{cx:"92",cy:"74",r:"2.2",fill:"#fff"}),i.jsx("circle",{cx:"67",cy:"82",r:"1.3",fill:"#fff",opacity:".7"}),i.jsx("circle",{cx:"97",cy:"82",r:"1.3",fill:"#fff",opacity:".7"})]})}),i.jsx("path",{d:"M77 88 L83 88 L80 92 Z",fill:"#e58aa0"}),o==="worried"?i.jsx("path",{d:"M72 99 C76 95 84 95 88 99",fill:"none",stroke:"#15151b",strokeWidth:"2",strokeLinecap:"round"}):o==="happy"||o==="excited"?i.jsx("path",{d:"M70 94 C74 102 86 102 90 94",fill:"none",stroke:"#15151b",strokeWidth:"2.2",strokeLinecap:"round"}):i.jsx("path",{d:"M80 92 C76 97 73 96 71 94 M80 92 C84 97 87 96 89 94",fill:"none",stroke:"#15151b",strokeWidth:"1.9",strokeLinecap:"round"}),i.jsxs("g",{stroke:"#15151b",strokeWidth:"1.3",strokeLinecap:"round",opacity:".5",children:[i.jsx("path",{d:"M50 84 L36 82"}),i.jsx("path",{d:"M50 89 L37 90"}),i.jsx("path",{d:"M110 84 L124 82"}),i.jsx("path",{d:"M110 89 L123 90"})]})]})]}),t&&i.jsxs("g",{fill:"#cfcfdc",fontFamily:"Georgia, serif",fontStyle:"italic",fontWeight:"700",children:[i.jsx("text",{className:"nomi-ink__z nomi-ink__z1",x:"108",y:"40",fontSize:"13",children:"z"}),i.jsx("text",{className:"nomi-ink__z nomi-ink__z2",x:"116",y:"30",fontSize:"17",children:"z"})]}),o==="worried"&&i.jsx("path",{className:"nomi-ink__sweat",d:"M112 56 C112 52 116 50 116 50 C116 50 120 52 120 56 C120 59 118 61 116 61 C114 61 112 59 112 56 Z",fill:"#9fdcf5",stroke:"#6bb8df",strokeWidth:"1"}),o==="excited"&&i.jsxs("g",{fill:"#ffe39a",children:[i.jsx("path",{className:"nomi-ink__spark nomi-ink__spark1",d:"M118 44 l2 5 5 2 -5 2 -2 5 -2 -5 -5 -2 5 -2 Z"}),i.jsx("path",{className:"nomi-ink__spark nomi-ink__spark2",d:"M36 50 l1.5 4 4 1.5 -4 1.5 -1.5 4 -1.5 -4 -4 -1.5 4 -1.5 Z"}),i.jsx("path",{className:"nomi-ink__spark nomi-ink__spark3",d:"M126 70 l1.2 3 3 1.2 -3 1.2 -1.2 3 -1.2 -3 -3 -1.2 3 -1.2 Z"})]}),e==="thinking"&&i.jsxs("g",{fill:"#2b2b33",opacity:".78",children:[i.jsx("path",{className:"nomi-ink__drop nomi-ink__drop1",d:"M78 28 C78 24 80 20 80 20 C80 20 82 24 82 28 C82 31 80.5 33 80 33 C79.5 33 78 31 78 28 Z"}),i.jsx("path",{className:"nomi-ink__drop nomi-ink__drop2",d:"M88 30 C88 27 89.5 24 89.5 24 C89.5 24 91 27 91 30 C91 32 90 33.5 89.5 33.5 C89 33.5 88 32 88 30 Z"}),i.jsx("path",{className:"nomi-ink__drop nomi-ink__drop3",d:"M70 31 C70 28 71.5 25 71.5 25 C71.5 25 73 28 73 31 C73 33 72 34.5 71.5 34.5 C71 34.5 70 33 70 31 Z"})]})]})]})};/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const z=({mood:o,activity:e,size:n=150})=>{const t=o==="sleepy",a=e==="thinking";return i.jsxs("div",{className:`nomi-ch nomi-bolt nomi-bolt--${o} nomi-bolt--${e}`,style:{width:n,height:n},children:[i.jsx("style",{children:`
        .nomi-bolt{position:relative;display:inline-block}
        .nomi-bolt svg{display:block;overflow:visible}
        .nomi-bolt *{transform-box:view-box}
        /* float = whole-body hover */
        .nomi-bolt__body{transform-origin:80px 150px;animation:nomi-bolt-float 3.6s ease-in-out infinite}
        .nomi-bolt__shadow{transform-origin:80px 150px;animation:nomi-bolt-shadow 3.6s ease-in-out infinite}
        .nomi-bolt__jet{transform-origin:80px 132px;animation:nomi-bolt-jet 1.5s ease-in-out infinite}
        .nomi-bolt__antBall{transform-origin:80px 26px;animation:nomi-bolt-ant 3.6s ease-in-out infinite}
        .nomi-bolt__armL{transform-origin:44px 96px;animation:nomi-bolt-armL 3.6s ease-in-out infinite}
        .nomi-bolt__armR{transform-origin:116px 96px;animation:nomi-bolt-armR 3.6s ease-in-out infinite}
        .nomi-bolt__eyes{transform-origin:80px 78px;animation:nomi-bolt-glance 6.5s ease-in-out infinite}
        .nomi-bolt__eyeL,.nomi-bolt__eyeR{transform-origin:80px 76px;animation:nomi-bolt-blink 5s ease-in-out infinite}
        .nomi-bolt__face{animation:nomi-bolt-flicker 4.5s ease-in-out infinite}

        @keyframes nomi-bolt-float{0%,100%{transform:translateY(3px)}50%{transform:translateY(-4px)}}
        @keyframes nomi-bolt-shadow{0%,100%{transform:scaleX(.92) scaleY(.92);opacity:.13}50%{transform:scaleX(1.12) scaleY(1.12);opacity:.08}}
        @keyframes nomi-bolt-jet{0%,100%{transform:scaleX(.85) scaleY(.7);opacity:.55}50%{transform:scaleX(1.12) scaleY(1.15);opacity:.95}}
        @keyframes nomi-bolt-ant{0%,100%{transform:rotate(-7deg)}50%{transform:rotate(7deg)}}
        @keyframes nomi-bolt-armL{0%,100%{transform:rotate(0deg)}50%{transform:rotate(-6deg)}}
        @keyframes nomi-bolt-armR{0%,100%{transform:rotate(0deg)}50%{transform:rotate(6deg)}}
        @keyframes nomi-bolt-blink{0%,42%,48%,100%{transform:scaleY(1)}45%{transform:scaleY(.08)}}
        @keyframes nomi-bolt-glance{0%,18%,82%,100%{transform:translateX(0)}30%,46%{transform:translateX(-5px)}58%,74%{transform:translateX(5px)}}
        @keyframes nomi-bolt-flicker{0%,100%{opacity:1}50%{opacity:.92}}

        /* happy: small bounce + brighter screen */
        .nomi-bolt--happy .nomi-bolt__body{animation:nomi-bolt-bounce 1.8s ease-in-out infinite}
        @keyframes nomi-bolt-bounce{0%,100%{transform:translateY(2px)}50%{transform:translateY(-6px)}}

        /* excited: big hover + sparkles */
        .nomi-bolt--excited .nomi-bolt__body{animation:nomi-bolt-exfloat 1.4s ease-in-out infinite}
        @keyframes nomi-bolt-exfloat{0%,100%{transform:translateY(4px)}50%{transform:translateY(-12px)}}
        .nomi-bolt--excited .nomi-bolt__shadow{animation:nomi-bolt-exshadow 1.4s ease-in-out infinite}
        @keyframes nomi-bolt-exshadow{0%,100%{transform:scaleX(.86);opacity:.13}50%{transform:scaleX(1.22);opacity:.06}}
        .nomi-bolt__spark{animation:nomi-bolt-spark 1.2s ease-in-out infinite}
        .nomi-bolt__spark--b{animation-delay:.4s}
        .nomi-bolt__spark--c{animation-delay:.8s}
        @keyframes nomi-bolt-spark{0%,100%{opacity:0;transform:scale(.4)}50%{opacity:1;transform:scale(1)}}

        /* sleepy: sink + dim + z */
        .nomi-bolt--sleepy .nomi-bolt__body{animation:nomi-bolt-sink 4.5s ease-in-out infinite}
        @keyframes nomi-bolt-sink{0%,100%{transform:translateY(6px)}50%{transform:translateY(3px)}}
        .nomi-bolt--sleepy .nomi-bolt__face{animation:none;opacity:.62}
        .nomi-bolt--sleepy .nomi-bolt__jet{animation:nomi-bolt-jetlow 3s ease-in-out infinite;opacity:.4}
        @keyframes nomi-bolt-jetlow{0%,100%{transform:scaleY(.5);opacity:.3}50%{transform:scaleY(.7);opacity:.5}}
        .nomi-bolt__z{opacity:0}
        .nomi-bolt--sleepy .nomi-bolt__z{animation:nomi-bolt-z 3s ease-in-out infinite}
        .nomi-bolt--sleepy .nomi-bolt__z--b{animation-delay:1.5s}
        @keyframes nomi-bolt-z{0%{opacity:0;transform:translate(0,0) scale(.6)}30%{opacity:.9}100%{opacity:0;transform:translate(7px,-22px) scale(1.1)}}

        /* worried: antenna droops, sweat slides */
        .nomi-bolt--worried .nomi-bolt__antBall{animation:none;transform:rotate(22deg) translateY(3px)}
        .nomi-bolt__sweat{opacity:0}
        .nomi-bolt--worried .nomi-bolt__sweat{animation:nomi-bolt-sweat 2.8s ease-in-out infinite}
        @keyframes nomi-bolt-sweat{0%{opacity:0;transform:translateY(0) scale(.7)}18%{opacity:.95}80%{opacity:.85}100%{opacity:0;transform:translateY(16px) scale(1)}}

        /* thinking: loading dots + antenna blink */
        .nomi-bolt__dot{opacity:0}
        .nomi-bolt--thinking .nomi-bolt__dot{animation:nomi-bolt-dot 1.4s ease-in-out infinite}
        .nomi-bolt--thinking .nomi-bolt__dot--b{animation-delay:.25s}
        .nomi-bolt--thinking .nomi-bolt__dot--c{animation-delay:.5s}
        @keyframes nomi-bolt-dot{0%,80%,100%{opacity:.18}40%{opacity:1}}
        .nomi-bolt--thinking .nomi-bolt__antBall{animation:nomi-bolt-antblink 1s ease-in-out infinite}
        @keyframes nomi-bolt-antblink{0%,100%{opacity:1}50%{opacity:.45}}
      `}),i.jsxs("svg",{viewBox:"0 0 160 160",width:n,height:n,children:[i.jsxs("defs",{children:[i.jsxs("radialGradient",{id:"boltBody",cx:"40%",cy:"32%",r:"75%",children:[i.jsx("stop",{offset:"0%",stopColor:"#ffffff"}),i.jsx("stop",{offset:"55%",stopColor:"#f4f7f5"}),i.jsx("stop",{offset:"100%",stopColor:"#d9e4e0"})]}),i.jsxs("radialGradient",{id:"boltScreen",cx:"50%",cy:"40%",r:"70%",children:[i.jsx("stop",{offset:"0%",stopColor:"#28323f"}),i.jsx("stop",{offset:"100%",stopColor:"#1e2733"})]}),i.jsxs("radialGradient",{id:"boltAnt",cx:"38%",cy:"35%",r:"70%",children:[i.jsx("stop",{offset:"0%",stopColor:"#ffd9a0"}),i.jsx("stop",{offset:"60%",stopColor:"#ffb454"}),i.jsx("stop",{offset:"100%",stopColor:"#e8902f"})]}),i.jsxs("radialGradient",{id:"boltJet",cx:"50%",cy:"20%",r:"80%",children:[i.jsx("stop",{offset:"0%",stopColor:"#d7fff4",stopOpacity:".95"}),i.jsx("stop",{offset:"55%",stopColor:"#bfeee0",stopOpacity:".6"}),i.jsx("stop",{offset:"100%",stopColor:"#bfeee0",stopOpacity:"0"})]}),i.jsxs("radialGradient",{id:"boltBlush",cx:"50%",cy:"50%",r:"50%",children:[i.jsx("stop",{offset:"0%",stopColor:"#ff9ab0",stopOpacity:".55"}),i.jsx("stop",{offset:"100%",stopColor:"#ff9ab0",stopOpacity:"0"})]}),i.jsxs("filter",{id:"boltGlow",x:"-60%",y:"-60%",width:"220%",height:"220%",children:[i.jsx("feGaussianBlur",{stdDeviation:"1.6",result:"b"}),i.jsxs("feMerge",{children:[i.jsx("feMergeNode",{in:"b"}),i.jsx("feMergeNode",{in:"SourceGraphic"})]})]})]}),i.jsx("ellipse",{className:"nomi-bolt__shadow",cx:"80",cy:"150",rx:"34",ry:"7",fill:"#000000",opacity:".12"}),i.jsxs("g",{className:"nomi-bolt__body",children:[i.jsx("ellipse",{className:"nomi-bolt__jet",cx:"80",cy:"132",rx:"26",ry:"12",fill:"url(#boltJet)"}),i.jsx("ellipse",{cx:"80",cy:"128",rx:"17",ry:"5",fill:"#eafdf7",opacity:".55"}),i.jsx("path",{d:"M80 44 Q78 32 80 24",fill:"none",stroke:"#c3d2cd",strokeWidth:"3",strokeLinecap:"round"}),i.jsxs("g",{className:"nomi-bolt__antBall",children:[i.jsx("circle",{cx:"80",cy:"22",r:"7",fill:"url(#boltAnt)",stroke:"#d98724",strokeWidth:"2"}),i.jsx("circle",{cx:"77.5",cy:"19.5",r:"2.2",fill:"#fff",opacity:".7"})]}),i.jsx("g",{className:"nomi-bolt__armL",children:i.jsx("ellipse",{cx:"38",cy:"100",rx:"9",ry:"13",fill:"url(#boltBody)",stroke:"#b9c9c3",strokeWidth:"2.2"})}),i.jsx("g",{className:"nomi-bolt__armR",children:i.jsx("ellipse",{cx:"122",cy:"100",rx:"9",ry:"13",fill:"url(#boltBody)",stroke:"#b9c9c3",strokeWidth:"2.2"})}),i.jsx("path",{d:`M80 40
               C112 40 130 62 130 92
               C130 120 110 134 80 134
               C50 134 30 120 30 92
               C30 62 48 40 80 40 Z`,fill:"url(#boltBody)",stroke:"#b9c9c3",strokeWidth:"2.4",strokeLinejoin:"round"}),i.jsx("path",{d:"M52 120 Q80 132 108 120",fill:"none",stroke:"#bfeee0",strokeWidth:"5",strokeLinecap:"round",opacity:".85"}),i.jsx("ellipse",{cx:"66",cy:"62",rx:"20",ry:"11",fill:"#ffffff",opacity:".55"}),i.jsx("rect",{x:"44",y:"58",width:"72",height:"52",rx:"20",fill:"url(#boltScreen)",stroke:"#141b24",strokeWidth:"2.2"}),i.jsx("rect",{x:"48",y:"62",width:"38",height:"15",rx:"8",fill:"#39506a",opacity:".35"}),i.jsx("ellipse",{cx:"56",cy:"98",rx:"8",ry:"5",fill:"url(#boltBlush)"}),i.jsx("ellipse",{cx:"104",cy:"98",rx:"8",ry:"5",fill:"url(#boltBlush)"}),i.jsx("g",{className:"nomi-bolt__face",filter:"url(#boltGlow)",children:a?i.jsxs(i.Fragment,{children:[i.jsx("circle",{className:"nomi-bolt__dot",cx:"66",cy:"84",r:"5",fill:"#37e0ff"}),i.jsx("circle",{className:"nomi-bolt__dot nomi-bolt__dot--b",cx:"80",cy:"84",r:"5",fill:"#37e0ff"}),i.jsx("circle",{className:"nomi-bolt__dot nomi-bolt__dot--c",cx:"94",cy:"84",r:"5",fill:"#37e0ff"})]}):t?i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M58 80 Q66 88 74 80",fill:"none",stroke:"#37e0ff",strokeWidth:"3.2",strokeLinecap:"round"}),i.jsx("path",{d:"M86 80 Q94 88 102 80",fill:"none",stroke:"#37e0ff",strokeWidth:"3.2",strokeLinecap:"round"}),i.jsx("path",{d:"M72 98 Q80 102 88 98",fill:"none",stroke:"#37e0ff",strokeWidth:"2.6",strokeLinecap:"round"})]}):o==="excited"?i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M66 74 l2.4 5 5.4.6 -4 3.7 1.1 5.3 -4.9-2.8 -4.9 2.8 1.1-5.3 -4-3.7 5.4-.6 z",fill:"#37e0ff"}),i.jsx("path",{d:"M94 74 l2.4 5 5.4.6 -4 3.7 1.1 5.3 -4.9-2.8 -4.9 2.8 1.1-5.3 -4-3.7 5.4-.6 z",fill:"#37e0ff"}),i.jsx("path",{d:"M70 96 Q80 106 90 96 Q80 100 70 96 Z",fill:"#37e0ff"})]}):o==="worried"?i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M58 78 L72 74",fill:"none",stroke:"#37e0ff",strokeWidth:"3",strokeLinecap:"round"}),i.jsx("path",{d:"M102 78 L88 74",fill:"none",stroke:"#37e0ff",strokeWidth:"3",strokeLinecap:"round"}),i.jsx("circle",{cx:"66",cy:"86",r:"4.6",fill:"#37e0ff"}),i.jsx("circle",{cx:"94",cy:"86",r:"4.6",fill:"#37e0ff"}),i.jsx("path",{d:"M71 100 Q80 94 89 100",fill:"none",stroke:"#37e0ff",strokeWidth:"2.8",strokeLinecap:"round"})]}):o==="happy"?i.jsxs(i.Fragment,{children:[i.jsx("path",{d:"M58 86 Q66 78 74 86",fill:"none",stroke:"#37e0ff",strokeWidth:"3.4",strokeLinecap:"round"}),i.jsx("path",{d:"M86 86 Q94 78 102 86",fill:"none",stroke:"#37e0ff",strokeWidth:"3.4",strokeLinecap:"round"}),i.jsx("path",{d:"M70 96 Q80 104 90 96",fill:"none",stroke:"#37e0ff",strokeWidth:"2.8",strokeLinecap:"round"})]}):i.jsxs("g",{className:"nomi-bolt__eyes",children:[i.jsx("ellipse",{className:"nomi-bolt__eyeL",cx:"66",cy:"84",rx:"5",ry:"6.5",fill:"#37e0ff"}),i.jsx("ellipse",{className:"nomi-bolt__eyeR",cx:"94",cy:"84",rx:"5",ry:"6.5",fill:"#37e0ff"}),i.jsx("circle",{cx:"64",cy:"81.5",r:"1.6",fill:"#d7fbff"}),i.jsx("circle",{cx:"92",cy:"81.5",r:"1.6",fill:"#d7fbff"}),i.jsx("path",{d:"M73 99 Q80 104 87 99",fill:"none",stroke:"#37e0ff",strokeWidth:"2.6",strokeLinecap:"round"})]})}),o==="excited"&&i.jsxs("g",{fill:"#ffe08a",children:[i.jsx("path",{className:"nomi-bolt__spark",d:"M120 56 l1.6 4 4 1.6 -4 1.6 -1.6 4 -1.6-4 -4-1.6 4-1.6 z"}),i.jsx("path",{className:"nomi-bolt__spark nomi-bolt__spark--b",d:"M40 60 l1.3 3.2 3.2 1.3 -3.2 1.3 -1.3 3.2 -1.3-3.2 -3.2-1.3 3.2-1.3 z"}),i.jsx("path",{className:"nomi-bolt__spark nomi-bolt__spark--c",d:"M114 92 l1.2 3 3 1.2 -3 1.2 -1.2 3 -1.2-3 -3-1.2 3-1.2 z"})]}),o==="worried"&&i.jsx("path",{className:"nomi-bolt__sweat",d:"M112 64 q4 6 0 9 a4.6 4.6 0 1 1 0-9 z",fill:"#7fd4ff",opacity:".85"}),t&&i.jsxs("g",{fill:"none",stroke:"#9fb0c4",strokeWidth:"2.4",strokeLinecap:"round",strokeLinejoin:"round",children:[i.jsx("path",{className:"nomi-bolt__z",d:"M104 50 h8 l-8 9 h8",transform:"scale(.8)","transform-origin":"108 55"}),i.jsx("path",{className:"nomi-bolt__z nomi-bolt__z--b",d:"M114 44 h6 l-6 7 h6"})]})]})]})]})};/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const u={s:150,m:210,l:280},v=140,B=400,_=400,$=240,w=14,Z=64;function E(o){const e=Number.isFinite(o.aspect)&&o.aspect>0?o.aspect:1;let n=Number.isFinite(o.sizePx)&&o.sizePx>0?Math.min(B,Math.max(v,o.sizePx)):u[o.sizeTier]??u.m,t=Math.ceil(n*e)+w*2;return t>_&&(t=_,n=Math.floor((_-w*2)/e)),t=Math.max(t,$),{windowWidth:t,windowHeight:n+Z,figureHeight:n}}/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const C=[{id:"mochi",nameKey:"mochi",palette:["#fff6f0","#ffb7c9"],Component:N},{id:"ink",nameKey:"ink",palette:["#2b2b33","#e8b04b"],Component:W},{id:"bolt",nameKey:"bolt",palette:["#bfeee0","#37e0ff"],Component:z}],q="mochi",M=o=>C.find(e=>e.id===o)??C[0],I={windowWidth:240,windowHeight:214,figureHeight:150},S=o=>M(o).desk??I,g="custom",U=(o,e)=>o===g&&e?E(e):S(o);/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const h=o=>typeof o=="number"&&Number.isFinite(o);function H(o){var r;if(!o||o.character!==g)return null;const e=(r=o.appearance)==null?void 0:r.custom_figure;if(!e)return null;const n=e.head_box;if(!h(e.aspect)||e.aspect<=0||!n||!h(n.x)||!h(n.y)||!h(n.w)||n.w<=0)return null;const t=h(n.h)&&n.h>0?n.h:n.w*e.aspect,a=e.size_tier==="s"||e.size_tier==="l"?e.size_tier:"m",s=h(e.size_px)&&e.size_px>0?e.size_px:void 0,c=typeof e.figure_id=="string"&&e.figure_id?e.figure_id:void 0;return{aspect:e.aspect,headBox:{x:n.x,y:n.y,w:n.w,h:t},sizeTier:a,sizePx:s,figureId:c}}function F(o,e,n){const t=encodeURIComponent(`${n.aspect}-${n.headBox.x}-${n.headBox.y}-${n.headBox.w}-${n.headBox.h}`);return n.figureId?`${o}/api/companion/figures/${n.figureId}/image?v=${t}`:`${o}/api/companion/companions/${e}/figure?v=${t}`}function P(o,e,n){const t=n!=null?`?v=${encodeURIComponent(String(n))}`:"";return`${o}/api/companion/figures/${e}/image${t}`}function G(o,e,n){const t=o.h>0?o.h:o.w*e,a=o.w*e/t,s=a>=1?n:n*a,c=a>=1?n/a:n,r=s/o.w,l=c/t;return{width:r,height:l,left:(n-s)/2-o.x*r,top:(n-c)/2-o.y*l}}/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const j=new WeakMap;function Q(o,e){j.set(o,e)}function A(o){j.delete(o)}function K(o){return j.get(o)}function X(o,e=64,n=96,t=16){try{const a=document.createElement("canvas");a.width=e,a.height=n;const s=a.getContext("2d",{willReadFrequently:!0});if(!s)return null;s.drawImage(o,0,0,e,n);const c=s.getImageData(0,0,e,n).data,r=new Uint8Array(e*n);for(let l=0;l<e*n;l++)r[l]=c[l*4+3];return(l,k)=>{if(l<0||l>1||k<0||k>1)return!1;const x=Math.min(e-1,Math.max(0,Math.round(l*(e-1)))),y=Math.min(n-1,Math.max(0,Math.round(k*(n-1))));for(let m=-1;m<=1;m++)for(let f=-1;f<=1;f++){const p=x+f,d=y+m;if(!(p<0||p>=e||d<0||d>=n)&&r[d*e+p]>t)return!0}return!1}}catch{return null}}const O=130,R=`
.nomi-cfig { position: relative; line-height: 0; }
.nomi-cfig img { display: block; -webkit-user-drag: none; }

/* ground shadow, pulsing against the breath */
.nomi-cfig__shadow {
  position: absolute; left: 27%; bottom: 0.2%; width: 46%; height: 3.2%;
  background: radial-gradient(closest-side, rgba(0,0,0,.16), rgba(0,0,0,0));
  animation: nomi-cfig-shadow 3.6s ease-in-out infinite;
}
@keyframes nomi-cfig-shadow { 0%,100% { transform: scaleX(1); opacity: 1; } 50% { transform: scaleX(.95); opacity: .72; } }

/* figure wrapper rig — CSS-only breathing + mood posture (no WebGL: a per-frame
   GPU canvas flickers in the transparent always-on-top window; see file header) */
.nomi-cfig__all { position: relative; width: 100%; height: 100%; transform-origin: 50% 100%; animation: nomi-cfig-breathe 3.6s ease-in-out infinite; }
@keyframes nomi-cfig-breathe { 0%,100% { transform: scaleY(1); } 50% { transform: scaleY(1.008) translateY(-1px); } }
.nomi-cfig--happy .nomi-cfig__all { animation: nomi-cfig-hop 1.7s ease-in-out infinite; }
@keyframes nomi-cfig-hop {
  0%,100% { transform: translateY(0); }
  30% { transform: translateY(-7px); }
  55% { transform: translateY(0) scaleY(.997); }
}
.nomi-cfig--excited .nomi-cfig__all { animation: nomi-cfig-jump 1.15s ease-in-out infinite; }
@keyframes nomi-cfig-jump {
  0%,100% { transform: translateY(0); }
  40% { transform: translateY(-12px); }
  62% { transform: translateY(-8px); }
  82% { transform: translateY(1px) scaleY(.995); }
}
.nomi-cfig--sleepy .nomi-cfig__all { animation: nomi-cfig-doze 4.6s ease-in-out infinite; }
@keyframes nomi-cfig-doze { 0%,100% { transform: translateY(1.5px) rotate(.35deg); } 50% { transform: translateY(3px) rotate(-.35deg); } }
.nomi-cfig--worried .nomi-cfig__all { animation: nomi-cfig-worry 2.6s ease-in-out infinite; }
@keyframes nomi-cfig-worry { 0%,100% { transform: translateY(2px) scaleY(.998); } 50% { transform: translateY(3px) scaleY(1.002); } }

.nomi-cfig__img { position: absolute; inset: 0; width: 100%; height: 100%; }

/* bust crop is a fixed camera window — light breathing only, never translate */
.nomi-cfig__bust-img { position: absolute; max-width: none; transform-origin: 50% 100%; animation: nomi-cfig-breathe 3.6s ease-in-out infinite; }

/* particle fx overlay (viewBox 0 0 944 1000) */
.nomi-cfig__fx { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; overflow: visible; }
.nomi-cfig__fx-el { transform-box: fill-box; transform-origin: 50% 50%; }
.nomi-cfig__z, .nomi-cfig__sweat, .nomi-cfig__spark, .nomi-cfig__leaf { opacity: 0; }
.nomi-cfig--sleepy .nomi-cfig__z1 { animation: nomi-cfig-z 3s ease-in-out infinite; }
.nomi-cfig--sleepy .nomi-cfig__z2 { animation: nomi-cfig-z 3s ease-in-out infinite 1.5s; }
@keyframes nomi-cfig-z {
  0% { opacity: 0; transform: translate(0,0) scale(.6); }
  25% { opacity: .9; }
  70% { opacity: .65; }
  100% { opacity: 0; transform: translate(32px,-68px) scale(1.1); }
}
.nomi-cfig--worried .nomi-cfig__sweat { animation: nomi-cfig-sweat 2.6s ease-in-out infinite; }
@keyframes nomi-cfig-sweat {
  0%,14% { opacity: 0; transform: translateY(0); }
  24% { opacity: .85; }
  72% { opacity: .85; }
  100% { opacity: 0; transform: translateY(45px); }
}
.nomi-cfig--excited .nomi-cfig__spark1 { animation: nomi-cfig-spark 1.1s ease-in-out infinite; }
.nomi-cfig--excited .nomi-cfig__spark2 { animation: nomi-cfig-spark 1.1s ease-in-out infinite .38s; }
.nomi-cfig--excited .nomi-cfig__spark3 { animation: nomi-cfig-spark 1.1s ease-in-out infinite .72s; }
@keyframes nomi-cfig-spark { 0%,100% { opacity: 0; transform: scale(.4); } 45% { opacity: 1; transform: scale(1.1); } }
.nomi-cfig--thinking .nomi-cfig__leaf1 { animation: nomi-cfig-leaf 3.4s ease-in-out infinite; }
.nomi-cfig--thinking .nomi-cfig__leaf2 { animation: nomi-cfig-leaf 3.4s ease-in-out infinite 1.7s; }
@keyframes nomi-cfig-leaf {
  0% { opacity: 0; transform: translate(23px,-46px) rotate(0deg); }
  12% { opacity: .95; }
  50% { transform: translate(-18px,82px) rotate(160deg); opacity: .9; }
  82% { opacity: .75; }
  100% { opacity: 0; transform: translate(18px,205px) rotate(330deg); }
}
`,b=(o,e,n,t)=>`M ${o} ${e-n} L ${o+t} ${e-t} L ${o+n} ${e} L ${o+t} ${e+t} L ${o} ${e+n} L ${o-t} ${e+t} L ${o-n} ${e} L ${o-t} ${e-t} Z`,D=({src:o,aspect:e,headBox:n,mood:t,activity:a,size:s=150,hitRef:c})=>{const r=s<=O;Y.useEffect(()=>{if(r||!c)return;let y=!1,m=null;const f=new Image;return f.crossOrigin="anonymous",f.onload=()=>{if(y)return;const p=c.current;if(!p)return;const d=X(f);d&&(Q(p,d),m=p)},f.src=o,()=>{y=!0,m&&(A(m),m=null)}},[r,o,c]);const l=r?s:Math.round(s*e),k=s,x=G(n,e,s);return i.jsxs("div",{className:`nomi-ch nomi-cfig nomi-cfig--${t} nomi-cfig--${a}${r?" nomi-cfig--bust":""}`,style:{width:l,height:k,overflow:r?"hidden":"visible"},children:[i.jsx("style",{children:R}),r?i.jsx("img",{className:"nomi-cfig__bust-img",src:o,alt:"",draggable:!1,style:{width:x.width,height:x.height,left:x.left,top:x.top}}):i.jsxs(i.Fragment,{children:[i.jsx("div",{className:"nomi-cfig__shadow"}),i.jsx("div",{className:"nomi-cfig__all",children:i.jsx("img",{className:"nomi-cfig__img",src:o,alt:"",draggable:!1})}),i.jsxs("svg",{className:"nomi-cfig__fx",viewBox:"0 0 944 1000","aria-hidden":"true",children:[i.jsxs("g",{fontFamily:"Georgia, serif",fontWeight:"700",fill:"#b78757",children:[i.jsx("text",{className:"nomi-cfig__fx-el nomi-cfig__z nomi-cfig__z1",x:"660",y:"90",fontSize:"40",children:"z"}),i.jsx("text",{className:"nomi-cfig__fx-el nomi-cfig__z nomi-cfig__z2",x:"720",y:"55",fontSize:"54",children:"z"})]}),i.jsx("path",{className:"nomi-cfig__fx-el nomi-cfig__sweat",d:"M640 95 q 14 18 0 30 q -14 -12 0 -30 Z",fill:"#9ed4f2",stroke:"#6fb6dd",strokeWidth:"3"}),i.jsxs("g",{fill:"#ffd23f",children:[i.jsx("path",{className:"nomi-cfig__fx-el nomi-cfig__spark nomi-cfig__spark1",d:b(180,330,26,7)}),i.jsx("path",{className:"nomi-cfig__fx-el nomi-cfig__spark nomi-cfig__spark2",d:b(800,430,21,6)}),i.jsx("path",{className:"nomi-cfig__fx-el nomi-cfig__spark nomi-cfig__spark3",d:b(150,700,17,5)})]}),i.jsx("g",{className:"nomi-cfig__fx-el nomi-cfig__leaf nomi-cfig__leaf1",children:i.jsx("path",{d:"M560 60 q 25 -18 45 0 q -20 25 -45 0 Z",fill:"#c84a3a",stroke:"#9c3528",strokeWidth:"3"})}),i.jsx("g",{className:"nomi-cfig__fx-el nomi-cfig__leaf nomi-cfig__leaf2",children:i.jsx("path",{d:"M360 90 q 20 -15 38 0 q -17 21 -38 0 Z",fill:"#d8693a",stroke:"#a8462a",strokeWidth:"2.5"})})]})]})]})},J=({character:o,mood:e,activity:n,size:t,companionId:a,customFigure:s,figureHitRef:c})=>{if(o===g&&a&&s){const l=F(L(),a,s);return i.jsx(D,{src:l,aspect:s.aspect,headBox:s.headBox,mood:e,activity:n,size:t,hitRef:c},l)}const{Component:r}=M(o);return i.jsx(r,{mood:e,activity:n,size:t})};export{J as C,q as D,u as F,B as S,U as a,G as b,H as c,g as d,C as e,P as f,K as g,v as h};
