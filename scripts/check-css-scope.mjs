#!/usr/bin/env node
/**
 * 路由级 CSS 作用域门禁 / Route-level CSS scope guard
 *
 * 背景:画布子系统 (videoCanvas `oc/` + videoGeneration) 是从独立项目移植过来的,
 * 它的 globals.css 曾经 `@import "tailwindcss"` 并直接写 `:root` / `html, body,
 * #root, .ant-app` / `*` 规则。这些样式表随懒加载 chunk 注入后不会被移除,于是
 * Tailwind preflight 与 token 覆盖泄漏到其他所有路由:composer 图标 `svg` 从
 * inline 变 block 而偏移、暗色下 body 被刷成白色、--primary/--background 被改写。
 *
 * 契约:移植子系统的 CSS 必须挂在 `.oc-root` 作用域下;
 *  - 禁止裸根选择器 `:root` / `html` / `body` / `#root` / `*`(允许 `body.app-x`
 *    这类带 class/属性的限定形态,以及 `.oc-root *` 这类作用域内后代选择器);
 *  - 禁止全量 `@import "tailwindcss"`(允许 theme.css / utilities.css 分拆导入);
 *  - 禁止全局引入 `antd/dist/reset.css`(画布基线由 oc-scoped-preflight.css 承担)。
 *
 * 扫描范围:`ui/src/renderer/pages/videoCanvas/**` 与
 * `ui/src/renderer/pages/videoGeneration/**` 下的全部 .css,外加画布兼容层
 * `ui/src/renderer/styles/canvas-utility-shield.css`,以及 ui/src 下
 * 全部 .ts/.tsx 的禁用 import(测试文件除外:断言里的反例字符串不算违规)。
 *
 * 用法 / Usage:
 *   bun scripts/check-css-scope.mjs             # 校验,发现违规 exit 1
 *   bun scripts/check-css-scope.mjs --self-test # 校验器自测
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, dirname, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..');
const UI_SRC = join(ROOT, 'ui', 'src');
const CSS_SCAN_DIRS = [
  join(UI_SRC, 'renderer', 'pages', 'videoCanvas'),
  join(UI_SRC, 'renderer', 'pages', 'videoGeneration'),
];
const EXTRA_CSS_FILES = [join(UI_SRC, 'renderer', 'styles', 'canvas-utility-shield.css')];

const GLOBAL_ROOT_RE = /^(?::root\b|html\b|body\b|#root\b|\*)/;
const FULL_TAILWIND_IMPORT_RE = /@import\s+["']tailwindcss(?:\/preflight\.css)?["']/;
const ANTD_RESET_IMPORT_RE = /antd\/dist\/reset\.css/;

function* walk(dir, test) {
  for (const name of readdirSync(dir)) {
    if (name === 'node_modules' || name === 'dist' || name.startsWith('.')) continue;
    const full = join(dir, name);
    const st = statSync(full);
    if (st.isDirectory()) yield* walk(full, test);
    else if (test(name)) yield full;
  }
}

/** 把注释替换成等长空白,保留行号。 */
function blankComments(source) {
  return source.replace(/\/\*[\s\S]*?\*\//g, (match) => match.replace(/[^\n]/g, ' '));
}

function lineOf(source, index) {
  return source.slice(0, index).split('\n').length;
}

/** 返回 [{line, selector}] 形式的裸根选择器违规。 */
function scanCssSource(rawSource) {
  const source = blankComments(rawSource);
  const violations = [];
  let depth = 0;
  let preludeStart = 0;
  for (let i = 0; i < source.length; i += 1) {
    const ch = source[i];
    if (ch === '{') {
      const prelude = source.slice(preludeStart, i).trim();
      if (!prelude.startsWith('@')) {
        for (const selector of prelude.split(',')) {
          const sel = selector.trim().replace(/\s+/g, ' ');
          if (!sel || !GLOBAL_ROOT_RE.test(sel)) continue;
          const matched = GLOBAL_ROOT_RE.exec(sel)[0];
          const rest = sel.slice(matched.length).trimStart();
          const qualified = rest.startsWith('.') || rest.startsWith('[');
          if (!qualified) violations.push({ line: lineOf(source, preludeStart), selector: sel });
        }
      }
      depth += 1;
      preludeStart = i + 1;
    } else if (ch === '}') {
      depth -= 1;
      preludeStart = i + 1;
    } else if (ch === ';' && depth === 0) {
      preludeStart = i + 1;
    }
  }
  return violations;
}

/** 返回全量 tailwind / antd reset 导入违规。 */
function scanImports(source) {
  const violations = [];
  if (FULL_TAILWIND_IMPORT_RE.test(source)) violations.push('全量 @import "tailwindcss"');
  if (ANTD_RESET_IMPORT_RE.test(source)) violations.push('全局 antd/dist/reset.css');
  return violations;
}

function selfTest() {
  const cases = [
    { src: '.oc-root *, .oc-root ::before { margin: 0 }', bad: 0 },
    { src: ':root { --a: 1 }', bad: 1 },
    { src: 'html:not(.dark) .x { color: red }', bad: 1 },
    { src: 'body.app-spatial-overlays .x { color: red }', bad: 0 },
    { src: "body[arco-theme='dark'] .x { color: red }", bad: 0 },
    { src: '* { box-sizing: border-box }', bad: 1 },
    { src: '@media (min-width: 1px) { :root { --a: 1 } }', bad: 1 },
    { src: '.foo * { margin: 0 }', bad: 0 },
    { src: '#root { position: fixed }', bad: 1 },
    { src: 'html { line-height: 1.5 }', bad: 1 },
    { src: '@keyframes spin { from { opacity: 0 } to { opacity: 1 } }', bad: 0 },
    { src: '@layer base { .oc-root textarea { resize: none } }', bad: 0 },
  ];
  let failed = 0;
  cases.forEach(({ src, bad }, index) => {
    const got = scanCssSource(src).length;
    if (got !== bad) {
      failed += 1;
      console.error(`self-test case ${index} failed: expected ${bad}, got ${got}\n  ${src}`);
    }
  });
  const importCases = [
    { src: '@import "tailwindcss";', bad: 1 },
    { src: '@import "tailwindcss/preflight.css";', bad: 1 },
    { src: '@import "tailwindcss/theme.css" layer(theme);', bad: 0 },
    { src: "@import 'antd/dist/reset.css'", bad: 1 },
  ];
  importCases.forEach(({ src, bad }, index) => {
    const got = scanImports(src).length;
    if (got !== bad) {
      failed += 1;
      console.error(`self-test import case ${index} failed: expected ${bad}, got ${got}\n  ${src}`);
    }
  });
  if (failed > 0) {
    console.error(`❌ check-css-scope self-test: ${failed}/${cases.length + importCases.length} case(s) failed`);
    process.exit(1);
  }
  console.log(`✅ check-css-scope self-test: ${cases.length + importCases.length} cases pass`);
}

if (process.argv.includes('--self-test')) {
  selfTest();
  process.exit(0);
}

const problems = [];
let cssScanned = 0;
let importsScanned = 0;

function scanCssFile(file) {
  const source = readFileSync(file, 'utf8');
  for (const v of scanCssSource(source)) {
    problems.push(`${relative(ROOT, file)}:${v.line} 裸根选择器 "${v.selector}"`);
  }
  for (const v of scanImports(blankComments(source))) {
    problems.push(`${relative(ROOT, file)} ${v}`);
  }
}

for (const dir of CSS_SCAN_DIRS) {
  for (const file of walk(dir, (name) => name.endsWith('.css'))) {
    cssScanned += 1;
    scanCssFile(file);
  }
}

for (const file of EXTRA_CSS_FILES) {
  cssScanned += 1;
  scanCssFile(file);
}

for (const file of walk(UI_SRC, (name) => /\.(ts|tsx)$/.test(name) && !/\.(test|spec)\./.test(name))) {
  importsScanned += 1;
  const source = readFileSync(file, 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, (match) => match.replace(/[^\n]/g, ' '))
    .replace(/^[ \t]*\/\/.*$/gm, (match) => match.replace(/[^\n]/g, ' '));
  for (const v of scanImports(source)) {
    problems.push(`${relative(ROOT, file)} ${v}`);
  }
}

if (problems.length > 0) {
  console.error('❌ 路由级 CSS 作用域违规(详见 scripts/check-css-scope.mjs 头注):');
  for (const p of problems) console.error(`  ${p}`);
  process.exit(1);
}
console.log(`✅ css scope clean (${cssScanned} css file(s), ${importsScanned} ts/tsx file(s) scanned)`);
