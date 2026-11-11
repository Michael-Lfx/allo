/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Home "slides" mode: a few ppt-master choices the user picks before the first
 * turn. They travel as one plain, editable header line at the top of the
 * message (what you see is what Nomi gets); the managed ppt-master Skill and
 * `nomi_presentation_status` turn them into ppt-master's own fields.
 */

import { themeDirective, type SlideThemeId } from './slideThemes';

/** ppt-master registered canvas ids (references/canvas-formats.md). */
export type SlideCanvas = 'ppt169' | 'ppt43' | 'xiaohongshu' | 'moments' | 'story' | 'a4';
export type SlidePages = 'auto' | '5-8' | '10-15' | '20+';
export type SlideMode = 'quick' | 'guided';

/**
 * Template choice for the deck:
 * - `free`: ppt-master's free design (no fixed look).
 * - `theme`: one visual theme from the home gallery (visual style + palette +
 *   typography, see slideThemes.ts).
 * - `import`: a reference file the user attached; Nomi first turns it into a
 *   project-scoped template workspace with Create Template, then uses it.
 */
export type SlideTemplate =
  | { kind: 'free' }
  | { kind: 'theme'; id: SlideThemeId }
  | { kind: 'import'; path: string };

export interface SlideOptions {
  canvas: SlideCanvas;
  pages: SlidePages;
  mode: SlideMode;
  template: SlideTemplate;
}

export const FREE_SLIDE_TEMPLATE: SlideTemplate = { kind: 'free' };

export const DEFAULT_SLIDE_OPTIONS: SlideOptions = {
  canvas: 'ppt169',
  pages: 'auto',
  mode: 'quick',
  template: FREE_SLIDE_TEMPLATE,
};

/** File types Create Template reads as a reference (PPTX, PDF pages, images). */
export const SLIDE_TEMPLATE_IMPORT_EXTENSIONS: readonly string[] = ['pptx', 'pdf', 'png', 'jpg', 'jpeg', 'webp'];

/** Last path segment, for either separator. */
export function fileBaseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export function isSameSlideTemplate(a: SlideTemplate, b: SlideTemplate): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === 'theme' && b.kind === 'theme') return a.id === b.id;
  if (a.kind === 'import' && b.kind === 'import') return a.path === b.path;
  return true;
}

export const SLIDE_CANVASES: readonly SlideCanvas[] = ['ppt169', 'ppt43', 'xiaohongshu', 'moments', 'story', 'a4'];
export const SLIDE_PAGES: readonly SlidePages[] = ['auto', '5-8', '10-15', '20+'];
export const SLIDE_MODES: readonly SlideMode[] = ['quick', 'guided'];

/** Leading tag of the header line; also how an existing header is found again. */
export const SLIDE_HEADER_TAG = '[ppt-master]';

/** Model-facing wording, language-neutral on purpose (enum ids are stable). */
const MODE_DIRECTIVE: Record<SlideMode, string> = {
  quick: 'route=Quick, decide confirmations yourself',
  guided: 'route=Default, confirm each ⛔ gate with me in this chat',
};

/**
 * Template part of the header, or '' for free design. The header must stay one
 * line, so file names are flattened.
 */
function templateDirective(template: SlideTemplate): string {
  switch (template.kind) {
    case 'free':
      return '';
    case 'theme':
      return themeDirective(template.id);
    case 'import': {
      const name = fileBaseName(template.path).replace(/[\r\n]+/g, ' ');
      return (
        `template=import "${name}" (attached; first run Create Template with project scope on it, ` +
        'then generate this deck from that workspace root); '
      );
    }
  }
}

/** `[ppt-master] canvas=ppt169; pages=auto; [look=… | template=…; ]route=Quick, …. Check nomi_presentation_status first.` */
export function buildSlideHeader(options: SlideOptions): string {
  return (
    `${SLIDE_HEADER_TAG} canvas=${options.canvas}; pages=${options.pages}; ` +
    `${templateDirective(options.template)}${MODE_DIRECTIVE[options.mode]}. ` +
    'Make this deck with the ppt-master Skill; call nomi_presentation_status first.'
  );
}

/** Text after the header line, or the whole text when there is no header. */
export function stripSlideHeader(input: string): string {
  if (!input.startsWith(SLIDE_HEADER_TAG)) return input;
  const newline = input.indexOf('\n');
  return newline === -1 ? '' : input.slice(newline + 1);
}

/** Put (or replace) the header line on top of `input`, keeping the user's text. */
export function applySlideHeader(input: string, options: SlideOptions): string {
  const body = stripSlideHeader(input);
  return `${buildSlideHeader(options)}\n${body}`;
}

/** Whether the user typed anything besides the header. */
export function hasSlideBody(input: string): boolean {
  return stripSlideHeader(input).trim().length > 0;
}
