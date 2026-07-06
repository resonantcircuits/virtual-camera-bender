import { clamp, fbmNoise, fract, hashSigned, hashUnit, lerp, smoothstep } from "./utils.js";

const PALETTES = {
  "solarized-ccd": [
    [0, 0, 18],
    [0, 42, 212],
    [0, 214, 238],
    [246, 30, 214],
    [255, 238, 72],
    [18, 224, 72],
    [255, 255, 255]
  ],
  "thermal-bleach": [
    [0, 0, 0],
    [0, 34, 120],
    [0, 220, 255],
    [255, 110, 232],
    [255, 248, 190],
    [255, 255, 255]
  ],
  "pink-blue": [
    [0, 0, 14],
    [16, 24, 190],
    [60, 192, 255],
    [224, 38, 212],
    [255, 132, 224],
    [255, 255, 246]
  ],
  "toxic-green": [
    [0, 0, 0],
    [0, 84, 32],
    [26, 236, 86],
    [232, 18, 226],
    [255, 230, 44],
    [238, 255, 255]
  ],
  "rainbow": [
    [110, 0, 190],
    [22, 24, 255],
    [0, 190, 255],
    [0, 226, 80],
    [250, 240, 0],
    [255, 128, 0],
    [255, 22, 60],
    [255, 0, 210]
  ],
  "acid-sunset": [
    [46, 0, 66],
    [178, 0, 96],
    [255, 34, 34],
    [255, 122, 0],
    [255, 64, 158],
    [255, 158, 224],
    [255, 246, 236]
  ],
  "infrared": [
    [8, 6, 58],
    [66, 22, 158],
    [188, 42, 202],
    [255, 108, 228],
    [255, 190, 244],
    [255, 255, 255]
  ],
  "candy-shop": [
    [0, 146, 168],
    [118, 228, 214],
    [236, 246, 236],
    [255, 172, 222],
    [255, 92, 192],
    [198, 38, 142]
  ],
  "poison-dart": [
    [4, 10, 4],
    [18, 62, 30],
    [92, 142, 40],
    [192, 222, 60],
    [242, 255, 122],
    [250, 255, 232]
  ]
};

const CHANNEL_ORDERS = {
  none: [0, 1, 2],
  gbr: [1, 2, 0],
  brg: [2, 0, 1],
  grb: [1, 0, 2],
  bgr: [2, 1, 0],
  rbg: [0, 2, 1]
};

const EDGE_COLORS = {
  cyan: [0, 236, 255],
  magenta: [255, 0, 220],
  green: [0, 255, 80],
  black: [0, 0, 0],
  white: [255, 255, 255],
  red: [255, 22, 22],
  yellow: [255, 242, 42]
};

export function processCircuitBendImageData(image, preset) {
  const seed = Number.isFinite(preset.seed) ? preset.seed : 1;
  const pipeline = preset.pipeline;
  applyCheapScale(image, pipeline.cheapCamera);
  applySoftFocus(image, pipeline.cheapCamera);
  applySyncFault(image, pipeline.syncFault, seed);
  applyBayerFault(image, pipeline.bayerFault);
  applyChromaShift(image, pipeline.chromaShift, seed);
  applyExposureFault(image, pipeline.exposureFault, seed);
  applyColorBend(image, pipeline.colorBend);
  applyContourRings(image, pipeline.contourRings, seed);
  applyFalseColor(image, pipeline.falseColor, seed);
  applyGradientWash(image, pipeline.gradientWash, seed);
  applyEdgeBurn(image, pipeline.edgeBurn, seed);
  applyPixelSort(image, pipeline.pixelSort, seed);
  applyVerticalSmear(image, pipeline.verticalSmear, seed);
  applySensorNoise(image, pipeline.sensorNoise, seed);
  applyMemoryFault(image, pipeline.memoryFault, seed);
  applyDctCrunch(image, pipeline.dctCrunch, seed);
  applyFinalCrunch(image, pipeline);
  applyOsdOverlay(image, pipeline.osdOverlay, seed);
  return image;
}

function applyCheapScale(image, config) {
  if (!config?.enabled || config.internalScale >= 0.995) return;
  const { width, height, data } = image;
  const scale = clamp(config.internalScale, 0.2, 1);
  const source = new Uint8ClampedArray(data);
  const lowWidth = Math.max(1, Math.round(width * scale));
  const lowHeight = Math.max(1, Math.round(height * scale));

  for (let y = 0; y < height; y += 1) {
    const lowY = Math.min(lowHeight - 1, Math.floor(y * scale));
    const sy = Math.min(height - 1, Math.floor((lowY + 0.5) / scale));
    for (let x = 0; x < width; x += 1) {
      const lowX = Math.min(lowWidth - 1, Math.floor(x * scale));
      const sx = Math.min(width - 1, Math.floor((lowX + 0.5) / scale));
      const to = pixelIndex(x, y, width);
      const from = pixelIndex(sx, sy, width);
      data[to] = source[from];
      data[to + 1] = source[from + 1];
      data[to + 2] = source[from + 2];
      data[to + 3] = source[from + 3];
    }
  }
}

function applySoftFocus(image, config) {
  if (!config?.enabled || !(config.blur > 0.01)) return;
  const { width, height, data } = image;
  const radius = Math.max(
    1,
    Math.round(config.blur * 5 * Math.max(1, Math.min(width, height) / 900))
  );
  boxBlurPass(data, width, height, radius, true);
  boxBlurPass(data, width, height, radius, false);
}

function boxBlurPass(data, width, height, radius, horizontal) {
  const lineLength = horizontal ? width : height;
  const lineCount = horizontal ? height : width;
  const stride = horizontal ? 4 : width * 4;
  const lineStride = horizontal ? width * 4 : 4;
  const window = radius * 2 + 1;
  const buffer = new Float32Array(lineLength * 3);

  for (let line = 0; line < lineCount; line += 1) {
    const base = line * lineStride;
    let sumR = 0;
    let sumG = 0;
    let sumB = 0;
    for (let i = -radius; i <= radius; i += 1) {
      const index = base + clamp(i, 0, lineLength - 1) * stride;
      sumR += data[index];
      sumG += data[index + 1];
      sumB += data[index + 2];
    }
    for (let i = 0; i < lineLength; i += 1) {
      buffer[i * 3] = sumR / window;
      buffer[i * 3 + 1] = sumG / window;
      buffer[i * 3 + 2] = sumB / window;
      const addIndex = base + clamp(i + radius + 1, 0, lineLength - 1) * stride;
      const dropIndex = base + clamp(i - radius, 0, lineLength - 1) * stride;
      sumR += data[addIndex] - data[dropIndex];
      sumG += data[addIndex + 1] - data[dropIndex + 1];
      sumB += data[addIndex + 2] - data[dropIndex + 2];
    }
    for (let i = 0; i < lineLength; i += 1) {
      const index = base + i * stride;
      data[index] = buffer[i * 3];
      data[index + 1] = buffer[i * 3 + 1];
      data[index + 2] = buffer[i * 3 + 2];
    }
  }
}

