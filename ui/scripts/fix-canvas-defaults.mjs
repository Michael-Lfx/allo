import fs from 'node:fs';
import path from 'node:path';

const roots = [
  'c:/flowy-workspace/code/migrants/allo/ui/src/renderer/pages/videoCanvas',
  'c:/flowy-workspace/code/migrants/allo/crates/backend/nomifun-canvas/src',
];

function walk(dir, acc = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, acc);
    else if (/\.(ts|tsx|rs)$/.test(entry.name)) acc.push(full);
  }
  return acc;
}

const replacements = [
  ['backgroundMode || "dots"', 'backgroundMode || "lines"'],
  ["backgroundMode || 'dots'", "backgroundMode || 'lines'"],
  ['backgroundMode: "dots"', 'backgroundMode: "lines"'],
  ["backgroundMode: 'dots'", "backgroundMode: 'lines'"],
  ['backgroundMode = "dots"', 'backgroundMode = "lines"'],
  ['useState<CanvasBackgroundMode>("dots")', 'useState<CanvasBackgroundMode>("lines")'],
  ['useState<"lines" | "dots" | "blank">("dots")', 'useState<"lines" | "dots" | "blank">("lines")'],
  ['"backgroundMode": "dots"', '"backgroundMode": "lines"'],
  ["?? canvasThemes.dark", "?? canvasThemes.light"],
];

for (const root of roots) {
  for (const file of walk(root)) {
    let content = fs.readFileSync(file, 'utf8');
    let next = content;
    for (const [from, to] of replacements) next = next.split(from).join(to);
    if (next !== content) {
      fs.writeFileSync(file, next, 'utf8');
      console.log('updated', path.relative(process.cwd(), file));
    }
  }
}
