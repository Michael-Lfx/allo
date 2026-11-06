/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Slide wireframes for the home card wall. Each one is a tiny SVG used as a CSS
 * mask (alpha only, drawn in black with varying opacity), so the visible ink
 * color comes from the theme and one data URI serves every card that uses it.
 * No text: these are pictograms, nothing to translate.
 */

/**
 * Patterns are drawn on a 72×42 board, but the viewBox is
 * cropped to the area they actually use so the picture fills the card.
 */
const WIDE_VIEWBOX = '4 4 64 34';
const svg = (body: string, viewBox: string) =>
  `url("data:image/svg+xml;utf8,${encodeURIComponent(
    `<svg xmlns='http://www.w3.org/2000/svg' viewBox='${viewBox}'>${body}</svg>`
  )}")`;

const line = (x: number, y: number, w: number, o = 0.32) =>
  `<rect x='${x}' y='${y}' width='${w}' height='1.6' rx='0.8' fill-opacity='${o}'/>`;
const title = (x = 6, y = 6, w = 20) => `<rect x='${x}' y='${y}' width='${w}' height='2.6' rx='1.3' fill-opacity='0.6'/>`;
const stroke = (d: string, o = 0.5, w = 1.1) =>
  `<path d='${d}' fill='none' stroke='#000' stroke-opacity='${o}' stroke-width='${w}' stroke-linecap='round' stroke-linejoin='round'/>`;
const mountain = (x: number, y: number, s: number, o = 0.28) =>
  `<path d='M${x} ${y} l${5 * s} ${-6 * s} l${3 * s} ${3 * s} l${3 * s} ${-4 * s} l${5 * s} ${7 * s}z' fill-opacity='${o}'/>`;

