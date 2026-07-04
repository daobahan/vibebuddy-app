// Generate app icons procedurally — no art files, ever.
// Outputs: src-tauri/icons/{32x32,128x128,128x128@2x}.png + icon.ico (256px PNG entry)
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.join(here, '..', 'src-tauri', 'icons');
fs.mkdirSync(outDir, { recursive: true });

// ---- 16x16 brand mark v2: a speech bubble holding a heart (conversation with love in it) ----
function markGrid() {
  const g = Array.from({ length: 16 }, () => Array(16).fill(null));
  const set = (x, y, c) => { if (x >= 0 && x < 16 && y >= 0 && y < 16) g[y][x] = c; };
  const body = '#f2ede2', shade = '#d9d2c2', line = '#26262b', heart = '#d85a30', spark = '#f5c542';
  const ROWS = { 2: [4, 11], 3: [3, 12], 4: [2, 13], 5: [2, 13], 6: [2, 13], 7: [2, 13], 8: [2, 13], 9: [3, 12], 10: [4, 11] };
  for (const [yStr, [a, b]] of Object.entries(ROWS)) {
    const y = Number(yStr);
    for (let x = a; x <= b; x++) set(x, y, x >= b - 1 || y >= 9 ? shade : body);
    set(a - 1, y, line); set(b + 1, y, line);
  }
  for (let x = 4; x <= 11; x++) set(x, 1, line);
  for (let x = 4; x <= 11; x++) set(x, 11, x === 5 || x === 6 ? shade : line);
  set(5, 12, line); set(6, 12, shade); set(7, 12, line);
  set(4, 13, line); set(5, 13, shade); set(6, 13, line);
  set(4, 14, line); set(5, 14, line);
  set(5, 4, heart); set(6, 4, heart); set(9, 4, heart); set(10, 4, heart);
  for (let x = 5; x <= 10; x++) set(x, 5, heart);
  for (let x = 5; x <= 10; x++) set(x, 6, heart);
  for (let x = 6; x <= 9; x++) set(x, 7, heart);
  set(7, 8, heart); set(8, 8, heart);
  set(13, 1, spark); set(12, 2, spark); set(14, 2, spark); set(13, 3, spark);
  return g;
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
const grid = markGrid();

function renderIcon(size) {
  const rgba = Buffer.alloc(size * size * 4);
  const cell = Math.floor(size / 16);
  const off = Math.floor((size - cell * 16) / 2);
  for (let y = 0; y < size; y++)
    for (let x = 0; x < size; x++) {
      const gx = Math.floor((x - off) / cell);
      const gy = Math.floor((y - off) / cell);
      let c = BG;
      if (gx >= 0 && gx < 16 && gy >= 0 && gy < 16 && grid[gy][gx]) c = hex(grid[gy][gx]);
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