function applySyncFault(image, config, seed) {
  if (!config?.enabled) return;
  const tearCount = clamp(config.tearCount ?? 0);
  const tearShift = clamp(config.tearShift ?? 0.4);
  const wobbleAmount = clamp(config.wobbleAmount ?? 0);
  const wobbleFrequency = clamp(config.wobbleFrequency ?? 0.4);
  const drift = clamp(config.drift ?? 0.3);
  const tears = Math.round(tearCount * 3);
  if (tears === 0 && wobbleAmount <= 0.005) return;

  const { width, height, data } = image;
  const source = new Uint8ClampedArray(data);
  const offsets = new Float32Array(height);
  const bandRows = new Set();

  // Frame wrap: below each tear row the whole frame shifts sideways and wraps,
  // with a short band of corrupted rows at the transition.
  for (let tear = 0; tear < tears; tear += 1) {
    const row =
      Math.floor(height * 0.05) +
      Math.floor(hashUnit(tear, 1, seed + 4001) * height * 0.9);
    const shift = hashSigned(tear, 2, seed + 4003) * tearShift * width * 0.5;
    const band = 2 + Math.floor(hashUnit(tear, 3, seed + 4005) * 5);
    for (let y = row; y < height; y += 1) offsets[y] += shift;
    for (let b = 0; b < band && row + b < height; b += 1) bandRows.add(row + b);
  }

  // Rolling wobble: per-row sine with progressive phase (jello) plus
  // low-frequency phase drift so verticals wander instead of ringing evenly.
  if (wobbleAmount > 0.005) {
    const amp = wobbleAmount * width * 0.045;
    const cycles = 1 + wobbleFrequency * 7;
    const freq = (cycles * Math.PI * 2) / height;
    for (let y = 0; y < height; y += 1) {
      const slow = (fbmNoise(0.3, y / (height * 0.24), seed + 4011) - 0.5) * drift * Math.PI * 3;
      const envelope = 0.55 + 0.45 * Math.sin(y * freq * 0.37 + slow);
      offsets[y] += Math.sin(y * freq + slow) * amp * envelope;
    }
  }

  for (let y = 0; y < height; y += 1) {
    const offset = Math.round(offsets[y]);
    const corrupt = bandRows.has(y);
    if (offset === 0 && !corrupt) continue;
    for (let x = 0; x < width; x += 1) {
      const sx = (((x - offset) % width) + width) % width;
      const to = pixelIndex(x, y, width);
      if (corrupt) {
        // Torn transition band: stuttered cells, split chroma, sparse specks.
        const jitter = Math.floor(hashUnit(x >> 3, y, seed + 4021) * width * 0.18);
        const rx = (sx + jitter) % width;
        const gx = (sx + jitter * 2) % width;
        data[to] = source[pixelIndex(rx, y, width)];
        data[to + 1] = source[pixelIndex(gx, y, width) + 1];
        data[to + 2] = source[pixelIndex(rx, y, width) + 2];
        if (hashUnit(x, y, seed + 4031) < 0.1) {
          const speck = hashUnit(x, y, seed + 4032) > 0.5 ? 255 : 0;
          data[to] = speck;
          data[to + 1] = speck;
          data[to + 2] = speck;
        }
        continue;
      }
      const from = pixelIndex(sx, y, width);
      data[to] = source[from];
      data[to + 1] = source[from + 1];
      data[to + 2] = source[from + 2];
    }
  }
}

// RGGB quad, indexed by ((y & 1) << 1) | (x & 1).
const BAYER_CHANNEL = [0, 1, 1, 2];

function applyBayerFault(image, config) {
  if (!config?.enabled || !(config.strength > 0.005)) return;
  const { width, height, data } = image;
  const strength = clamp(config.strength);
  const phase = Math.round(clamp(config.phaseError ?? 1, 0, 3));
  const zipper = clamp(config.zipper ?? 0);
  const ox = phase & 1;
  const oy = phase >> 1;

  // Sample the image into an RGGB mosaic at the true grid phase, then
  // demosaic assuming a shifted phase so every channel reconstructs from
  // the wrong sensor wells: green/magenta checkerboards and zipper edges.
  const source = new Uint8ClampedArray(data);
  const mosaic = new Float32Array(width * height);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const channel = BAYER_CHANNEL[((y & 1) << 1) | (x & 1)];
      mosaic[y * width + x] = source[pixelIndex(x, y, width) + channel];
    }
  }

  const sample = (x, y) => {
    const cx = x < 0 ? 0 : x >= width ? width - 1 : x;
    const cy = y < 0 ? 0 : y >= height ? height - 1 : y;
    return mosaic[cy * width + cx];
  };

  for (let y = 0; y < height; y += 1) {
    const redRow = ((y + oy) & 1) === 0;
    for (let x = 0; x < width; x += 1) {
      const redCol = ((x + ox) & 1) === 0;
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

      const index = pixelIndex(x, y, width);
      if (zipper > 0.005) {
        const left = pixelIndex(Math.max(0, x - 1), y, width);
        const right = pixelIndex(Math.min(width - 1, x + 1), y, width);
        const up = pixelIndex(x, Math.max(0, y - 1), width);
        const down = pixelIndex(x, Math.min(height - 1, y + 1), width);
        const edge =
          (Math.abs(pixelLuma(source, right) - pixelLuma(source, left)) +
            Math.abs(pixelLuma(source, down) - pixelLuma(source, up))) /
          255;
        // Alternating light/dark teeth along edges, with a green/magenta tinge.
        const shimmer = smoothstep(0.02, 0.28, edge) * zipper * ((x ^ y) & 1 ? 1 : -1);
        g += shimmer * 96;
        r += shimmer * 48;
        b += shimmer * 62;
      }

      data[index] = clampByte(lerp(source[index], r, strength));
      data[index + 1] = clampByte(lerp(source[index + 1], g, strength));
      data[index + 2] = clampByte(lerp(source[index + 2], b, strength));
    }
  }
}

function applyChromaShift(image, config, seed) {
  if (!config?.enabled || !(config.amount > 0.003)) return;
  const { width, height, data } = image;
  const source = new Uint8ClampedArray(data);
  const angle = ((config.angle ?? 0) * Math.PI) / 180;
  const distance = config.amount * Math.max(2, width * 0.014);
  const dx = Math.round(Math.cos(angle) * distance);
  const dy = Math.round(Math.sin(angle) * distance);
  const wobble = clamp(config.wobble ?? 0);

  for (let y = 0; y < height; y += 1) {
    const rowWobble = wobble > 0 ? Math.round(hashSigned(0, y, seed + 909) * wobble * distance) : 0;
    const ox = dx + rowWobble;
    for (let x = 0; x < width; x += 1) {
      const index = pixelIndex(x, y, width);
      const rx = clamp(x + ox, 0, width - 1) | 0;
      const ry = clamp(y + dy, 0, height - 1) | 0;
      const bx = clamp(x - ox, 0, width - 1) | 0;
      const by = clamp(y - dy, 0, height - 1) | 0;
      data[index] = source[pixelIndex(rx, ry, width)];
      data[index + 2] = source[pixelIndex(bx, by, width) + 2];
    }
  }
}

