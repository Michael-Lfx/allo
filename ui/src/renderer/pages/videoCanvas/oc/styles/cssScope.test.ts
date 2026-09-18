import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8');
const stripCssComments = (css: string) => css.replace(/\/\*[\s\S]*?\*\//g, '');

const globals = stripCssComments(read('./globals.css'));
const preflight = stripCssComments(read('./oc-scoped-preflight.css'));
const projectPage = read('../../ProjectPage.tsx');
const shield = stripCssComments(read('../../../../styles/canvas-utility-shield.css'));
const mainEntry = read('../../../../main.tsx');

describe('oc css scope containment', () => {
    test('globals.css never imports full tailwind or leaks bare root selectors', () => {
        expect(/@import\s+["']tailwindcss["']/.test(globals)).toBe(false);
        expect(globals.includes('@import "tailwindcss/theme.css" layer(theme);')).toBe(true);
        expect(globals.includes('@import "tailwindcss/utilities.css" layer(utilities) source(none);')).toBe(true);
        expect(/^\s*(:root|html|body|#root|\*)\s*[,{]/m.test(globals)).toBe(false);
        expect(/^\s*\.dark\s*\{/m.test(globals)).toBe(false);
        expect(globals.includes('.oc-root.dark {')).toBe(true);
        expect(globals.includes('html:not(.dark)')).toBe(false);
    });

    test('scoped preflight binds every rule to .oc-root', () => {
        const selectors = (preflight.match(/[^{}]+\{/g) ?? [])
            .map((chunk) => chunk.trim())
            .filter((selector) => !selector.startsWith('@'));
        expect(selectors.length).toBeGreaterThan(30);
        expect(selectors.every((selector) => selector.includes('.oc-root'))).toBe(true);
    });

    test('tailwind only scans the ported canvas trees', () => {
        expect(globals.includes('source(none)')).toBe(true);
        expect(globals.includes('@source "..";')).toBe(true);
        expect(globals.includes('@source "../../videoGeneration";')).toBe(true);
    });

    test('utility shield splits Tailwind and UnoCSS transform semantics by scope', () => {
        expect(mainEntry.includes("import './styles/canvas-utility-shield.css';")).toBe(true);
        expect(shield.includes(':where(:not(.oc-root):not(.oc-root *))')).toBe(true);
        expect(shield.includes('translate: none;')).toBe(true);
        expect(shield.includes('--un-translate-x: 0 !important;')).toBe(true);
        expect(shield.includes('--un-scale-x: 1 !important;')).toBe(true);
    });

    test('canvas route keeps the scope class and portals through the shared host', () => {
        expect(projectPage.includes('oc-root oc-shell')).toBe(true);
        expect(projectPage.includes('getOcPortalHost')).toBe(true);
        expect(projectPage.includes('antd/dist/reset.css')).toBe(false);
        expect(projectPage.includes('documentElement.classList')).toBe(false);
        expect(projectPage.includes("import '@oc/styles/globals.css';")).toBe(true);
    });
});
