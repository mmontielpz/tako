// Generates src-tauri/icons/icon.png from the idle pixel-art Tako. Run: node scripts/make-icon.ts
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { PALETTE, poseGrid } from "../src/pixelGrid.ts";

const SCALE = 8; // 16 px grid -> 128 px
const N = 16 * SCALE;
const raw = Buffer.alloc((N * 4 + 1) * N);
const grid = poseGrid("idle");
for (let y = 0; y < N; y++) {
  raw[y * (N * 4 + 1)] = 0; // filter: none
  for (let x = 0; x < N; x++) {
    const ch = grid[Math.floor(y / SCALE)]?.[Math.floor(x / SCALE)] ?? ".";
    const hex = PALETTE[ch];
    const o = y * (N * 4 + 1) + 1 + x * 4;
    if (hex) {
      raw[o] = parseInt(hex.slice(1, 3), 16); raw[o + 1] = parseInt(hex.slice(3, 5), 16);
      raw[o + 2] = parseInt(hex.slice(5, 7), 16); raw[o + 3] = 255;
    }
  }
}
const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
const crc = (b: Buffer) => { let c = 0xffffffff; for (const x of b) c = crcTable[(c ^ x) & 0xff]! ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
const chunk = (t: string, d: Buffer) => {
  const len = Buffer.alloc(4); len.writeUInt32BE(d.length);
  const td = Buffer.concat([Buffer.from(t), d]);
  const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
  return Buffer.concat([len, td, c]);
};
const ihdr = Buffer.alloc(13); ihdr.writeUInt32BE(N, 0); ihdr.writeUInt32BE(N, 4); ihdr[8] = 8; ihdr[9] = 6;
const png = Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
mkdirSync("../src-tauri/icons", { recursive: true });
writeFileSync("../src-tauri/icons/icon.png", png);
console.log("wrote ../src-tauri/icons/icon.png", png.length, "bytes");
