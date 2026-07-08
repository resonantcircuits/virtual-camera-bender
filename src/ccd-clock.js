import { clamp, hashUnit } from "./utils.js";
import { RAW_MAX, BLACK_LEVEL } from "./raw-domain.js";

// Charge-transfer clock bend (physics rail, spec Phase 7 item 2). Before
// there are bits or even voltages, the image is charge packets marched out of
// the sensor by multi-phase transfer clocks; this module corrupts that
// schedule, so it runs first on the rail. Everything operates on charge
// (raw minus pedestal, always >= 0), in sensor readout order:
//
// - Anti-blooming failure (`bloom`): the knob lowers the effective full-well
//   depth, so clipped highlights overflow and the excess charge spills up and
//   down the column until absorbed — vertical light spikes off bright areas.
// - Vertical transfer faults (`transferLoss`, `vSkip`): each row shift leaves
//   a fraction of the packet behind to mix with the row coming through
//   (recursive melt — bright sources wash down the frame), and skipped or
//   doubled V pulses stall or jump the readout pointer (row-repeat stretches,
//   row-skip compression). A stall of odd length flips the Bayer row phase
//   for everything after it, so the develop step color-swaps below the tear —
//   physically real, gloriously wrong.
// - H-register glitches (`hShear`): a horizontal clock glitch shifts a row
//   band's readout phase, shearing it sideways with wraparound. Odd offsets
//   flip the Bayer column phase: rainbow fringes on the sheared bands.
//
// Event durations and rates are normalized to a 960-row frame and the melt
// coefficient is compensated per row (gamma^rowRate), so streak lengths and
// band thickness hold across render resolutions (railSag precedent).

export function ccdClockActive(config) {
  if (!config?.enabled) return false;
  return (
    clamp(config.transferLoss ?? 0) > 0.004 ||
    clamp(config.vSkip ?? 0) > 0.004 ||
    clamp(config.hShear ?? 0) > 0.004 ||
    clamp(config.bloom ?? 0) > 0.004
  );
}

export function applyCcdClockRaw(raw, width, height, config, seed) {
  const transferLoss = clamp(config.transferLoss ?? 0);
  const vSkip = clamp(config.vSkip ?? 0);
  const hShear = clamp(config.hShear ?? 0);
  const bloom = clamp(config.bloom ?? 0);
  const rowRate = Math.min(2.5, 960 / height);

  if (bloom > 0.004) applyBloom(raw, width, height, bloom, rowRate);
  if (transferLoss > 0.004 || vSkip > 0.004) {
    applyVerticalTransfer(raw, width, height, transferLoss, vSkip, rowRate, seed);
  }
  if (hShear > 0.004) applyHShear(raw, width, height, hShear, rowRate, seed);
}

// Full-well overflow: charge beyond the (artificially lowered) well depth is
// clamped, and the excess splits half-down / half-up the column, absorbed by
// whatever headroom the wells on the way still have. Excess is scaled by
// 1/rowRate so spike length is a fraction of the frame, not a fixed pixel
// count.
function applyBloom(raw, width, height, bloom, rowRate) {
  const range = RAW_MAX - BLACK_LEVEL;
  const well = range * (1 - 0.88 * Math.pow(bloom, 1.3));
  // Wells along the way drain the passing spill at a limited rate (the
  // anti-blooming structure is what failed), so streaks decay exponentially
  // instead of dying at the first dark pixel; deeper bloom drains slower.
  const absorb = Math.min(1, (0.28 - 0.22 * bloom) * rowRate);
  const charge = new Float32Array(height);
  const spill = new Float32Array(height);
  for (let x = 0; x < width; x += 1) {
    for (let y = 0; y < height; y += 1) {
      let c = raw[y * width + x] - BLACK_LEVEL;
      if (c < 0) c = 0;
      const excess = c - well;
      if (excess > 0) {
        charge[y] = well;
        spill[y] = excess / rowRate;
      } else {
        charge[y] = c;
        spill[y] = 0;
      }
    }
    let carry = 0;
    for (let y = 0; y < height; y += 1) {
      carry += spill[y] * 0.6;
      if (carry > 0) {
        const room = well - charge[y];
        if (room > 0) {
          const drained = carry * absorb;
          const absorbed = room < drained ? room : drained;
          charge[y] += absorbed;
          carry -= absorbed;
        }
      }
    }
    carry = 0;
    for (let y = height - 1; y >= 0; y -= 1) {
      carry += spill[y] * 0.4;
      if (carry > 0) {
        const room = well - charge[y];
        if (room > 0) {
          const drained = carry * absorb;
          const absorbed = room < drained ? room : drained;
          charge[y] += absorbed;
          carry -= absorbed;
        }
      }
    }
    for (let y = 0; y < height; y += 1) {
      raw[y * width + x] = BLACK_LEVEL + charge[y] + 0.5;
    }
  }
}

