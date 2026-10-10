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
        expect(preflight.includes('.oc-root fieldset')).toBe(true);
        expect(preflight.includes('.oc-root legend')).toBe(true);
    });

    test('tailwind only scans the ported canvas trees', () => {
        expect(globals.includes('source(none)')).toBe(true);
        expect(globals.includes('@source "..";')).toBe(true);
        expect(globals.includes('@source "../../videoGeneration";')).toBe(true);
    });

    test('utility shield splits Tailwind and UnoCSS transform semantics by scope', () => {
        expect(mainEntry.includes("import './styles/canvas-utility-shield.css';")).toBe(true);
        expect(shield.includes(':where(:not(.oc-canvas):not(.oc-canvas *))')).toBe(true);
        expect(shield.includes('.oc-canvas,')).toBe(true);
        expect(shield.includes('translate: none;')).toBe(true);
        expect(shield.includes('--un-translate-x: 0 !important;')).toBe(true);
        expect(shield.includes('--un-scale-x: 1 !important;')).toBe(true);
        expect(shield.includes('data-wallpaper')).toBe(false);
    });

    test('canvas route keeps the scope class and portals through the shared host', () => {
        expect(projectPage.includes('oc-root oc-shell oc-canvas')).toBe(true);
        expect(projectPage.includes('data-canvas-theme={colorTheme}')).toBe(true);
        expect(projectPage.includes('getOcPortalHost')).toBe(true);
        expect(projectPage.includes('disposeOcPortalHost')).toBe(true);
        expect(projectPage.includes('antd/dist/reset.css')).toBe(false);
        expect(projectPage.includes('documentElement.classList')).toBe(false);
        expect(projectPage.includes("import '@oc/styles/globals.css';")).toBe(true);
        expect(globals.includes('.oc-portal-host')).toBe(true);
        expect(globals.includes('z-index: var(--z-popover)')).toBe(true);
    });

    test('canvas persist theme store stays React-free; portal host follows the canvas shell', () => {
        const themeStore = read('../stores/use-theme-store.ts');
        const colorTheme = read('../stores/use-canvas-color-theme.ts');
        const ocScope = read('../lib/oc-scope.ts');
        expect(themeStore.includes('from "react"')).toBe(false);
        expect(themeStore.includes('createContext')).toBe(false);
        expect(colorTheme.includes('CanvasColorThemeScope')).toBe(true);
        expect(colorTheme.includes('useCanvasColorTheme')).toBe(true);
        expect(projectPage.includes("from '@oc/stores/use-canvas-color-theme'")).toBe(true);
        expect(ocScope.includes('.oc-root.oc-canvas:not(.oc-portal-host)')).toBe(true);
    });

    test('canvas title control avoids Uno text-base color and uses canvas-scoped title classes', () => {
        const topBar = read('../pages/canvas/canvas-project-top-bar.tsx');
        expect(topBar.includes('canvas-title-label')).toBe(true);
        expect(topBar.includes('canvas-title-input')).toBe(true);
        expect(topBar.includes('text-base')).toBe(false);
        expect(topBar.includes('titleColor')).toBe(true);
        expect(topBar.includes('#ffffff')).toBe(true);
        const project = read('../pages/canvas/project.tsx');
        expect(project.includes('CanvasColorThemeScope')).toBe(true);
        expect(project.includes('data-canvas-theme')).toBe(true);
    });
});