function applyColorBend(image, config) {
  if (!config?.enabled) return;
  const data = image.data;
  const hue = (((config.hueRotate ?? 0) % 360) + 360) % 360;
  const hueAmount = clamp(config.hueStrength ?? 1);
  const order = CHANNEL_ORDERS[config.channelMode] || CHANNEL_ORDERS.none;
  const channelAmount = clamp(config.channelStrength ?? 1);
  const invert = config.invert || "none";
  const invertAmount = clamp(config.invertStrength ?? 1);
  const solarize = clamp(config.solarize ?? 0);
  const applyHue = hue > 0.5 && hueAmount > 0.01;
  const applyOrder = order !== CHANNEL_ORDERS.none && channelAmount > 0.01;
  const applyInvert = invert !== "none" && invertAmount > 0.01;
  if (!applyHue && !applyOrder && !applyInvert && solarize <= 0.01) return;

  // Luminance-preserving hue rotation matrix (CSS filter spec coefficients).
  const rad = (hue * Math.PI) / 180;
  const cosA = Math.cos(rad);
  const sinA = Math.sin(rad);
  const m = [
    0.213 + cosA * 0.787 - sinA * 0.213,
    0.715 - cosA * 0.715 - sinA * 0.715,
    0.072 - cosA * 0.072 + sinA * 0.928,
    0.213 - cosA * 0.213 + sinA * 0.143,
    0.715 + cosA * 0.285 + sinA * 0.14,
    0.072 - cosA * 0.072 - sinA * 0.283,
    0.213 - cosA * 0.213 - sinA * 0.787,
    0.715 - cosA * 0.715 + sinA * 0.715,
    0.072 + cosA * 0.928 + sinA * 0.072
  ];
  const foldPoint = 255 * (1 - solarize * 0.55);

  for (let index = 0; index < data.length; index += 4) {
    let r = data[index];
    let g = data[index + 1];
    let b = data[index + 2];

    if (applyOrder) {
      const channels = [r, g, b];
      r = lerp(r, channels[order[0]], channelAmount);
      g = lerp(g, channels[order[1]], channelAmount);
      b = lerp(b, channels[order[2]], channelAmount);
    }

    if (applyHue) {
      const hr = r * m[0] + g * m[1] + b * m[2];
      const hg = r * m[3] + g * m[4] + b * m[5];
      const hb = r * m[6] + g * m[7] + b * m[8];
      r = lerp(r, hr, hueAmount);
      g = lerp(g, hg, hueAmount);
      b = lerp(b, hb, hueAmount);
    }

    if (applyInvert) {
      if (invert === "red" || invert === "all") r = lerp(r, 255 - r, invertAmount);
      if (invert === "green" || invert === "all") g = lerp(g, 255 - g, invertAmount);
      if (invert === "blue" || invert === "all") b = lerp(b, 255 - b, invertAmount);
    }

    if (solarize > 0.01) {
      if (r > foldPoint) r = lerp(r, foldPoint * 2 - r, solarize);
      if (g > foldPoint) g = lerp(g, foldPoint * 2 - g, solarize);
      if (b > foldPoint) b = lerp(b, foldPoint * 2 - b, solarize);
    }

    data[index] = clampByte(r);
    data[index + 1] = clampByte(g);
    data[index + 2] = clampByte(b);
  }
}

function applyGradientWash(image, config, seed) {
  if (!config?.enabled || !(config.strength > 0.01)) return;
  const { width, height, data } = image;
  const palette = PALETTES[config.mode] || PALETTES.rainbow;
  const strength = clamp(config.strength);
  const angle = ((config.angle ?? 35) * Math.PI) / 180;
  const scale = clamp(config.scale ?? 0.7, 0.05, 1);
  const keepLuma = clamp(config.keepLuma ?? 0.75);
  const wobble = clamp(config.wobble ?? 0.3);
  const cosA = Math.cos(angle);
  const sinA = Math.sin(angle);
  const span = Math.max(1, (Math.abs(cosA) * width + Math.abs(sinA) * height) * scale);
  const noiseScale = Math.max(48, Math.min(width, height) / 4);

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const index = pixelIndex(x, y, width);
      const drift = wobble > 0
        ? (fbmNoise(x / noiseScale, y / noiseScale, seed + 577) - 0.5) * wobble * 0.9
        : 0;
      const t = clamp(fract((x * cosA + y * sinA) / span + drift + 0.001), 0, 0.999);
      const color = samplePalette(palette, t);
      const lum = pixelLuma(data, index) / 255;
      const shade = Math.min(1.45, 0.34 + lum * 1.35);
      const highlight = smoothstep(0.78, 0.99, lum);
      const washR = lerp(color[0], lerp(color[0] * shade, 255, highlight), keepLuma);
      const washG = lerp(color[1], lerp(color[1] * shade, 255, highlight), keepLuma);
      const washB = lerp(color[2], lerp(color[2] * shade, 255, highlight), keepLuma);
      data[index] = clampByte(lerp(data[index], washR, strength));
      data[index + 1] = clampByte(lerp(data[index + 1], washG, strength));
      data[index + 2] = clampByte(lerp(data[index + 2], washB, strength));
    }
  }
}

function applyPixelSort(image, config, seed) {
  if (!config?.enabled || !(config.strength > 0.01)) return;
  const { width, height, data } = image;
  const strength = clamp(config.strength);
  const threshold = clamp(config.threshold ?? 0.6, 0.02, 0.98) * 255;
  const window = clamp(config.window ?? 0.35, 0.02, 1) * 255;
  const direction = config.direction || "down";
  const maxRun = Math.max(6, Math.round(clamp(config.maxRun ?? 0.6, 0.02, 1) * height));
  const brightMode = (config.mode || "bright") !== "dark";
  const columnChance = 0.15 + strength * 0.85;
  const lumaCache = new Float32Array(height);
  const runPixels = [];

  for (let x = 0; x < width; x += 1) {
    if (hashUnit(x, 7, seed + 1201) > columnChance) continue;
    const localThreshold = threshold + hashSigned(x, 11, seed + 1207) * 26;
    const low = brightMode ? localThreshold : Math.max(0, localThreshold - window);
    const high = brightMode ? Math.min(255, localThreshold + window) : localThreshold;

    for (let y = 0; y < height; y += 1) {
      lumaCache[y] = pixelLuma(data, pixelIndex(x, y, width));
    }

    let y = 0;
    while (y < height) {
      if (lumaCache[y] < low || lumaCache[y] > high) {
        y += 1;
        continue;
      }
      let runEnd = y;
      while (
        runEnd < height &&
        runEnd - y < maxRun &&
        lumaCache[runEnd] >= low &&
        lumaCache[runEnd] <= high
      ) {
        runEnd += 1;
      }
      // Extend the run past the band so bright pixels drip into darker rows.
      const drip = Math.round(
        (runEnd - y) * (0.6 + strength * 2.4) * (0.5 + hashUnit(x, y, seed + 1213))
      );
      runEnd = Math.min(height, Math.min(y + maxRun, runEnd + drip));

      if (runEnd - y > 3) {
        runPixels.length = 0;
        for (let row = y; row < runEnd; row += 1) {
          const index = pixelIndex(x, row, width);
          runPixels.push([lumaCache[row], data[index], data[index + 1], data[index + 2]]);
        }
        runPixels.sort((a, b) => (direction === "up" ? a[0] - b[0] : b[0] - a[0]));
        for (let row = y; row < runEnd; row += 1) {
          const pixel = runPixels[row - y];
          const index = pixelIndex(x, row, width);
          data[index] = pixel[1];
          data[index + 1] = pixel[2];
          data[index + 2] = pixel[3];
        }
      }
      y = runEnd + 1;
    }
  }
}

function applyExposureFault(image, config, seed) {
  if (!config?.enabled) return;
  const data = image.data;
  const gain = config.gain ?? 1;
  const blackCrush = config.blackCrush ?? 0;
  const highlightClip = config.highlightClip ?? 0;
  const contourBands = config.contourBands ?? 0;
  const bias = config.clipColorBias || [1, 0.2, 0.85];
  const threshold = clamp(0.86 - highlightClip * 0.42, 0.34, 0.95);

  for (let index = 0; index < data.length; index += 4) {
    let r = data[index] * gain;
    let g = data[index + 1] * gain;
    let b = data[index + 2] * gain;
    let lum = luma(r, g, b) / 255;

    const dark = 1 - smoothstep(0, clamp(0.08 + blackCrush * 0.42), lum);
    const darkAmount = dark * blackCrush * 1.15;
    r *= 1 - darkAmount;
    g *= 1 - darkAmount;
    b *= 1 - darkAmount;

    lum = luma(r, g, b) / 255;
    const hot = smoothstep(threshold, 1, lum);
    if (hot > 0) {
      const band = Math.sin((lum * 20 + seed * 0.00001) * Math.PI);
      const contour = Math.abs(band) * contourBands * hot;
      const core = smoothstep(0.78, 1, hot);
      const clipR = lerp(255 * bias[0], 255, core);
      const clipG = lerp(255 * bias[1], 255, core);
      const clipB = lerp(255 * bias[2], 255, core);
      const amount = clamp(hot * (0.58 + highlightClip * 0.5) + contour * 0.28);
      r = lerp(r, clipR, amount);
      g = lerp(g, clipG, amount);
      b = lerp(b, clipB, amount);
    }

    data[index] = clampByte(r);
    data[index + 1] = clampByte(g);
    data[index + 2] = clampByte(b);
  }
}

