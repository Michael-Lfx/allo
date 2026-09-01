#!/usr/bin/env node
/**
 * Original briefing compositor. Reads beats.json + timing.json and emits
 * PPM stills + ffmpeg MP4. Original news cards only — not Remotion and not
 * third-party licensed card source. ffmpeg essentials often cannot decode SVG, so stills
 * are PPM (always readable) and clips are lavfi/image2, not SVG loops.
 *
 * Visual language: ink-navy desk, brass gold, paper cream, signal red.
 * Card roles follow a news-briefing shot list (open / evidence / highlight /
 * numeral / ticker / chip / wipe / lower-third) with original geometry.
 */
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync, existsSync, readdirSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const catalog = JSON.parse(readFileSync(join(here, 'catalog.json'), 'utf8'));
const WIDTH = 1920;
const HEIGHT = 1080;
const TAIL_GUARD_SECS = 0.5;

const INK = [18, 28, 42];
const NAVY = [12, 22, 36];
const CREAM = [236, 226, 204];
const BRASS = [196, 154, 78];
const SIGNAL = [176, 48, 52];
const SLATE = [42, 58, 74];
const PAPER = [248, 241, 226];
const CHIP = [28, 40, 54];

function argValue(flag) {
  const idx = process.argv.indexOf(flag);
  return idx >= 0 ? process.argv[idx + 1] : null;
}

function ffmpeg(args) {
  return spawnSync('ffmpeg', args, { encoding: 'utf8' });
}

function ffmpegOk(result) {
  return result.status === 0;
}

