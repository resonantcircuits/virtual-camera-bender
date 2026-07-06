export function clamp(value, min = 0, max = 1) {
  return Math.min(max, Math.max(min, value));
}

export function lerp(a, b, amount) {
  return a + (b - a) * amount;
}

export function smoothstep(edge0, edge1, value) {
  const t = clamp((value - edge0) / (edge1 - edge0));
  return t * t * (3 - 2 * t);
}

export function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

export function mergeDeep(target, source) {
  for (const key of Object.keys(source)) {
    const sourceValue = source[key];
    if (
      sourceValue &&
      typeof sourceValue === "object" &&
      !Array.isArray(sourceValue)
    ) {
      if (!target[key] || typeof target[key] !== "object") {
        target[key] = {};
      }
      mergeDeep(target[key], sourceValue);
    } else {
      target[key] = sourceValue;
    }
  }
  return target;
}

export function getAtPath(object, path) {
  return path.split(".").reduce((value, key) => value?.[key], object);
}

export function setAtPath(object, path, nextValue) {
  const keys = path.split(".");
  const last = keys.pop();
  const target = keys.reduce((value, key) => value[key], object);
  target[last] = nextValue;
}

export function createRng(seed) {
  let state = seed >>> 0;
  return function rng() {
    state += 0x6d2b79f5;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function randomInt(min, max, rng = Math.random) {
  return Math.floor(lerp(min, max + 1, rng()));
}

export function randomRange(min, max, rng = Math.random) {
  return lerp(min, max, rng());
}

export function hashUnit(x, y, seed = 1) {
  let n =
    Math.imul(x | 0, 374761393) ^
    Math.imul(y | 0, 668265263) ^
    Math.imul(seed | 0, 224682251);
  n = Math.imul(n ^ (n >>> 13), 1274126177);
  return ((n ^ (n >>> 16)) >>> 0) / 4294967295;
}

export function hashSigned(x, y, seed = 1) {
  return hashUnit(x, y, seed) * 2 - 1;
}

export function fitWithin(width, height, maxDimension) {
  if (!maxDimension || Math.max(width, height) <= maxDimension) {
    return { width, height, scale: 1 };
  }
  const scale = maxDimension / Math.max(width, height);
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
    scale
  };
}

export function downloadBlob(blob, filename) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export async function createBlobWriter(filename, mime, extension) {
  if ("showSaveFilePicker" in window) {
    try {
      const handle = await window.showSaveFilePicker({
        suggestedName: filename,
        types: [
          {
            description: "Image",
            accept: { [mime]: [extension] }
          }
        ]
      });
      return async (blob) => {
        const writable = await handle.createWritable();
        await writable.write(blob);
        await writable.close();
      };
    } catch (error) {
      if (error?.name === "AbortError") return null;
      console.warn("Save picker failed, falling back to browser download", error);
    }
  }

  const requestedName = window.prompt("Export filename", filename);
  if (requestedName === null) return null;
  const fallbackName = requestedName.trim() || filename;
  return async (blob) => downloadBlob(blob, fallbackName);
}

export function safeFilename(value) {
  return String(value || "virtual-camera-bender")
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 80) || "virtual-camera-bender";
}

export function fract(value) {
  return value - Math.floor(value);
}

export function noise2D(x, y, seed = 1) {
  const xf = Math.floor(x);
  const yf = Math.floor(y);
  const xs = fract(x);
  const ys = fract(y);

  const u = xs * xs * (3 - 2 * xs);
  const v = ys * ys * (3 - 2 * ys);

  const n00 = hashUnit(xf, yf, seed);
  const n10 = hashUnit(xf + 1, yf, seed);
  const n01 = hashUnit(xf, yf + 1, seed);
  const n11 = hashUnit(xf + 1, yf + 1, seed);

  return lerp(lerp(n00, n10, u), lerp(n01, n11, u), v);
}

export function fbmNoise(x, y, seed = 1) {
  let value = 0;
  let amplitude = 0.5;
  let frequency = 1.0;
  for (let i = 0; i < 4; i++) {
    value += amplitude * noise2D(x * frequency, y * frequency, seed);
    frequency *= 2.0;
    amplitude *= 0.5;
  }
  return value;
}