function applyContourRings(image, config, seed) {
  if (!config?.enabled || config.strength <= 0) return;
  const { width, height, data } = image;
  const source = new Uint8ClampedArray(data);
  const strength = clamp(config.strength);
  const scale = clamp(config.scale ?? 0.5);
  const bandSharpness = clamp(config.bandSharpness ?? 0.5);
  const tonalBias = clamp(config.tonalBias ?? 0.5);
  const colorBleed = clamp(config.colorBleed ?? 0.5);
  const contourLevels = 5 + scale * 18;
  const noiseScale = 180 - scale * 120;

  for (let y = 1; y < height - 1; y += 1) {
    for (let x = 1; x < width - 1; x += 1) {
      const index = pixelIndex(x, y, width);
      const r = source[index];
      const g = source[index + 1];
      const b = source[index + 2];
      const lum = luma(r, g, b) / 255;
      const left = pixelIndex(x - 1, y, width);
      const right = pixelIndex(x + 1, y, width);
      const up = pixelIndex(x, y - 1, width);
      const down = pixelIndex(x, y + 1, width);
      const edge =
        (Math.abs(pixelLuma(source, right) - pixelLuma(source, left)) +
          Math.abs(pixelLuma(source, down) - pixelLuma(source, up))) /
        255;
      const smoothResponse = 1 - smoothstep(0.035, 0.32, edge);
      const tonalResponse = lerp(1, smoothstep(0.16, 0.96, lum), tonalBias);

      const field = fbmNoise(
        x / Math.max(24, noiseScale),
        y / Math.max(24, noiseScale * 0.72),
        seed + 917
      );
      const tonal = lum * contourLevels + field * (1.2 + scale * 2.4);
      const contourPhase = Math.abs(fract(tonal) - 0.5) * 2;
      const lineWidth = 0.34 - bandSharpness * 0.24;
      const ringLine = 1 - smoothstep(0.035, Math.max(0.055, lineWidth), contourPhase);
      const amount = clamp(
        ringLine *
          strength *
          (0.28 + smoothResponse * 0.72) *
          tonalResponse *
          (0.62 + colorBleed * 0.52)
      );
      if (amount <= 0.003) continue;

      const color = samplePalette(
        PALETTES["solarized-ccd"],
        fract(lum + field * 0.38 + hashUnit(Math.floor(x / 47), Math.floor(y / 47), seed + 921))
      );
      const lift = ringLine * strength * 34;
      data[index] = clampByte(lerp(data[index] + lift, color[0], amount * colorBleed));
      data[index + 1] = clampByte(lerp(data[index + 1] + lift * 0.55, color[1], amount));
      data[index + 2] = clampByte(lerp(data[index + 2] + lift * 0.8, color[2], amount));
    }
  }
}

function applyFalseColor(image, config, seed) {
  if (!config?.enabled) return;
  const data = image.data;
  const palette = PALETTES[config.mode] || PALETTES["solarized-ccd"];
  const strength = clamp(config.strength ?? 0);
  const levels = Math.max(2, Math.round(config.posterizeLevels || 7));
  const smoothness = clamp(config.smoothness ?? 0);
  const channelSwap = clamp(config.channelSwap ?? 0);
  const hueWarp = clamp(config.hueWarp ?? 0);
  const saturation = config.saturation ?? 1;

  for (let index = 0; index < data.length; index += 4) {
    let r = data[index];
    let g = data[index + 1];
    let b = data[index + 2];
    const lum = luma(r, g, b) / 255;
    const poster = lerp(Math.round(lum * (levels - 1)) / (levels - 1), lum, smoothness);
    const solar = 1 - Math.abs(((poster + hueWarp * 0.42) % 1) * 2 - 1);
    const color = samplePalette(palette, clamp(lerp(poster, solar, hueWarp * 0.75)));

    const swappedR = lerp(r, g, channelSwap * 0.55);
    const swappedG = lerp(g, b, channelSwap * 0.55);
    const swappedB = lerp(b, r, channelSwap * 0.55);

    r = lerp(swappedR, color[0], strength);
    g = lerp(swappedG, color[1], strength);
    b = lerp(swappedB, color[2], strength);

    const nextLum = luma(r, g, b);
    r = nextLum + (r - nextLum) * saturation;
    g = nextLum + (g - nextLum) * saturation;
    b = nextLum + (b - nextLum) * saturation;

    data[index] = clampByte(r);
    data[index + 1] = clampByte(g);
    data[index + 2] = clampByte(b);
  }
}

function applyEdgeBurn(image, config, seed) {
  if (!config?.enabled || config.strength <= 0) return;
  const { width, height, data } = image;
  const source = new Uint8ClampedArray(data);
  const strength = clamp(config.strength);
  const threshold = config.threshold ?? 0.12;
  const darkOutline = clamp(config.darkOutline ?? 0);
  const palette = config.palette?.length ? config.palette : ["cyan", "magenta", "green"];

  for (let y = 1; y < height - 1; y += 1) {
    for (let x = 1; x < width - 1; x += 1) {
      const index = pixelIndex(x, y, width);
      const left = pixelIndex(x - 1, y, width);
      const right = pixelIndex(x + 1, y, width);
      const up = pixelIndex(x, y - 1, width);
      const down = pixelIndex(x, y + 1, width);
      const dx = Math.abs(pixelLuma(source, right) - pixelLuma(source, left));
      const dy = Math.abs(pixelLuma(source, down) - pixelLuma(source, up));
      const edge = (dx + dy) / 255;
      const amount = smoothstep(threshold, threshold + 0.38, edge) * strength;
      if (amount <= 0) continue;

      const name = palette[Math.floor(hashUnit(x, y, seed + 17) * palette.length)];
      const color = EDGE_COLORS[name] || EDGE_COLORS.cyan;
      const outline = amount * darkOutline * smoothstep(0.38, 0.95, edge);

      data[index] = clampByte(lerp(lerp(source[index], 0, outline), color[0], amount));
      data[index + 1] = clampByte(lerp(lerp(source[index + 1], 0, outline), color[1], amount));
      data[index + 2] = clampByte(lerp(lerp(source[index + 2], 0, outline), color[2], amount));
    }
  }
}