const WIDE_BODIES: string[] = [
  // bar chart
  title() +
    [8, 12, 10, 15, 18, 13, 21]
      .map((h, i) => `<rect x='${10 + i * 7.5}' y='${36 - h}' width='4.5' height='${h}' rx='0.8' fill-opacity='${i === 6 ? 0.75 : 0.4}'/>`)
      .join(''),
  // venn
  stroke('M4 21h8M60 21h8', 0.35) +
    `<circle cx='30' cy='21' r='10' fill='none' stroke='#000' stroke-opacity='0.45' stroke-width='1.1'/>` +
    `<circle cx='42' cy='21' r='10' fill='none' stroke='#000' stroke-opacity='0.45' stroke-width='1.1'/>`,
  // area line chart
  title() +
    `<path d='M8 32 L18 28 L28 30 L38 22 L48 24 L58 16 L66 14 L66 36 L8 36Z' fill-opacity='0.12'/>` +
    stroke('M8 32 L18 28 L28 30 L38 22 L48 24 L58 16 L66 14', 0.6),
  // donut + legend
  `<circle cx='20' cy='21' r='9' fill='none' stroke='#000' stroke-opacity='0.18' stroke-width='3.6'/>` +
    stroke('M20 12 A9 9 0 1 1 11.5 24', 0.6, 3.6) +
    line(38, 15, 24, 0.45) +
    line(38, 21, 18) +
    line(38, 27, 21),
  // process chevrons
  title() +
    [8, 22, 36, 50]
      .map((x, i) => `<path d='M${x} 17h9l3 4.5-3 4.5h-9l3-4.5z' fill-opacity='${[0.25, 0.35, 0.65, 0.35][i]}'/>`)
      .join('') +
    [9, 23, 37, 51].map((x) => line(x, 31, 9, 0.25)).join(''),
  // timeline
  `<rect x='8' y='21' width='56' height='0.8' fill-opacity='0.35'/>` +
    [12, 28, 44, 60].map((cx, i) => `<circle cx='${cx}' cy='21.4' r='2.4' fill-opacity='${i === 2 ? 0.8 : 0.45}'/>`).join('') +
    [7, 23, 39, 55].map((x, i) => line(x, i % 2 ? 13 : 28, 10, 0.3)).join(''),
  // bullet list
  title() +
    [0, 1, 2, 3].map((i) => `<circle cx='9' cy='${15 + i * 6}' r='1.1' fill-opacity='0.5'/>` + line(13, 14.2 + i * 6, 34 - i * 4)).join(''),
  // two columns
  title() + [26, 22, 25, 17].map((w, i) => line(6, 15 + i * 5, w)).join('') + [26, 20, 24, 22].map((w, i) => line(40, 15 + i * 5, w)).join(''),
  // picture + text
  `<rect x='6' y='8' width='28' height='26' rx='1.5' fill-opacity='0.1'/>` +
    mountain(6, 34, 1.75) +
    `<circle cx='28' cy='14' r='2.2' fill-opacity='0.3'/>` +
    title(40, 12, 22) +
    line(40, 19, 24) +
    line(40, 24, 19) +
    line(40, 29, 22),
  // table
  title() +
    `<rect x='6' y='13' width='60' height='3' rx='0.8' fill-opacity='0.32'/>` +
    [19, 24, 29, 34].map((y) => [6, 22, 38, 54].map((x) => line(x, y, 11, 0.24)).join('')).join(''),
  // big number
  `<rect x='8' y='10' width='9' height='18' rx='2.6' fill-opacity='0.45'/>` +
    `<rect x='19.5' y='25' width='3' height='3' rx='1.5' fill-opacity='0.45'/>` +
    `<rect x='25' y='10' width='9' height='18' rx='2.6' fill-opacity='0.45'/>` +
    line(8, 33, 26) +
    line(44, 16, 20, 0.4) +
    line(44, 21, 16),
  // quote
  `<circle cx='11' cy='12' r='2' fill-opacity='0.45'/><circle cx='17' cy='12' r='2' fill-opacity='0.45'/>` +
    stroke('M10 13.5 l-1 3.5M16 13.5 l-1 3.5', 0.45, 1.2) +
    line(10, 21, 50, 0.45) +
    line(10, 26, 42, 0.45) +
    line(42, 33, 18, 0.28),
  // funnel
  `<path d='M14 9h44l-5 7H19z' fill-opacity='0.2'/><path d='M20 18h32l-4 7H24z' fill-opacity='0.35'/><path d='M25 27h22l-3 7H28z' fill-opacity='0.55'/>`,
  // pyramid + labels
  `<path d='M8 34 L22 10 L36 34Z' fill-opacity='0.12'/><path d='M17.3 18 L22 10 L26.7 18Z' fill-opacity='0.55'/>` +
    `<path d='M14 24 H30 L33.3 30 H10.7Z' fill-opacity='0.3'/>` +
    line(42, 14, 22, 0.45) +
    line(42, 21, 18) +
    line(42, 28, 20),
  // three cards
  title() +
    [6, 28, 50]
      .map((x) => `<rect x='${x}' y='14' width='17' height='21' rx='1.5' fill-opacity='0.1'/><circle cx='${x + 4}' cy='18.5' r='1.6' fill-opacity='0.5'/>` + line(x + 2.5, 24, 12) + line(x + 2.5, 28.5, 9))
      .join(''),
  // pie
  `<circle cx='21' cy='21' r='10' fill-opacity='0.16'/><path d='M21 21 L21 11 A10 10 0 0 1 30.5 24.2Z' fill-opacity='0.55'/>` +
    [0, 1, 2].map((i) => `<circle cx='41' cy='${15.8 + i * 5.5}' r='1.1' fill-opacity='0.45'/>` + line(44, 15 + i * 5.5, 18)).join(''),
  // scatter + trend
  stroke('M8 8 V34 H64', 0.32, 0.8) +
    [
      [14, 30],
      [20, 27],
      [25, 28],
      [31, 23],
      [37, 22],
      [43, 18],
      [50, 16],
      [57, 12],
    ]
      .map(([cx, cy]) => `<circle cx='${cx}' cy='${cy}' r='1.3' fill-opacity='0.55'/>`)
      .join('') +
    `<path d='M12 31 L60 11' stroke='#000' stroke-opacity='0.3' stroke-width='0.8' stroke-dasharray='2 1.5'/>`,
  // kpi circles
  title() + [16, 36, 56].map((cx) => `<circle cx='${cx}' cy='21' r='4.2' fill-opacity='0.35'/>` + line(cx - 6, 30, 12, 0.26)).join(''),
  // section number
  `<rect x='8' y='14' width='9' height='14' rx='2.4' fill-opacity='0.4'/><rect x='19.5' y='14' width='9' height='14' rx='2.4' fill-opacity='0.4'/>` +
    line(36, 18, 26, 0.5) +
    line(36, 23.5, 18),
  // title slide
  `<rect x='18' y='16' width='36' height='3.4' rx='1.7' fill-opacity='0.6'/>` + line(24, 24, 24, 0.34) + line(30, 30, 12, 0.24),
  // comparison
  `<rect x='6' y='9' width='27' height='25' rx='1.5' fill-opacity='0.1'/><rect x='39' y='9' width='27' height='25' rx='1.5' fill-opacity='0.1'/>` +
    title(9, 13, 12) +
    title(42, 13, 12) +
    line(9, 20, 20) +
    line(9, 25, 16) +
    line(42, 20, 20) +
    line(42, 25, 18) +
    `<circle cx='36' cy='21.5' r='2' fill-opacity='0.55'/>`,
  // horizontal bars
  title() + [50, 38, 44, 26].map((w, i) => `<rect x='6' y='${14 + i * 5}' width='${w}' height='3' rx='0.8' fill-opacity='${i === 0 ? 0.7 : 0.32}'/>`).join(''),
  // closing scribble
  stroke('M20 21 c3 -7 6 -7 6 0 s4 7 7 0 s4 -7 7 0 s4 7 7 0 s3 -5 5 -2', 0.42, 1.1) + line(30, 29, 12, 0.24),
  // full-bleed hero
  `<rect x='0' y='0' width='72' height='42' fill-opacity='0.08'/>` + mountain(28, 42, 2.75, 0.14) + title(6, 26, 26) + line(6, 32, 18, 0.4),
];

export const WIDE_PATTERNS: readonly string[] = WIDE_BODIES.map((body) => svg(body, WIDE_VIEWBOX));
