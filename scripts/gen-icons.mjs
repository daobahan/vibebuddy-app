// Generate app icons procedurally — no art files, ever.
// Outputs: src-tauri/icons/{32x32,128x128,128x128@2x}.png + icon.ico (256px PNG entry)
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.join(here, '..', 'src-tauri', 'icons');
fs.mkdirSync(outDir, { recursive: true });

function hashString(s) {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}
function mulberry32(seed) {
  let a = seed >>> 0;
  return function () {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const PALETTES = [
  ['#f5c4b3', '#d85a30'], ['#cecbf6', '#7f77dd'], ['#9fe1cb', '#1d9e75'],
  ['#f4c0d1', '#d4537e'], ['#fac775', '#ba7517'], ['#b5d4f4', '#378add'],
];
function genSprite(seed) {
  const rnd = mulberry32(seed);
  const palette = PALETTES[Math.floor(rnd() * PALETTES.length)];
  const g = Array.from({ length: 8 }, () => [0, 0, 0, 0, 0, 0, 0, 0]);
  for (let y = 1; y < 7; y++) for (let x = 1; x < 4; x++) if (rnd() < 0.55) { g[y][x] = 1; g[y][7 - x] = 1; }
  g[3][3] = 1; g[3][4] = 1;
  for (let y = 1; y < 7; y++) for (let x = 1; x < 4; x++) if (g[y][x] === 1 && rnd() < 0.25) { g[y][x] = 2; g[y][7 - x] = 2; }
  for (let y = 2; y < 6; y++) if (g[y][2] && g[y][5]) { g[y][2] = 3; g[y][5] = 3; break; }
  return { grid: g, palette };
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
function png(width, height, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const raw = Buffer.alloc(height * (1 + width * 4));
  for (let y = 0; y < height; y++) {
    raw[y * (1 + width * 4)] = 0;
    rgba.copy(raw, y * (1 + width * 4) + 1, y * width * 4, (y + 1) * width * 4);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', zlib.deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

const hex = (h) => [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)];
const BG = hex('#141416');
const EYE = hex('#232326');
const { grid, palette } = genSprite(hashString('vibebuddy:pet:vibebuddy'));
const [light, dark] = [hex(palette[0]), hex(palette[1])];

function renderIcon(size) {
  const rgba = Buffer.alloc(size * size * 4);
  const cell = Math.floor(size / 10);
  const off = Math.floor((size - cell * 8) / 2);
  for (let y = 0; y < size; y++)
    for (let x = 0; x < size; x++) {
      const gx = Math.floor((x - off) / cell);
      const gy = Math.floor((y - off) / cell);
      let c = BG;
      if (gx >= 0 && gx < 8 && gy >= 0 && gy < 8) {
        const v = grid[gy][gx];
        if (v === 1) c = light;
        else if (v === 2) c = dark;
        else if (v === 3) c = EYE;
      }
      const i = (y * size + x) * 4;
      rgba[i] = c[0]; rgba[i + 1] = c[1]; rgba[i + 2] = c[2]; rgba[i + 3] = 255;
    }
  return png(size, size, rgba);
}

// ICO container with a single 256px PNG entry (Vista+)
function ico(png256) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(1, 4);
  const entry = Buffer.alloc(16);
  entry[0] = 0; // 256 width
  entry[1] = 0; // 256 height
  entry[4] = 1; // planes
  entry.writeUInt16LE(32, 6); // bpp
  entry.writeUInt32LE(png256.length, 8);
  entry.writeUInt32LE(22, 12);
  return Buffer.concat([header, entry, png256]);
}

fs.writeFileSync(path.join(outDir, '32x32.png'), renderIcon(32));
fs.writeFileSync(path.join(outDir, '128x128.png'), renderIcon(128));
fs.writeFileSync(path.join(outDir, '128x128@2x.png'), renderIcon(256));
fs.writeFileSync(path.join(outDir, 'icon.png'), renderIcon(512));
fs.writeFileSync(path.join(outDir, 'icon.ico'), ico(renderIcon(256)));
console.log('icons written to src-tauri/icons/');