function applyVerticalSmear(image, config, seed) {
  if (!config?.enabled || config.strength <= 0) return;
  const { width, height, data } = image;
  const source = new Uint8ClampedArray(data);
  const strength = clamp(config.strength);
  const length = clamp(config.length ?? strength);
  const spread = clamp(config.spread ?? 0.18);
  const contrast = clamp(config.contrast ?? strength);
  const curtainStrength = clamp(config.curtainStrength ?? 0);
  const curtainDensity = clamp(config.curtainDensity ?? 0);
  const curtainDrop = clamp(config.curtainDrop ?? length);
  const threshold = clamp((config.threshold ?? 0.58) - length * 0.1, 0.02, 0.95);
  const baseDecay = clamp((config.decay ?? 0.92) + length * 0.018, 0.5, 0.997);
  const resolutionScale = Math.max(1, height / 1000);
  const decay = clamp(1 - (1 - baseDecay) / resolutionScale, 0.5, 0.99975);
  const jitter = clamp(config.jitter ?? 0);
  const edgeBias = clamp(config.edgeBias ?? 0.35);
  const radius = Math.floor(spread * 4);
  const carryR = new Float32Array(width);
  const carryG = new Float32Array(width);
  const carryB = new Float32Array(width);
  const energy = new Float32Array(width);
  const curtainR = new Float32Array(width);
  const curtainG = new Float32Array(width);
  const curtainB = new Float32Array(width);
  const curtainEnergy = new Float32Array(width);
  const curtainLife = new Float32Array(width);
  const maxCurtainLife = Math.max(8, height * (0.06 + curtainDrop * 0.58));

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const index = pixelIndex(x, y, width);
      const lum = pixelLuma(source, index) / 255;
      const left = pixelIndex(Math.max(0, x - 1), y, width);
      const right = pixelIndex(Math.min(width - 1, x + 1), y, width);
      const up = pixelIndex(x, Math.max(0, y - 1), width);
      const down = pixelIndex(x, Math.min(height - 1, y + 1), width);
      const edge =
        (Math.abs(pixelLuma(source, right) - pixelLuma(source, left)) +
          Math.abs(pixelLuma(source, down) - pixelLuma(source, up))) /
        255;
      const trigger = clamp(
        smoothstep(threshold, 1, lum) + smoothstep(0.06, 0.56, edge) * edgeBias
      );

      if (trigger > 0.02) {
        const spark = hashUnit(x, y, seed + 29);
        const triggerAmount = clamp(trigger * (0.42 + strength * 0.95) * (0.82 + spark * 0.62));
        if (triggerAmount > energy[x] * 1.08) {
          const hotBoost = smoothstep(threshold, 1, lum) * strength * 42;
          carryR[x] = clampByte(source[index] + hotBoost);
          carryG[x] = clampByte(source[index + 1] + hotBoost * 0.45);
          carryB[x] = clampByte(source[index + 2] + hotBoost * 0.85);
          energy[x] = clamp(Math.max(energy[x], triggerAmount));
        }

        if (curtainStrength > 0.01 && curtainDensity > 0.01) {
          const verticalEdge = Math.abs(pixelLuma(source, down) - pixelLuma(source, up)) / 255;
          const startSignal = trigger * smoothstep(0.08, 0.58, edge + verticalEdge);
          const cellX = Math.floor(x / Math.max(1, Math.round(1 + curtainDensity * 9)));
          const cellY = Math.floor(y / Math.max(1, Math.round(2 + (1 - curtainDensity) * 14)));
          const gate = hashUnit(cellX, cellY, seed + 733);
          const startChance = startSignal * curtainDensity * (0.08 + curtainStrength * 0.22);
          if (gate < startChance && curtainLife[x] < maxCurtainLife * 0.16) {
            const lifeSeed = hashUnit(cellX, cellY, seed + 734);
            curtainR[x] = source[index];
            curtainG[x] = source[index + 1];
            curtainB[x] = source[index + 2];
            curtainEnergy[x] = clamp(curtainStrength * (0.42 + startSignal * 0.9 + lifeSeed * 0.35));
            curtainLife[x] = Math.max(6, maxCurtainLife * (0.22 + lifeSeed * 0.78));
          }
        }
      }

      const offset = Math.round(hashSigned(x, y, seed + 71) * jitter * 7);
      const carryX = clamp(x + offset, 0, width - 1) | 0;
      let sampleR = 0;
      let sampleG = 0;
      let sampleB = 0;
      let sampleEnergy = 0;

      for (let delta = -radius; delta <= radius; delta += 1) {
        const sx = clamp(carryX + delta, 0, width - 1) | 0;
        const columnWeight = radius === 0 ? 1 : 1 - Math.abs(delta) / (radius + 1);
        const columnEnergy = energy[sx] * columnWeight;
        sampleR += carryR[sx] * columnEnergy;
        sampleG += carryG[sx] * columnEnergy;
        sampleB += carryB[sx] * columnEnergy;
        sampleEnergy += columnEnergy;
      }

      const smear = clamp(sampleEnergy * (0.34 + strength * 0.96), 0, 0.98);
      if (smear > 0.004) {
        const invEnergy = 1 / Math.max(sampleEnergy, 0.0001);
        const blendedR = lerp(data[index], sampleR * invEnergy, smear);
        const blendedG = lerp(data[index + 1], sampleG * invEnergy, smear);
        const blendedB = lerp(data[index + 2], sampleB * invEnergy, smear);
        const line = smoothstep(0.02, 0.72, sampleEnergy) * contrast * (0.28 + strength * 0.52);
        const columnShade = 1 - line * (0.26 + hashUnit(carryX, 0, seed + 811) * 0.22);
        data[index] = clampByte(blendedR * columnShade);
        data[index + 1] = clampByte(blendedG * columnShade);
        data[index + 2] = clampByte(blendedB * columnShade);
      }

      if (curtainStrength > 0.01) {
        const curtainRadius = radius + Math.floor(curtainDensity * 4);
        let curtainSampleR = 0;
        let curtainSampleG = 0;
        let curtainSampleB = 0;
        let curtainSampleEnergy = 0;

        for (let delta = -curtainRadius; delta <= curtainRadius; delta += 1) {
          const sx = clamp(carryX + delta, 0, width - 1) | 0;
          if (curtainLife[sx] <= 0 || curtainEnergy[sx] <= 0) continue;
          const columnWeight =
            curtainRadius === 0 ? 1 : 1 - Math.abs(delta) / (curtainRadius + 1);
          const lifeFade = smoothstep(0, maxCurtainLife * 0.2, curtainLife[sx]);
          const columnEnergy = curtainEnergy[sx] * columnWeight * lifeFade;
          curtainSampleR += curtainR[sx] * columnEnergy;
          curtainSampleG += curtainG[sx] * columnEnergy;
          curtainSampleB += curtainB[sx] * columnEnergy;
          curtainSampleEnergy += columnEnergy;
        }

        if (curtainSampleEnergy > 0.004) {
          const invEnergy = 1 / Math.max(curtainSampleEnergy, 0.0001);
          const curtainAmount = clamp(
            curtainSampleEnergy * (0.48 + curtainStrength * 0.82),
            0,
            0.96
          );
          const stripeShade =
            1 -
            smoothstep(0.04, 0.8, curtainSampleEnergy) *
              contrast *
              (0.32 + hashUnit(carryX, 0, seed + 841) * 0.28);
          data[index] = clampByte(
            lerp(data[index], curtainSampleR * invEnergy, curtainAmount) * stripeShade
          );
          data[index + 1] = clampByte(
            lerp(data[index + 1], curtainSampleG * invEnergy, curtainAmount) * stripeShade
          );
          data[index + 2] = clampByte(
            lerp(data[index + 2], curtainSampleB * invEnergy, curtainAmount) * stripeShade
          );
        }
      }
    }

    for (let x = 0; x < width; x += 1) {
      const unevenDecay = decay - hashUnit(x, y, seed + 43) * jitter * 0.018;
      energy[x] *= clamp(unevenDecay, 0.5, 0.999);
      if (curtainLife[x] > 0) {
        curtainLife[x] -= 1;
        curtainEnergy[x] *= 0.9985 - hashUnit(x, y, seed + 847) * jitter * 0.003;
      }
    }
  }
}

function applySensorNoise(image, config, seed) {
  if (!config?.enabled || config.amount <= 0) return;
  const { width, height, data } = image;
  const amount = clamp(config.amount);
  const colorAmount = clamp(config.colorAmount ?? 0.75);
  const shadowBias = clamp(config.shadowBias ?? 0.5);
  const striping = clamp(config.striping ?? 0);
  const hotPixels = clamp(config.hotPixels ?? 0);

  for (let y = 0; y < height; y += 1) {
    const columnSeed = seed + 211;
    for (let x = 0; x < width; x += 1) {
      const index = pixelIndex(x, y, width);
      const lum = pixelLuma(data, index) / 255;
      const weight = amount * (0.32 + shadowBias * (1 - lum));
      const stripe = hashSigned(x, 0, columnSeed) * striping * 28;
      const nr = hashSigned(x, y, seed + 101) * 255 * weight;
      const ng = hashSigned(x, y, seed + 102) * 255 * weight * colorAmount;
      const nb = hashSigned(x, y, seed + 103) * 255 * weight * colorAmount;
      data[index] = clampByte(data[index] + nr + stripe);
      data[index + 1] = clampByte(data[index + 1] + ng + stripe * 0.35);
      data[index + 2] = clampByte(data[index + 2] + nb - stripe * 0.25);

      if (hashUnit(x, y, seed + 313) < hotPixels * 0.0018) {
        const color = samplePalette(PALETTES["solarized-ccd"], hashUnit(x, y, seed + 314));
        data[index] = color[0];
        data[index + 1] = color[1];
        data[index + 2] = color[2];
      }
    }
  }
}

