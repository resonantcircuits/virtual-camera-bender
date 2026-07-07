import { clamp } from "./utils.js";

// Shared raw-domain harness for the physics rail (spec Phase 7): the inverse
// and forward ISP that bracket every circuit-level bend module. The engine
// converts to 12-bit RGGB Bayer raw once, runs all enabled physics modules on
// the same buffer in true signal order (charge -> analog -> bus -> ...), and
// develops the result once — so corruptions compound physically and the image
// only pays the demosaic softening a single time.

export const RAW_BITS = 12;
export const RAW_MAX = (1 << RAW_BITS) - 1;
export const BLACK_LEVEL = 128;

// RGGB quad, indexed by ((y & 1) << 1) | (x & 1).
export const BAYER_CHANNEL = [0, 1, 1, 2];

const SRGB_TO_LINEAR = (() => {
  const lut = new Float32Array(256);
  for (let v = 0; v < 256; v += 1) {
    const c = v / 255;
    lut[v] = c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
  }
  return lut;
})();

const LINEAR_TO_SRGB = (() => {
  const lut = new Uint8ClampedArray(4096);
  for (let i = 0; i < 4096; i += 1) {
    const c = i / 4095;
    lut[i] = Math.round((c <= 0.0031308 ? c * 12.92 : 1.055 * Math.pow(c, 1 / 2.4) - 0.055) * 255);
  }
  return lut;
})();

// One camera has one white balance: the rail reads it off whichever physics
// module leads the chain (they all carry wbRed/wbBlue with the same defaults).
export function wbGains(config) {
  return [clamp(config?.wbRed ?? 2, 1, 4), 1, clamp(config?.wbBlue ?? 1.5, 1, 4)];
}

// Inverse ISP: sRGB -> linear -> un-white-balance -> 12-bit RGGB raw.
export function imageToRaw(image, wb) {
  const { width, height, data } = image;
  const raw = new Uint16Array(width * height);
  for (let y = 0; y < height; y += 1) {
    const row = y * width;
    for (let x = 0; x < width; x += 1) {
      const p = row + x;
      const ch = BAYER_CHANNEL[((y & 1) << 1) | (x & 1)];
      const lin = SRGB_TO_LINEAR[data[p * 4 + ch]] / wb[ch];
      raw[p] = (BLACK_LEVEL + lin * (RAW_MAX - BLACK_LEVEL) + 0.5) | 0;
    }
  }
  return raw;
}

// Forward ISP: black level, WB gains, bilinear RGGB demosaic, gamma.
export function rawToImage(image, raw, wb) {
  const { width, height, data } = image;
  const size = width * height;
  const plane = new Float32Array(size);
  const invRange = 1 / (RAW_MAX - BLACK_LEVEL);
  for (let y = 0; y < height; y += 1) {
    const row = y * width;
    for (let x = 0; x < width; x += 1) {
      const p = row + x;
      const ch = BAYER_CHANNEL[((y & 1) << 1) | (x & 1)];
      plane[p] = Math.max(0, (raw[p] - BLACK_LEVEL) * invRange) * wb[ch];
    }
  }

  const sample = (x, y) => {
    const cx = x < 0 ? 0 : x >= width ? width - 1 : x;
    const cy = y < 0 ? 0 : y >= height ? height - 1 : y;
    return plane[cy * width + cx];
  };
  const encode = (lin) => LINEAR_TO_SRGB[lin >= 1 ? 4095 : lin <= 0 ? 0 : (lin * 4095) | 0];

  for (let y = 0; y < height; y += 1) {
    const redRow = (y & 1) === 0;
    for (let x = 0; x < width; x += 1) {
      const redCol = (x & 1) === 0;
      let r;
      let g;
      let b;
      if (redRow && redCol) {
        r = sample(x, y);
        g = (sample(x - 1, y) + sample(x + 1, y) + sample(x, y - 1) + sample(x, y + 1)) / 4;
        b = (sample(x - 1, y - 1) + sample(x + 1, y - 1) + sample(x - 1, y + 1) + sample(x + 1, y + 1)) / 4;
      } else if (!redRow && !redCol) {
        b = sample(x, y);
        g = (sample(x - 1, y) + sample(x + 1, y) + sample(x, y - 1) + sample(x, y + 1)) / 4;
        r = (sample(x - 1, y - 1) + sample(x + 1, y - 1) + sample(x - 1, y + 1) + sample(x + 1, y + 1)) / 4;
      } else {
        g = sample(x, y);
        if (redRow) {
          r = (sample(x - 1, y) + sample(x + 1, y)) / 2;
          b = (sample(x, y - 1) + sample(x, y + 1)) / 2;
        } else {
          r = (sample(x, y - 1) + sample(x, y + 1)) / 2;
          b = (sample(x - 1, y) + sample(x + 1, y)) / 2;
        }
      }
      const index = (y * width + x) * 4;
      data[index] = encode(r);
      data[index + 1] = encode(g);
      data[index + 2] = encode(b);
    }
  }
}
