import { clamp, lerp } from "./utils.js";
import { RAW_MAX, BLACK_LEVEL } from "./raw-domain.js";

// IR-cut filter removal (physics rail, spec Phase 8 item 4): the DSC-V1
// magnetic filter bypass. Not corruption but a spectral mod, and the rail's
// first module — it changes what light reaches the sensor before charge,
// voltage, or bits exist.
//
// The scene's near-infrared is unknowable from an RGB photo, so it is
// synthesized the way full-spectrum conversions actually behave: red-weighted
// luminance (blue skies carry little IR and stay dark), plus the Wood effect
// (chlorophyll reflects NIR hard, so green-dominant foliage glows), built at
// half resolution from each Bayer quad and blurred for sensor halation (IR
// focuses behind the sensor plane and spills past edges). The plane then
// leaks into every well — silicon sees IR through all three CFA dyes, red
// most — and the AE pulls exposure back down, so the image shifts spectrum
// instead of blowing out. White balance then fights the leak in the shared
// rail develop: the classic pink-white foliage and dark liquid skies.
//
// `strength`: how far the filter is pulled (0 = seated, 1 = removed).
// `spectrum`: CFA dye transparency to IR — low = deep NIR (all dyes pass it,
// ghostly white), high = red-heavy leak (magenta/pink full-spectrum look).
// `wood`: foliage NIR reflectance boost. `haze`: halation blur reach.

export function irCutActive(config) {
  if (!config?.enabled) return false;
  return clamp(config.strength ?? 0) > 0.004;
}

// Absolute NIR brightness of fully green foliage, in charge units.
const FOLIAGE_IR = (RAW_MAX - BLACK_LEVEL) * 0.5;

export function applyIrCutRaw(raw, width, height, config) {
  const strength = clamp(config.strength ?? 0);
  const spectrum = clamp(config.spectrum ?? 0.6);
  const wood = clamp(config.wood ?? 0.5);
  const haze = clamp(config.haze ?? 0.35);

  const qw = Math.max(1, width >> 1);
  const qh = Math.max(1, height >> 1);
  const plane = new Float32Array(qw * qh);

  // The raw buffer is un-white-balanced (the inverse ISP divided the gains
  // out), so multiply them back for scene analysis — irCut leads the rail
  // whenever it is enabled, so its own WB gains are exactly the ones the
  // inverse ISP used.
  const wbR = clamp(config.wbRed ?? 2, 1, 4);
  const wbB = clamp(config.wbBlue ?? 1.5, 1, 4);

  // Synthesize the IR plane from each RGGB quad, in linear scene light.
  for (let qy = 0; qy < qh; qy += 1) {
    const y0 = qy * 2;
    const row0 = y0 * width;
    const row1 = Math.min(y0 + 1, height - 1) * width;
    for (let qx = 0; qx < qw; qx += 1) {
      const x0 = qx * 2;
      const x1 = Math.min(x0 + 1, width - 1);
      const r = Math.max(0, raw[row0 + x0] - BLACK_LEVEL) * wbR;
      const g = Math.max(0, raw[row0 + x1] - BLACK_LEVEL) * 0.5 + Math.max(0, raw[row1 + x0] - BLACK_LEVEL) * 0.5;
      const b = Math.max(0, raw[row1 + x1] - BLACK_LEVEL) * wbB;
      // Blue sky carries almost no infrared (Rayleigh scatter), so blue
      // barely contributes to the plane.
      let ir = 0.5 * r + 0.32 * g + 0.05 * b;
      // Wood effect: chlorophyll reflects ~half of all NIR regardless of how
      // dark it looks in visible light, so foliage IR follows *greenness
      // ratio* at near-absolute brightness, not the pixel's luminance.
      const gExcess = g - 0.55 * (r + b);
      if (gExcess > 0) {
        const greenness = Math.min(1, (gExcess * 4) / (r + g + b + 1));
        ir += greenness * greenness * (0.2 + 0.8 * wood) * FOLIAGE_IR;
      }
      plane[qy * qw + qx] = ir;
    }
  }

  // Halation: separable box blur passes over the half-res plane, radius a
  // fraction of the frame so the glow reach matches across resolutions.
  const radius = Math.round(Math.pow(haze, 1.5) * qw * 0.05);
  if (radius > 0) {
    boxBlurPlane(plane, qw, qh, radius);
    boxBlurPlane(plane, qw, qh, Math.max(1, radius >> 1));
  }

  // Leak the plane into every well and let AE pull exposure back down. Low
  // `spectrum` is the deep-NIR conversion: the visible pass-through fades
  // (dark liquid skies, ghost-white foliage); high `spectrum` keeps visible
  // light and leaks mostly into red (the pink full-spectrum cast).
  const leak = [1, lerp(0.9, 0.35, spectrum), lerp(0.85, 0.25, spectrum)];
  const leakEven = [leak[0], leak[1]];
  const leakOdd = [leak[1], leak[2]];
  const visKeep = lerp(1, lerp(0.1, 1, Math.pow(spectrum, 0.8)), strength);
  const exposure = 1 / (visKeep + 0.7 * strength);
  for (let y = 0; y < height; y += 1) {
    const fy = Math.min(qh - 1.001, Math.max(0, y * 0.5 - 0.25));
    const qy = fy | 0;
    const ty = fy - qy;
    const rowA = qy * qw;
    const rowB = Math.min(qy + 1, qh - 1) * qw;
    const row = y * width;
    const rowLeak = (y & 1) === 0 ? leakEven : leakOdd;
    for (let x = 0; x < width; x += 1) {
      const fx = Math.min(qw - 1.001, Math.max(0, x * 0.5 - 0.25));
      const qx = fx | 0;
      const tx = fx - qx;
      const qx1 = Math.min(qx + 1, qw - 1);
      const ir =
        (plane[rowA + qx] * (1 - tx) + plane[rowA + qx1] * tx) * (1 - ty) +
        (plane[rowB + qx] * (1 - tx) + plane[rowB + qx1] * tx) * ty;
      const p = row + x;
      const charge = Math.max(0, raw[p] - BLACK_LEVEL);
      const out = (charge * visKeep + ir * rowLeak[x & 1] * strength) * exposure;
      raw[p] = BLACK_LEVEL + (out >= RAW_MAX - BLACK_LEVEL ? RAW_MAX - BLACK_LEVEL : out) + 0.5;
    }
  }
}

function boxBlurPlane(plane, w, h, radius) {
  const tmp = new Float32Array(Math.max(w, h));
  const norm = 1 / (radius * 2 + 1);
  for (let y = 0; y < h; y += 1) {
    const row = y * w;
    let acc = 0;
    for (let i = -radius; i <= radius; i += 1) acc += plane[row + clampIndex(i, w)];
    for (let x = 0; x < w; x += 1) {
      tmp[x] = acc * norm;
      acc += plane[row + clampIndex(x + radius + 1, w)] - plane[row + clampIndex(x - radius, w)];
    }
    plane.set(tmp.subarray(0, w), row);
  }
  for (let x = 0; x < w; x += 1) {
    let acc = 0;
    for (let i = -radius; i <= radius; i += 1) acc += plane[clampIndex(i, h) * w + x];
    for (let y = 0; y < h; y += 1) {
      tmp[y] = acc * norm;
      acc += plane[clampIndex(y + radius + 1, h) * w + x] - plane[clampIndex(y - radius, h) * w + x];
    }
    for (let y = 0; y < h; y += 1) plane[y * w + x] = tmp[y];
  }
}

function clampIndex(i, n) {
  return i < 0 ? 0 : i >= n ? n - 1 : i;
}