function applyMemoryFault(image, config, seed) {
  if (!config?.enabled) return;
  const { width, height, data } = image;

  if (config.interlace > 0) {
    const amount = clamp(config.interlace);
    const source = new Uint8ClampedArray(data);
    const bandSize = Math.max(8, Math.round(height * 0.05));
    for (let y = 0; y < height; y += 1) {
      const band = Math.floor(y / bandSize);
      if (hashUnit(3, band, seed + 651) > amount * 0.85) continue;
      if (y % 2 === 0) continue;
      const shift = Math.round(
        hashSigned(4, band, seed + 652) * width * (0.015 + amount * 0.08)
      );
      const chromaSplit = Math.round(1 + amount * 5);
      for (let x = 0; x < width; x += 1) {
        const to = pixelIndex(x, y, width);
        const rx = (((x - shift - chromaSplit) % width) + width) % width;
        const gx = (((x - shift) % width) + width) % width;
        const bx = (((x - shift + chromaSplit) % width) + width) % width;
        data[to] = source[pixelIndex(rx, y, width)];
        data[to + 1] = source[pixelIndex(gx, y, width) + 1];
        data[to + 2] = source[pixelIndex(bx, y, width) + 2];
      }
    }
  }

  if (config.blockShift > 0) {
    const source = new Uint8ClampedArray(data);
    const amount = clamp(config.blockShift);
    let y = 0;
    while (y < height) {
      const blockHeight = 2 + Math.floor(hashUnit(0, y, seed + 401) * (6 + amount * 80));
      const shouldShift = hashUnit(1, y, seed + 402) < amount * 0.42;
      if (shouldShift) {
        const shift = Math.round(hashSigned(2, y, seed + 403) * width * (0.04 + amount * 0.28));
        for (let row = y; row < Math.min(height, y + blockHeight); row += 1) {
          for (let x = 0; x < width; x += 1) {
            const sx = (x - shift + width) % width;
            const to = pixelIndex(x, row, width);
            const from = pixelIndex(sx, row, width);
            data[to] = source[from];
            data[to + 1] = source[from + 1];
            data[to + 2] = source[from + 2];
          }
        }
      }
      y += blockHeight;
    }
  }

  if (config.rowRepeat > 0) {
    const amount = clamp(config.rowRepeat);
    for (let y = 1; y < height; y += 1) {
      if (hashUnit(0, y, seed + 501) > amount * 0.3) continue;
      const fromY = Math.max(0, y - 1 - Math.floor(hashUnit(1, y, seed + 502) * 28));
      copyRow(data, width, fromY, y);
    }
  }

  if (config.scanlineDropout > 0) {
    const amount = clamp(config.scanlineDropout);
    for (let y = 0; y < height; y += 1) {
      if (hashUnit(0, y, seed + 601) > amount * 0.32) continue;
      const mode = Math.floor(hashUnit(1, y, seed + 602) * 4);
      for (let x = 0; x < width; x += 1) {
        const index = pixelIndex(x, y, width);
        if (mode === 0) {
          data[index] = 0;
          data[index + 1] = 0;
          data[index + 2] = 0;
        } else if (mode === 1) {
          data[index] = clampByte(data[index] * 1.35 + 44);
          data[index + 1] = clampByte(data[index + 1] * 0.75);
        } else if (mode === 2) {
          data[index + 2] = clampByte(data[index + 2] * 1.5 + 55);
        } else {
          data[index] = 255 - data[index];
          data[index + 1] = 255 - data[index + 1];
          data[index + 2] = 255 - data[index + 2];
        }
      }
    }
  }
}

// Orthonormal 8x8 DCT basis: DCT_TABLE[u * 8 + x] = c(u) * cos((2x + 1) * u * PI / 16).
const DCT_TABLE = (() => {
  const table = new Float32Array(64);
  for (let u = 0; u < 8; u += 1) {
    const c = u === 0 ? Math.sqrt(1 / 8) : 0.5;
    for (let x = 0; x < 8; x += 1) {
      table[u * 8 + x] = c * Math.cos(((2 * x + 1) * u * Math.PI) / 16);
    }
  }
  return table;
})();

function dct8Forward(src, dst, tmp) {
  for (let y = 0; y < 8; y += 1) {
    for (let u = 0; u < 8; u += 1) {
      let sum = 0;
      for (let x = 0; x < 8; x += 1) sum += src[y * 8 + x] * DCT_TABLE[u * 8 + x];
      tmp[y * 8 + u] = sum;
    }
  }
  for (let u = 0; u < 8; u += 1) {
    for (let v = 0; v < 8; v += 1) {
      let sum = 0;
      for (let y = 0; y < 8; y += 1) sum += tmp[y * 8 + u] * DCT_TABLE[v * 8 + y];
      dst[v * 8 + u] = sum;
    }
  }
}

function dct8Inverse(src, dst, tmp) {
  for (let u = 0; u < 8; u += 1) {
    for (let y = 0; y < 8; y += 1) {
      let sum = 0;
      for (let v = 0; v < 8; v += 1) sum += src[v * 8 + u] * DCT_TABLE[v * 8 + y];
      tmp[y * 8 + u] = sum;
    }
  }
  for (let y = 0; y < 8; y += 1) {
    for (let x = 0; x < 8; x += 1) {
      let sum = 0;
      for (let u = 0; u < 8; u += 1) sum += tmp[y * 8 + u] * DCT_TABLE[u * 8 + x];
      dst[y * 8 + x] = sum;
    }
  }
}

function subsampleChroma(plane, width, height, blend) {
  for (let y = 0; y < height; y += 2) {
    const y1 = Math.min(height - 1, y + 1);
    for (let x = 0; x < width; x += 2) {
      const x1 = Math.min(width - 1, x + 1);
      const a = plane[y * width + x];
      const b = plane[y * width + x1];
      const c = plane[y1 * width + x];
      const d = plane[y1 * width + x1];
      const mean = (a + b + c + d) * 0.25;
      plane[y * width + x] = lerp(a, mean, blend);
      plane[y * width + x1] = lerp(b, mean, blend);
      plane[y1 * width + x] = lerp(c, mean, blend);
      plane[y1 * width + x1] = lerp(d, mean, blend);
    }
  }
}

