#!/usr/bin/env node
/**
 * P0 synthetic-image MCP. Oracle is written beside the process, never in tool text.
 */
import { createHash, randomInt } from "node:crypto";
import { createWriteStream, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";
import readline from "node:readline";

const ROOT = dirname(fileURLToPath(import.meta.url));
const OUT_DIR = process.env.GROK_CU_ORACLE_DIR || join(ROOT, ".run");
mkdirSync(OUT_DIR, { recursive: true });

const DIGIT_FONT = [
  [0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
  [0b00100, 0b01100, 0b00100, 0b00100, 0b01110],
  [0b01110, 0b10001, 0b00010, 0b00100, 0b11111],
  [0b11110, 0b00001, 0b01110, 0b00001, 0b11110],
  [0b10001, 0b10001, 0b11111, 0b00001, 0b00001],
  [0b11111, 0b10000, 0b11110, 0b00001, 0b11110],
  [0b01110, 0b10000, 0b11110, 0b10001, 0b01110],
  [0b11111, 0b00001, 0b00010, 0b00100, 0b00100],
  [0b01110, 0b10001, 0b01110, 0b10001, 0b01110],
  [0b01110, 0b10001, 0b01111, 0b00001, 0b01110],
];

function crc32(buf) {
  let c = ~0;
  for (const b of buf) {
    c ^= b;
    for (let i = 0; i < 8; i++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type, data) {
  const t = Buffer.from(type);
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([t, data])));
  return Buffer.concat([len, t, data, crc]);
}

function encodePng(width, height, rgb) {
  const raw = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (width * 3 + 1)] = 0;
    rgb.copy(raw, y * (width * 3 + 1) + 1, y * width * 3, (y + 1) * width * 3);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8;
  ihdr[9] = 2;
  const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  return Buffer.concat([
    sig,
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function render(seed) {
  const width = 400;
  const height = 300;
  const rng = mulberry32(seed);
  const code = String(Math.floor(rng() * 10000)).padStart(4, "0");
  const boxW = 120;
  const boxH = 80;
  const boxX = 20 + Math.floor(rng() * (width - boxW - 40));
  const boxY = 20 + Math.floor(rng() * (height - boxH - 40));
  const rgb = Buffer.alloc(width * height * 3, 28);
  const put = (x, y, r, g, b) => {
    if (x < 0 || y < 0 || x >= width || y >= height) return;
    const i = (y * width + x) * 3;
    rgb[i] = r;
    rgb[i + 1] = g;
    rgb[i + 2] = b;
  };
  for (let x = 0; x < width; x += 40) for (let y = 0; y < height; y++) put(x, y, 48, 54, 64);
  for (let y = 0; y < height; y += 40) for (let x = 0; x < width; x++) put(x, y, 48, 54, 64);
  for (let y = boxY; y < boxY + boxH; y++) {
    for (let x = boxX; x < boxX + boxW; x++) put(x, y, 180, 32, 32);
  }
  const scale = 6;
  const digitW = 5 * scale + 4;
  for (let di = 0; di < 4; di++) {
    const d = Number(code[di]);
    const glyph = DIGIT_FONT[d];
    for (let row = 0; row < 5; row++) {
      for (let col = 0; col < 5; col++) {
        if (glyph[row] & (1 << (4 - col))) {
          for (let dy = 0; dy < scale; dy++) {
            for (let dx = 0; dx < scale; dx++) {
              put(boxX + 8 + di * digitW + col * scale + dx, boxY + 16 + row * scale + dy, 255, 255, 255);
            }
          }
        }
      }
    }
  }
  put(0, 0, 0, 255, 0);
  put(width - 1, height - 1, 255, 0, 255);
  const png = encodePng(width, height, rgb);
  const oracle = {
    seed,
    code,
    targetCx: boxX + Math.floor(boxW / 2),
    targetCy: boxY + Math.floor(boxH / 2),
    boxX,
    boxY,
    boxW,
    boxH,
    width,
    height,
    sha256: createHash("sha256").update(png).digest("hex"),
  };
  return { png, oracle };
}

function mulberry32(a) {
  return function () {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const seed = Number(process.env.GROK_CU_SEED || randomInt(1, 1e9));
const { png, oracle } = render(seed);
const oraclePath = join(OUT_DIR, "oracle.json");
writeFileSync(oraclePath, JSON.stringify(oracle, null, 2));
writeFileSync(join(OUT_DIR, "synthetic.png"), png);
const contentId = `syn-${oracle.sha256.slice(0, 12)}`;

function send(msg) {
  process.stdout.write(`${JSON.stringify(msg)}\n`);
}

function handle(msg) {
  if (msg.method === "initialize") {
    send({
      jsonrpc: "2.0",
      id: msg.id,
      result: {
        protocolVersion: "2024-11-05",
        capabilities: { tools: {} },
        serverInfo: { name: "grok-cu-synthetic", version: "0.1.0" },
      },
    });
    return;
  }
  if (msg.method === "notifications/initialized") return;
  if (msg.method === "tools/list") {
    send({
      jsonrpc: "2.0",
      id: msg.id,
      result: {
        tools: [
          {
            name: "computer_observe",
            description: "Observe the synthetic fixture. Returns an image; coordinates are image pixels.",
            inputSchema: { type: "object", properties: {}, additionalProperties: false },
          },
        ],
      },
    });
    return;
  }
  if (msg.method === "tools/call" && msg.params?.name === "computer_observe") {
    const text = JSON.stringify({
      snapshotId: contentId,
      coordinateSpace: "image_pixels",
      image: { width: oracle.width, height: oracle.height, contentId },
      truncated: false,
    });
    if (text.includes(oracle.code) || text.toLowerCase().includes("oracle")) {
      send({ jsonrpc: "2.0", id: msg.id, error: { code: -32000, message: "oracle leak" } });
      return;
    }
    send({
      jsonrpc: "2.0",
      id: msg.id,
      result: {
        content: [
          { type: "text", text },
          {
            type: "image",
            data: png.toString("base64"),
            mimeType: "image/png",
          },
        ],
        isError: false,
      },
    });
    return;
  }
  if (msg.id != null) {
    send({ jsonrpc: "2.0", id: msg.id, error: { code: -32601, message: "method not found" } });
  }
}

let buf = Buffer.alloc(0);
process.stdin.on("data", (chunk) => {
  buf = Buffer.concat([buf, chunk]);
  while (true) {
    const headerEnd = buf.indexOf("\r\n\r\n");
    if (headerEnd < 0) break;
    const header = buf.slice(0, headerEnd).toString("utf8");
    const m = /Content-Length:\s*(\d+)/i.exec(header);
    if (!m) {
      buf = buf.slice(headerEnd + 4);
      continue;
    }
    const len = Number(m[1]);
    const start = headerEnd + 4;
    if (buf.length < start + len) break;
    const body = buf.slice(start, start + len).toString("utf8");
    buf = buf.slice(start + len);
    try {
      handle(JSON.parse(body));
    } catch (e) {
      process.stderr.write(String(e));
    }
  }
});