// The vertical stage replays readout with a corruptible row pointer and a
// leaky shift register. gamma is the charge fraction left behind per shift;
// compensating by rowRate keeps the melt length a constant fraction of the
// frame. The pointer stalls (skipped V pulse: row repeats) or jumps (doubled
// pulse: row skipped) for drawn durations in normalized rows.
function applyVerticalTransfer(raw, width, height, transferLoss, vSkip, rowRate, seed) {
  const src = raw.slice();
  // Perceptual sweep in melt decay length: ~1 normalized row at the bottom of
  // the knob (soft bleed) to ~500 (paint drips washing down most of the frame).
  const decayRows = Math.pow(10, 0.08 + 2.62 * transferLoss);
  const gamma = transferLoss > 0.004 ? Math.exp(-rowRate / decayRows) : 0;
  const keep = 1 - gamma;
  const melt = new Float32Array(width);
  const pEvent = vSkip > 0 ? (0.0025 + 0.02 * vSkip) * rowRate : 0;

  let sy = -1;
  let stallLeft = 0;
  let jumpLeft = 0;
  for (let y = 0; y < height; y += 1) {
    if (stallLeft > 0) {
      stallLeft -= 1;
    } else if (jumpLeft > 0) {
      jumpLeft -= 1;
      sy += 2;
    } else {
      sy += 1;
      if (pEvent > 0 && hashUnit(y, 21, seed) < pEvent) {
        const rows = Math.max(1, Math.round((1 + hashUnit(y, 22, seed) * (3 + 26 * vSkip)) / rowRate));
        if (hashUnit(y, 23, seed) < 0.7) stallLeft = rows;
        else jumpLeft = Math.max(1, Math.round(rows * 0.5));
      }
    }
    // A jump past the last row reads an empty register: only the leftover
    // melt charge comes out, fading toward black.
    const srcRow = sy < height ? Math.max(0, sy) * width : -1;
    const outRow = y * width;
    for (let x = 0; x < width; x += 1) {
      let charge = 0;
      if (srcRow >= 0) {
        charge = src[srcRow + x] - BLACK_LEVEL;
        if (charge < 0) charge = 0;
      }
      const m = keep * charge + gamma * melt[x];
      melt[x] = m;
      raw[outRow + x] = BLACK_LEVEL + (m >= RAW_MAX - BLACK_LEVEL ? RAW_MAX - BLACK_LEVEL : m) + 0.5;
    }
  }
}

// H-register glitch: bands of rows read out with a shifted horizontal phase.
// Band thickness follows the documented bimodal slice statistics (narrow
// 2-4 px bands vs 8-22 px blocks, in normalized rows), offsets reach +-20%
// of the width at full strength, and rows wrap around.
function applyHShear(raw, width, height, hShear, rowRate, seed) {
  const rowBuf = new Uint16Array(width);
  const pEvent = (0.002 + 0.024 * hShear) * rowRate;
  let bandLeft = 0;
  let dx = 0;
  for (let y = 0; y < height; y += 1) {
    if (bandLeft > 0) {
      bandLeft -= 1;
    } else if (hashUnit(y, 31, seed) < pEvent) {
      const thin = hashUnit(y, 32, seed) < 0.6;
      const rows = thin ? 2 + hashUnit(y, 33, seed) * 2 : 8 + hashUnit(y, 33, seed) * 14;
      bandLeft = Math.max(1, Math.round(rows / rowRate)) - 1;
      const span = (0.03 + 0.17 * hShear) * width;
      dx = Math.round((hashUnit(y, 34, seed) * 2 - 1) * span);
    } else {
      continue;
    }
    if (dx === 0) continue;
    const row = y * width;
    for (let x = 0; x < width; x += 1) {
      let sx = (x - dx) % width;
      if (sx < 0) sx += width;
      rowBuf[x] = raw[row + sx];
    }
    raw.set(rowBuf, row);
  }
}