function applyDctCrunch(image, config, seed) {
  if (!config?.enabled) return;
  const quality = clamp(config.quality ?? 0.5);
  const dcDrift = clamp(config.dcDrift ?? 0);
  const acScramble = clamp(config.acScramble ?? 0);
  const blockRepeat = clamp(config.blockRepeat ?? 0);
  const chromaSubsample = clamp(config.chromaSubsample ?? 0);
  const wantsDct =
    quality < 0.985 || dcDrift > 0.005 || acScramble > 0.005 || blockRepeat > 0.005;
  if (!wantsDct && chromaSubsample <= 0.005) return;

  const { width, height, data } = image;
  const size = width * height;
  const planes = [new Float32Array(size), new Float32Array(size), new Float32Array(size)];
  const [Y, Cb, Cr] = planes;

  for (let i = 0, p = 0; p < size; i += 4, p += 1) {
    const r = data[i];
    const g = data[i + 1];
    const b = data[i + 2];
    Y[p] = r * 0.299 + g * 0.587 + b * 0.114;
    Cb[p] = r * -0.168736 + g * -0.331264 + b * 0.5 + 128;
    Cr[p] = r * 0.5 + g * -0.418688 + b * -0.081312 + 128;
  }

  if (chromaSubsample > 0.005) {
    subsampleChroma(Cb, width, height, chromaSubsample);
    subsampleChroma(Cr, width, height, chromaSubsample);
  }

  if (wantsDct) {
    const blocksX = Math.ceil(width / 8);
    const blocksY = Math.ceil(height / 8);
    const pixels = new Float32Array(64);
    const coeffs = new Float32Array(64);
    const scratch = new Float32Array(64);
    const decoded = [new Float32Array(64), new Float32Array(64), new Float32Array(64)];
    const held = [new Float32Array(64), new Float32Array(64), new Float32Array(64)];
    // Quantizer step in pixel units; higher frequencies get coarser steps like JPEG.
    const qBase = quality >= 0.985 ? 0 : 1 + Math.pow(1 - quality, 1.5) * 56;
    // DC drift is a random walk along block-scan order; chroma wanders further
    // than luma so color slides into wrong hues before brightness collapses.
    // The walk rate scales with total block count so the drift spans the whole
    // frame at any resolution, and hard jumps stay rare (a few per frame).
    const totalBlocks = blocksX * blocksY;
    const walkScale = 1.6 / Math.sqrt(totalBlocks);
    const jumpChance = (dcDrift * 5) / totalBlocks;
    const driftCaps = [dcDrift * 70, dcDrift * 150, dcDrift * 150];
    const driftRates = driftCaps.map((cap) => cap * walkScale);
    const drift = [0, 0, 0];
    let holdCount = 0;

    for (let by = 0; by < blocksY; by += 1) {
      for (let bx = 0; bx < blocksX; bx += 1) {
        // Corrupt macroblocks arrive in patches, not uniform sprinkle.
        const patch = fbmNoise(bx / 14, by / 14, seed + 2101);
        const scrambleBlock = acScramble > 0.005 && patch < acScramble * 0.72;

        for (let plane = 0; plane < 3; plane += 1) {
          const source = planes[plane];
          for (let y = 0; y < 8; y += 1) {
            const sy = Math.min(height - 1, by * 8 + y);
            for (let x = 0; x < 8; x += 1) {
              const sx = Math.min(width - 1, bx * 8 + x);
              pixels[y * 8 + x] = source[sy * width + sx];
            }
          }

          dct8Forward(pixels, coeffs, scratch);

          if (qBase > 0) {
            for (let i = 1; i < 64; i += 1) {
              const u = i & 7;
              const v = i >> 3;
              const step = qBase * (1 + (u + v) * 0.8);
              coeffs[i] = Math.round(coeffs[i] / step) * step;
            }
            coeffs[0] = Math.round(coeffs[0] / qBase) * qBase;
          }

          if (scrambleBlock) {
            for (let i = 1; i < 64; i += 1) {
              const roll = hashUnit(bx * 64 + i, by * 3 + plane, seed + 2113);
              if (roll < 0.32) {
                coeffs[i] = 0;
              } else if (roll < 0.48) {
                const from = 1 + Math.floor(hashUnit(bx * 64 + i, by, seed + 2117) * 63);
                coeffs[i] = coeffs[from] * 1.6;
              } else if (roll < 0.56) {
                // Inject garbage energy so corruption reads even in flat blocks.
                coeffs[i] = hashSigned(bx * 64 + i, by + 5, seed + 2119) * 85;
              } else if (roll < 0.64) {
                coeffs[i] *= -1.8;
              }
            }
          }

          if (dcDrift > 0.005) {
            if (plane === 0) {
              // One walk step per block, shared timing across planes.
              for (let p = 0; p < 3; p += 1) {
                drift[p] += hashSigned(bx, by, seed + 2131 + p) * driftRates[p];
                if (hashUnit(bx, by, seed + 2141 + p) < jumpChance) {
                  drift[p] += hashSigned(bx + 7, by + 7, seed + 2151 + p) * driftCaps[p];
                }
                drift[p] = clamp(drift[p], -driftCaps[p], driftCaps[p]);
              }
            }
            coeffs[0] += drift[plane] * 8;
          }

          dct8Inverse(coeffs, decoded[plane], scratch);
        }

        if (blockRepeat > 0.005) {
          if (holdCount > 0) {
            holdCount -= 1;
            for (let plane = 0; plane < 3; plane += 1) decoded[plane].set(held[plane]);
          } else if (
            // Stutters cluster in bursts (patch-gated), not uniform sprinkle.
            fbmNoise(bx / 11, by / 11, seed + 2167) < blockRepeat * 0.6 &&
            hashUnit(bx, by, seed + 2161) < 0.14 + blockRepeat * 0.2
          ) {
            holdCount = 1 + Math.floor(hashUnit(bx, by, seed + 2163) * (1 + blockRepeat * 10));
            for (let plane = 0; plane < 3; plane += 1) held[plane].set(decoded[plane]);
          }
        }

        const maxY = Math.min(8, height - by * 8);
        const maxX = Math.min(8, width - bx * 8);
        for (let plane = 0; plane < 3; plane += 1) {
          const target = planes[plane];
          for (let y = 0; y < maxY; y += 1) {
            const row = (by * 8 + y) * width + bx * 8;
            for (let x = 0; x < maxX; x += 1) {
              target[row + x] = decoded[plane][y * 8 + x];
            }
          }
        }
      }
    }
  }

  for (let i = 0, p = 0; p < size; i += 4, p += 1) {
    const luminance = Y[p];
    const cb = Cb[p] - 128;
    const cr = Cr[p] - 128;
    data[i] = clampByte(luminance + 1.402 * cr);
    data[i + 1] = clampByte(luminance - 0.344136 * cb - 0.714136 * cr);
    data[i + 2] = clampByte(luminance + 1.772 * cb);
  }
}

const BAYER_4X4 = [
  [0, 8, 2, 10],
  [12, 4, 14, 6],
  [3, 11, 1, 9],
  [15, 7, 13, 5]
];

function applyFinalCrunch(image, pipeline) {
  const config = pipeline.cheapCamera;
  if (!config?.enabled) return;
  const { width, height, data } = image;
  const bitDepth = Math.max(2, Math.min(8, Math.round(config.bitDepth || 8)));
  const levels = 2 ** bitDepth - 1;
  const dither = clamp(config.dither ?? 0);
  const step = 255 / levels;

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const index = pixelIndex(x, y, width);
      const bias = dither > 0 ? (BAYER_4X4[y & 3][x & 3] / 16 - 0.5) * step * dither : 0;
      data[index] = Math.round((data[index] + bias) / step) * step;
      data[index + 1] = Math.round((data[index + 1] + bias) / step) * step;
      data[index + 2] = Math.round((data[index + 2] + bias) / step) * step;
    }
  }

  const sharpen = clamp(config.sharpen ?? 0);
  if (sharpen <= 0.01) return;
  const source = new Uint8ClampedArray(data);
  const amount = sharpen * 0.42;

  for (let y = 1; y < height - 1; y += 1) {
    for (let x = 1; x < width - 1; x += 1) {
      const index = pixelIndex(x, y, width);
      const left = pixelIndex(x - 1, y, width);
      const right = pixelIndex(x + 1, y, width);
      const up = pixelIndex(x, y - 1, width);
      const down = pixelIndex(x, y + 1, width);
      for (let channel = 0; channel < 3; channel += 1) {
        const center = source[index + channel];
        const neighbor =
          (source[left + channel] +
            source[right + channel] +
            source[up + channel] +
            source[down + channel]) /
          4;
        data[index + channel] = clampByte(center + (center - neighbor) * amount * 2.5);
      }
    }
  }
}

