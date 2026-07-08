import { clamp, hashSigned, hashUnit } from "./utils.js";

// SDRAM address-line bend (physics rail, spec Phase 7 item 3): the frame
// buffer holds the stream the DSP already captured, so this runs last on the
// rail. Where memoryFault paints random block damage, a stuck or bridged
// address line aliases memory in exact powers of two — mirrored tiles,
// interleaved row pairs, ping-ponged halves — crystalline deterministic
// repetition. The model corrupts the read address: each axis gets a small
// seeded set of line faults (stuck low, stuck high, bridged to a neighbour
// line, two lines swapped) folded into a lookup map.
//
// `rows`/`cols`: how many row / column address lines are damaged (each axis
// draws 1-3 faults). `scale`: which lines are hit — low bits give fine
// interleaves, high bits fold whole halves of the frame; positions are
// normalized to log2 of the frame dimension so tile size tracks resolution.
// `lowBit`: chance a fault lands on address line 0 — the only fault that
// breaks CFA parity, so aliased regions develop color-swapped (the pixel-
// grind dose, kept separate like masterClock's shred). `duty`: fraction of
// the readout the marginal contact is actually failing — 1 is a solid short
// (whole-frame kaleidoscope), lower values gate the fault on and off in
// smoothed bands of rows, the documented horizontal-tearing look of timing
// faults on address lines. Gate band scale is normalized to a 960-row frame.

export function addressBusActive(config) {
  if (!config?.enabled) return false;
  return clamp(config.rows ?? 0) > 0.004 || clamp(config.cols ?? 0) > 0.004;
}

export function applyAddressBusRaw(raw, width, height, config, seed) {
  const rows = clamp(config.rows ?? 0);
  const cols = clamp(config.cols ?? 0);
  const scale = clamp(config.scale ?? 0.55);
  const lowBit = clamp(config.lowBit ?? 0);
  const duty = clamp(config.duty ?? 0.85);

  const ymap = rows > 0.004 ? buildAddressMap(height, rows, scale, lowBit, 71, seed) : null;
  const xmap = cols > 0.004 ? buildAddressMap(width, cols, scale, lowBit, 76, seed) : null;
  if (!ymap && !xmap) return;

  const src = raw.slice();
  const rowRate = Math.min(2.5, 960 / height);
  // Low-passed gate noise (see railSag): the AR(1) drive is normalized to
  // unit variance so `duty` thresholds it consistently at any smoothing.
  const smoothK = Math.min(0.5, 0.06 * rowRate);
  const norm = 1 / Math.sqrt(smoothK / (3 * (2 - smoothK)));
  const solid = duty >= 0.995;
  let driveY = 0;
  let driveX = 0;
  for (let y = 0; y < height; y += 1) {
    driveY += smoothK * (hashSigned(y, 81, seed) - driveY);
    driveX += smoothK * (hashSigned(y, 86, seed) - driveX);
    const yOn = ymap !== null && (solid || 0.5 + 0.5 * Math.tanh(0.8 * driveY * norm) < duty);
    const xOn = xmap !== null && (solid || 0.5 + 0.5 * Math.tanh(0.8 * driveX * norm) < duty);
    if (!yOn && !xOn) continue;
    const sy = yOn ? ymap[y] : y;
    const srcRow = sy * width;
    const dstRow = y * width;
    if (xOn) {
      for (let x = 0; x < width; x += 1) raw[dstRow + x] = src[srcRow + xmap[x]];
    } else if (sy !== y) {
      raw.set(src.subarray(srcRow, srcRow + width), dstRow);
    }
  }
}

function buildAddressMap(dim, amount, scale, lowBit, salt, seed) {
  const topBit = Math.max(2, 31 - Math.clz32(dim));
  const count = 1 + Math.round(amount * 2.2);
  const faults = [];
  for (let i = 0; i < count; i += 1) {
    const isLow = hashUnit(i, salt + 3, seed) < lowBit * 0.85;
    const band = clamp(scale + (hashUnit(i, salt + 1, seed) - 0.5) * 0.5);
    const k = isLow ? 0 : 1 + Math.round(band * (topBit - 2));
    const step = 1 + ((hashUnit(i, salt + 4, seed) * 2) | 0);
    let j = hashUnit(i, salt + 5, seed) < 0.5 ? k - step : k + step;
    if (j < 0) j = k + step;
    else if (j > topBit) j = Math.max(0, k - step);
    faults.push({ k, j, roll: hashUnit(i, salt + 2, seed) });
  }
  const map = new Uint32Array(dim);
  for (let a = 0; a < dim; a += 1) {
    let m = a;
    for (let i = 0; i < faults.length; i += 1) {
      const f = faults[i];
      const bit = 1 << f.k;
      if (f.roll < 0.35) {
        m &= ~bit; // line stuck low: paired blocks read the same content
      } else if (f.roll < 0.5) {
        m |= bit; // line stuck high (bridged to the supply)
      } else if (f.roll < 0.75) {
        m = (m & ~bit) | (((m >> f.j) & 1) << f.k); // bridged to a neighbour line
      } else if (((m >> f.k) & 1) !== ((m >> f.j) & 1)) {
        m ^= bit | (1 << f.j); // two lines swapped: block-permuted interleave
      }
    }
    // Addresses pushed past the buffer wrap around — reading past the frame.
    map[a] = m < dim ? m : m % dim;
  }
  return map;
}
