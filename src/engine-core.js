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
  applyFinalCrunch(image, pipeline);
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