// 5x7 bitmap font for the on-screen display, one int per row, MSB = left pixel.
const OSD_FONT = {
  "0": [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
  "1": [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
  "2": [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
  "3": [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
  "4": [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
  "5": [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
  "6": [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
  "7": [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
  "8": [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
  "9": [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
  A: [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
  B: [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
  C: [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
  D: [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
  E: [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
  F: [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
  G: [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
  I: [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
  M: [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
  N: [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
  O: [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
  P: [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
  R: [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
  S: [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
  T: [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
  U: [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
  V: [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
  W: [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
  "'": [0b00100, 0b00100, 0b01000, 0b00000, 0b00000, 0b00000, 0b00000],
  ":": [0b00000, 0b00100, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000],
  ".": [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100],
  "/": [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
  "-": [0b00000, 0b00000, 0b00000, 0b01110, 0b00000, 0b00000, 0b00000],
  " ": [0, 0, 0, 0, 0, 0, 0]
};

const OSD_GLYPH_KEYS = Object.keys(OSD_FONT).filter((key) => key !== " ");

const OSD_COLORS = {
  orange: [255, 158, 42],
  green: [96, 255, 128],
  white: [242, 248, 242]
};

function osdFillRect(image, x, y, w, h, color, alpha) {
  const { width, height, data } = image;
  const x0 = Math.max(0, Math.round(x));
  const y0 = Math.max(0, Math.round(y));
  const x1 = Math.min(width, Math.round(x + w));
  const y1 = Math.min(height, Math.round(y + h));
  for (let py = y0; py < y1; py += 1) {
    for (let px = x0; px < x1; px += 1) {
      const index = pixelIndex(px, py, width);
      data[index] = clampByte(lerp(data[index], color[0], alpha));
      data[index + 1] = clampByte(lerp(data[index + 1], color[1], alpha));
      data[index + 2] = clampByte(lerp(data[index + 2], color[2], alpha));
    }
  }
}

function osdDrawGlyph(image, glyph, x0, y0, px, color, alpha) {
  for (let row = 0; row < 7; row += 1) {
    const bits = glyph[row];
    if (!bits) continue;
    for (let col = 0; col < 5; col += 1) {
      if (bits & (1 << (4 - col))) {
        osdFillRect(image, x0 + col * px, y0 + row * px, px, px, color, alpha);
      }
    }
  }
}

function osdDrawText(image, text, x0, y0, px, color, glitch, seed, alpha = 0.92) {
  const doubled = glitch > 0.01 && hashUnit(1, 9, seed) < glitch * 0.5;
  const shadow = Math.max(1, Math.round(px * 0.4));
  for (let i = 0; i < text.length; i += 1) {
    let key = text[i];
    let dy = 0;
    if (glitch > 0.01 && key !== " ") {
      if (hashUnit(i, 1, seed) < glitch * 0.55) {
        key = OSD_GLYPH_KEYS[Math.floor(hashUnit(i, 2, seed) * OSD_GLYPH_KEYS.length)];
      }
      if (hashUnit(i, 3, seed) < glitch * 0.35) {
        dy = Math.round(hashSigned(i, 4, seed) * px * 2);
      }
    }
    const glyph = OSD_FONT[key] || OSD_FONT[" "];
    const x = x0 + i * px * 6;
    osdDrawGlyph(image, glyph, x + shadow, y0 + dy + shadow, px, [10, 10, 10], alpha * 0.45);
    osdDrawGlyph(image, glyph, x, y0 + dy, px, color, alpha);
    if (doubled) {
      osdDrawGlyph(image, glyph, x + px * 2, y0 + dy - px, px, color, alpha * 0.4);
    }
  }
}

function osdTextWidth(text, px) {
  return text.length * px * 6 - px;
}

function applyOsdOverlay(image, config, seed) {
  if (!config?.enabled) return;
  const showDate = config.datestamp !== false;
  const showHud = !!config.hudIcons;
  if (!showDate && !showHud) return;
  const glitch = clamp(config.glitchText ?? 0);
  const scale = clamp(config.scale ?? 0.5);
  const color = OSD_COLORS[config.color] || OSD_COLORS.orange;
  const { width, height } = image;
  const px = Math.max(1, Math.round((Math.min(width, height) / 230) * (0.55 + scale * 1.5)));
  const margin = px * 5;

  if (showDate) {
    const yy = Math.floor(hashUnit(3, 1, seed + 3001) * 8);
    const mm = 1 + Math.floor(hashUnit(3, 2, seed + 3002) * 12);
    const dd = 1 + Math.floor(hashUnit(3, 3, seed + 3003) * 28);
    const text = `'0${yy} ${mm} ${dd}`;
    osdDrawText(
      image,
      text,
      width - margin - osdTextWidth(text, px),
      height - margin - px * 7,
      px,
      color,
      glitch,
      seed + 11
    );
  }

  if (showHud) {
    const isoValues = [64, 80, 100, 200, 400];
    const iso = isoValues[Math.floor(hashUnit(5, 1, seed + 3011) * isoValues.length)];
    osdDrawText(image, `ISO ${iso}`, margin, height - margin - px * 7, px, color, glitch, seed + 13);
    osdDrawText(image, "REC", margin + px * 6, margin, px, color, glitch, seed + 17);
    // Round-ish record dot to the left of REC.
    const dot = [0b0110, 0b1111, 0b1111, 0b0110];
    for (let row = 0; row < 4; row += 1) {
      for (let col = 0; col < 4; col += 1) {
        if (dot[row] & (1 << (3 - col))) {
          osdFillRect(image, margin + col * px, margin + px + row * px, px, px, [255, 46, 46], 0.92);
        }
      }
    }

    // Battery: outline body, charge bars, terminal nub.
    const batteryX = width - margin - px * 12;
    const batteryLevel = 1 + Math.floor(hashUnit(7, 1, seed + 3021) * 3);
    osdFillRect(image, batteryX, margin, px * 11, px, color, 0.92);
    osdFillRect(image, batteryX, margin + px * 4, px * 11, px, color, 0.92);
    osdFillRect(image, batteryX, margin, px, px * 5, color, 0.92);
    osdFillRect(image, batteryX + px * 10, margin, px, px * 5, color, 0.92);
    osdFillRect(image, batteryX + px * 11, margin + px, px, px * 3, color, 0.92);
    for (let bar = 0; bar < batteryLevel; bar += 1) {
      osdFillRect(image, batteryX + px * (2 + bar * 3), margin + px * 1.5, px * 2, px * 2, color, 0.92);
    }

    // Center focus brackets.
    const arm = px * 5;
    const left = Math.round(width * 0.34);
    const right = Math.round(width * 0.66);
    const top = Math.round(height * 0.36);
    const bottom = Math.round(height * 0.64);
    const corners = [
      [left, top, 1, 1],
      [right, top, -1, 1],
      [left, bottom, 1, -1],
      [right, bottom, -1, -1]
    ];
    for (const [cx, cy, sx, sy] of corners) {
      osdFillRect(image, sx > 0 ? cx : cx - arm, cy - (sy > 0 ? 0 : px), arm, px, color, 0.8);
      osdFillRect(image, cx - (sx > 0 ? 0 : px), sy > 0 ? cy : cy - arm, px, arm, color, 0.8);
    }
  }
}

function samplePalette(palette, t) {
  const value = clamp(t) * (palette.length - 1);
  const index = Math.floor(value);
  const next = Math.min(palette.length - 1, index + 1);
  const amount = value - index;
  return [
    lerp(palette[index][0], palette[next][0], amount),
    lerp(palette[index][1], palette[next][1], amount),
    lerp(palette[index][2], palette[next][2], amount)
  ];
}

function pixelIndex(x, y, width) {
  return (y * width + x) * 4;
}

function pixelLuma(data, index) {
  return luma(data[index], data[index + 1], data[index + 2]);
}

function luma(r, g, b) {
  return r * 0.2126 + g * 0.7152 + b * 0.0722;
}

function clampByte(value) {
  return Math.max(0, Math.min(255, value)) | 0;
}

function copyRow(data, width, fromY, toY) {
  const from = pixelIndex(0, fromY, width);
  const to = pixelIndex(0, toY, width);
  data.copyWithin(to, from, from + width * 4);
}