function escapeDrawtext(text) {
  return String(text || '')
    .replace(/\\/g, '\\\\')
    .replace(/:/g, '\\:')
    .replace(/'/g, "\\'")
    .replace(/%/g, '\\%')
    .replace(/[\r\n]+/g, ' ')
    .slice(0, 72);
}

function findFont() {
  const candidates = [
    process.env.NOMIFUN_BRIEFING_FONT,
    'C:\\Windows\\Fonts\\msyh.ttc',
    'C:\\Windows\\Fonts\\msyhbd.ttc',
    'C:\\Windows\\Fonts\\simhei.ttf',
    'C:\\Windows\\Fonts\\arial.ttf',
    '/System/Library/Fonts/PingFang.ttc',
    '/System/Library/Fonts/Supplemental/Arial Unicode.ttf',
    '/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc',
    '/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc',
    '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',
  ].filter(Boolean);
  return candidates.find((path) => existsSync(path)) ?? null;
}

function fillRect(pixels, x0, y0, w, h, color) {
  const [r, g, b] = color;
  const x1 = Math.max(0, Math.min(WIDTH, Math.round(x0)));
  const y1 = Math.max(0, Math.min(HEIGHT, Math.round(y0)));
  const x2 = Math.max(0, Math.min(WIDTH, Math.round(x0 + w)));
  const y2 = Math.max(0, Math.min(HEIGHT, Math.round(y0 + h)));
  for (let y = y1; y < y2; y += 1) {
    let offset = (y * WIDTH + x1) * 3;
    for (let x = x1; x < x2; x += 1) {
      pixels[offset] = r;
      pixels[offset + 1] = g;
      pixels[offset + 2] = b;
      offset += 3;
    }
  }
}

function fillBand(pixels, color) {
  const [r, g, b] = color;
  for (let i = 0; i < pixels.length; i += 3) {
    pixels[i] = r;
    pixels[i + 1] = g;
    pixels[i + 2] = b;
  }
}

function paintCard(pixels, card) {
  switch (card) {
    case 'title_desk':
      fillBand(pixels, NAVY);
      fillRect(pixels, 0, 0, WIDTH, 168, CREAM);
      fillRect(pixels, 0, 168, WIDTH, 8, BRASS);
      fillRect(pixels, 0, 0, 28, HEIGHT, SIGNAL);
      fillRect(pixels, 72, 36, 220, 10, BRASS);
      fillRect(pixels, 72, 900, 640, 4, BRASS);
      break;
    case 'evidence_tour':
      fillBand(pixels, INK);
      fillRect(pixels, 0, 0, 820, HEIGHT, SLATE);
      fillRect(pixels, 820, 0, 8, HEIGHT, BRASS);
      fillRect(pixels, 860, 72, 980, 936, PAPER);
      fillRect(pixels, 892, 108, 220, 44, CHIP);
      fillRect(pixels, 892, 188, 72, 8, SIGNAL);
      break;
    case 'highlighter':
      fillBand(pixels, PAPER);
      fillRect(pixels, 0, 0, WIDTH, 64, NAVY);
      fillRect(pixels, 96, 720, 1120, 18, BRASS);
      fillRect(pixels, 96, 720, 180, 18, SIGNAL);
      fillRect(pixels, 0, 1016, WIDTH, 64, NAVY);
      break;
    case 'number_roll':
      fillBand(pixels, NAVY);
      fillRect(pixels, 0, 0, WIDTH, 12, BRASS);
      fillRect(pixels, 72, 420, 980, 420, CHIP);
      fillRect(pixels, 72, 420, 18, 420, BRASS);
      fillRect(pixels, 72, 1020, 420, 8, SIGNAL);
      break;
    case 'source_bar':
      fillBand(pixels, INK);
      fillRect(pixels, 0, 0, WIDTH, 48, BRASS);
      fillRect(pixels, 0, 880, WIDTH, 200, CHIP);
      fillRect(pixels, 0, 880, WIDTH, 8, SIGNAL);
      fillRect(pixels, 72, 920, 48, 48, BRASS);
      break;
    case 'yield_shrink':
      fillBand(pixels, NAVY);
      fillRect(pixels, 160, 120, 1600, 840, SLATE);
      fillRect(pixels, 1280, 760, 420, 160, CHIP);
      fillRect(pixels, 1280, 760, 12, 160, BRASS);
      fillRect(pixels, 1280, 908, 420, 12, SIGNAL);
      break;
    case 'transition_wipe': {
      fillBand(pixels, NAVY);
      for (let y = 0; y < HEIGHT; y += 1) {
        const split = Math.round((y / HEIGHT) * WIDTH * 0.72 + 220);
        for (let x = split; x < WIDTH; x += 1) {
          const offset = (y * WIDTH + x) * 3;
          pixels[offset] = BRASS[0];
          pixels[offset + 1] = BRASS[1];
          pixels[offset + 2] = BRASS[2];
        }
        for (let x = split; x < split + 14 && x < WIDTH; x += 1) {
          const offset = (y * WIDTH + x) * 3;
          pixels[offset] = CREAM[0];
          pixels[offset + 1] = CREAM[1];
          pixels[offset + 2] = CREAM[2];
        }
      }
      break;
    }
    default:
      fillBand(pixels, INK);
      fillRect(pixels, 0, 792, WIDTH, 288, CHIP);
      fillRect(pixels, 0, 792, WIDTH, 6, BRASS);
      fillRect(pixels, 72, 820, 8, 96, SIGNAL);
      break;
  }
}

function writePpm(path, card) {
  const pixels = Buffer.alloc(WIDTH * HEIGHT * 3);
  paintCard(pixels, card);
  writeFileSync(path, Buffer.concat([Buffer.from(`P6\n${WIDTH} ${HEIGHT}\n255\n`), pixels]));
}

function beatDuration(beat, timing) {
  const chunks = (timing.chunks ?? []).filter((chunk) => chunk.beat_id === beat.id);
  if (chunks.length === 0) {
    return 3 + TAIL_GUARD_SECS;
  }
  const start = Math.min(...chunks.map((chunk) => Number(chunk.start_secs) || 0));
  const end = Math.max(...chunks.map((chunk) => Number(chunk.end_secs) || 0));
  return Math.max(1.5, end - start + TAIL_GUARD_SECS);
}

function posixPath(filePath) {
  return resolve(filePath).replaceAll('\\', '/');
}

function concatFileEntry(filePath) {
  const abs = posixPath(filePath).replaceAll("'", "'\\''");
  return `file '${abs}'`;
}

function ffmpegFontfile(filePath) {
  return posixPath(filePath).replaceAll(':', '\\:');
}

function findNarration(input) {
  const named = ['narration.wav', 'narration.mp3', 'audio.wav', 'audio.mp3', 'full.wav'];
  for (const name of named) {
    const path = join(input, name);
    if (existsSync(path)) return path;
  }
  const audioDir = join(input, 'audio');
  if (!existsSync(audioDir)) return null;
  const hit = readdirSync(audioDir).find((name) => /\.(wav|mp3|m4a)$/i.test(name));
  return hit ? join(audioDir, hit) : null;
}

function findAtmosphere(stillsDir, index) {
  const pad = String(index).padStart(3, '0');
  for (const ext of ['png', 'jpg', 'jpeg', 'webp']) {
    const path = join(stillsDir, `bg_${pad}.${ext}`);
    if (existsSync(path)) return path;
  }
  return null;
}

function extractNumeral(text) {
  const match = String(text || '').match(/[\d][\d,.\s]*%?/);
  return match ? match[0].replace(/\s/g, '') : '';
}

function domainOf(beat) {
  return beat.citations?.[0]?.domain || '';
}

function drawtextFilters(font, beat) {
  const file = ffmpegFontfile(font);
  const headline = escapeDrawtext(beat.on_screen || beat.spoken_text || beat.card);
  const domain = escapeDrawtext(domainOf(beat));
  const numeral = escapeDrawtext(extractNumeral(beat.on_screen || beat.spoken_text));
  const card = beat.card;
  const t = (text, size, color, x, y) =>
    `drawtext=fontfile='${file}':text='${text}':fontsize=${size}:fontcolor=${color}:x=${x}:y=${y}`;
  if (card === 'title_desk') {
    return [
      t('BRIEFING', 22, '0xC49A4E', 72, 52),
      t(headline, 56, '0x12242A', 72, 88),
      t(domain, 24, '0xE2D2B4', 72, 860),
    ].join(',');
  }
  if (card === 'evidence_tour') {
    return [
      t(domain || 'SOURCE', 22, '0xE2D2B4', 908, 116),
      t(headline, 40, '0x12242A', 908, 240),
    ].join(',');
  }
  if (card === 'highlighter') {
    return [
      t(headline, 44, '0x12242A', 96, 640),
      t(domain, 20, '0xE2D2B4', 96, 1040),
    ].join(',');
  }
  if (card === 'number_roll') {
    return [
      t(numeral || headline, 120, '0xE2D2B4', 108, 520),
      t(headline, 28, '0xC49A4E', 108, 360),
    ].join(',');
  }
  if (card === 'source_bar') {
    return [
      t(domain || 'SOURCES', 28, '0xC49A4E', 140, 932),
      t(headline, 36, '0xF8F1E2', 140, 980),
    ].join(',');
  }
  if (card === 'yield_shrink') {
    return [
      t(domain || 'NOTE', 20, '0xC49A4E', 1310, 792),
      t(headline, 24, '0xF8F1E2', 1310, 832),
    ].join(',');
  }
  if (card === 'transition_wipe') {
    return [t(headline, 36, '0xF8F1E2', 96, 480)].join(',');
  }
  return [
    t(headline, 36, '0xF8F1E2', 108, 860),
    t(domain, 20, '0xC49A4E', 108, 920),
  ].join(',');
}

function motionFilter(card, duration, textFilter) {
  const frames = Math.max(45, Math.round(Number(duration) * 30));
  const ken =
    card === 'title_desk' || card === 'evidence_tour'
      ? `scale=2048:1152,zoompan=z='min(1.04+0.0004*on,1.08)':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=${frames}:s=1920x1080:fps=30`
      : null;
  if (ken && textFilter) return `${ken},${textFilter}`;
  if (ken) return ken;
  return textFilter;
}

const input = argValue('--input');
if (!input) {
  console.error('usage: node cli.mjs --input <working_dir>');
  process.exit(2);
}

const beatsPath = join(input, 'beats.json');
if (!existsSync(beatsPath)) {
  console.error('missing beats.json');
  process.exit(1);
}

const payload = JSON.parse(readFileSync(beatsPath, 'utf8'));
const beats = payload.beats ?? [];
const timing = payload.timing ?? (existsSync(join(input, 'timing.json'))
  ? JSON.parse(readFileSync(join(input, 'timing.json'), 'utf8'))
  : { chunks: [] });
const errors = [];
for (const beat of beats) {
  if (!catalog.includes(beat.card)) {
    errors.push(`unknown card ${beat.card}`);
  }
}

const stillsDir = join(input, 'stills');
const clipsDir = join(input, 'clips');
mkdirSync(stillsDir, { recursive: true });
mkdirSync(clipsDir, { recursive: true });

if (errors.length) {
  writeFileSync(join(input, 'qa.json'), JSON.stringify({ ok: false, errors }, null, 2));
  console.error(errors.join('\n'));
  process.exit(1);
}

if (beats.length === 0) {
  console.error('no beats');
  process.exit(1);
}

const font = findFont();
const logs = [];
const concatLines = [];

for (let i = 0; i < beats.length; i += 1) {
  const beat = beats[i];
  const ppm = join(stillsDir, `${String(i).padStart(3, '0')}-${beat.card}.ppm`);
  const clip = join(clipsDir, `${String(i).padStart(3, '0')}.mp4`);
  writePpm(ppm, beat.card);
  const atmosphere = findAtmosphere(stillsDir, i);
  const still = atmosphere ?? ppm;
  const duration = beatDuration(beat, timing).toFixed(3);
  const textFilter = font ? drawtextFilters(font, beat) : null;
  const vf = motionFilter(beat.card, duration, textFilter);
  const baseArgs = [
    '-y',
    '-loop',
    '1',
    '-framerate',
    '30',
    '-i',
    still,
    '-t',
    duration,
    '-pix_fmt',
    'yuv420p',
  ];
  const attempts = [];
  if (vf) {
    attempts.push([...baseArgs, '-vf', vf, '-c:v', 'libx264', clip]);
    attempts.push([...baseArgs, '-vf', vf, clip]);
  }
  attempts.push([...baseArgs, '-c:v', 'libx264', clip]);
  attempts.push([...baseArgs, clip]);
  let encoded = false;
  for (const args of attempts) {
    const result = ffmpeg(args);
    if (ffmpegOk(result) && existsSync(clip)) {
      encoded = true;
      break;
    }
    logs.push(`encode retry ${beat.card}: ${(result.stderr || '').slice(-240)}`);
  }
  if (!encoded) {
    logs.push(`clip failed ${beat.card}`);
    writeFileSync(join(input, 'compose.log'), logs.join('\n'));
    process.exit(1);
  }
  concatLines.push(concatFileEntry(clip));
}

const listPath = join(clipsDir, 'concat.txt');
writeFileSync(listPath, `${concatLines.join('\n')}\n`);
const videoOnly = join(clipsDir, 'video-only.mp4');
const out = join(input, 'briefing.mp4');
const concat = ffmpeg([
  '-y',
  '-f',
  'concat',
  '-safe',
  '0',
  '-i',
  listPath,
  '-c:v',
  'libx264',
  '-pix_fmt',
  'yuv420p',
  '-movflags',
  '+faststart',
  videoOnly,
]);
if (!ffmpegOk(concat)) {
  logs.push(`concat failed: ${(concat.stderr || '').slice(-1200)}`);
  writeFileSync(join(input, 'compose.log'), logs.join('\n'));
  process.exit(1);
}

const narration = findNarration(input);
if (narration) {
  const mux = ffmpeg([
    '-y',
    '-i',
    videoOnly,
    '-i',
    narration,
    '-c:v',
    'copy',
    '-c:a',
    'aac',
    '-shortest',
    '-movflags',
    '+faststart',
    out,
  ]);
  if (!ffmpegOk(mux)) {
    logs.push(`audio mux skipped: ${(mux.stderr || '').slice(-400)}`);
      copyFileSync(videoOnly, out);
  }
} else {
  copyFileSync(videoOnly, out);
}

writeFileSync(
  join(input, 'qa.json'),
  JSON.stringify({ ok: true, errors: [], clips: beats.length, duration_source: 'timing.json' }, null, 2)
);
if (logs.length) {
  writeFileSync(join(input, 'compose.log'), logs.join('\n'));
}
if (!existsSync(out)) {
  process.exit(1);
}
console.log('wrote', out);
