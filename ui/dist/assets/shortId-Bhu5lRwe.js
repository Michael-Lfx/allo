/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */const n=/^[a-z]+_([0-9a-z]{8,})$/i,e=t=>{const o=n.exec(t);if(o)return o[1];const s=t.split(/[\\/]/).filter(Boolean).pop()??t;return s.length>24?`…${s.slice(-24)}`:s};export{e as s};
