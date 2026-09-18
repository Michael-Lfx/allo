/**
 * One-off port helper from open-ai-canvas. Do not reintroduce photo lookbook packs:
 * canvas style covers are CSS palettes from canvas-style-system.ts.
 */
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const src = path.resolve(root, '../../open-ai-canvas/web/src/components/canvas/canvas-style-picker-modal.tsx');
const dest = path.resolve(root, 'src/renderer/pages/videoCanvas/oc/components/canvas/canvas-style-picker-modal.tsx');

let s = fs.readFileSync(src, 'utf8');
s = s.replaceAll('from "@/', 'from "@oc/').replaceAll("from '@/", "from '@oc/");

// Ensure Modal mounts on body and sits above canvas chrome.
s = s.replace(
  '<Modal rootClassName="canvas-style-picker-modal" open={open}',
  '<Modal rootClassName="canvas-style-picker-modal" open={open} getContainer={() => document.body} zIndex={1200}',
);
s = s.replace(
  '<Modal rootClassName="canvas-style-detail-modal" open={open}',
  '<Modal rootClassName="canvas-style-detail-modal" open={open} getContainer={() => document.body} zIndex={1210}',
);

fs.writeFileSync(dest, s);
console.log('wrote', dest);
