import { clonePreset, defaultPipeline, FALSE_COLOR_MODES } from "./presets.js";
import { clamp, createRng, randomInt, randomRange } from "./utils.js";

// Mode ids are kept for preset/UI compatibility. Each roll materializes one
// latent severity inside the selected band; every parameter in that roll then
// follows it. This prevents a supposedly gentle camera from combining a low
// main strength with full-strength secondary damage.
const MODE_PROFILES = {
  bent: { min: 0.06, max: 0.22, parameterMin: 0.03, parameterMax: 0.25 },
  damaged: { min: 0.38, max: 0.66, parameterMin: 0.3, parameterMax: 0.7 },
  shorted: { min: 0.78, max: 1, parameterMin: 0.72, parameterMax: 1 },
  explore: { min: 0, max: 1, parameterMin: 0, parameterMax: 1 }
};

// [key, label, tooltip description, button subtitle]
export const RANDOM_FAMILIES = [
  ["global", "New Random Camera", "Build a whole new camera: re-rolls all damage modules, name, and seed", "re-roll everything"],
  ["physics", "Physics", "Re-roll sensor circuitry only: IR filter, charge-transfer clock, analog front end, supply rail, data bus, master clock, and address bus", "circuit bends"],
  ["color", "Color", "Re-roll palette and channel damage only: false color, channel bends, gradient wash, and WB hunting", "palette & channels"],
  ["melt", "Melt", "Re-roll vertical charge smear and pixel-sort drips only", "smear & drips"],
  ["burn", "Burn", "Re-roll exposure clipping, contour rings, and edge fringes only", "clipping & rings"],
  ["noise", "Noise", "Re-roll sensor grain, striping, hot/dead pixels, and amp glow only", "grain & speckle"],
  ["cheap", "Cheap", "Re-roll resolution, bit depth, Bayer damage, blur, dither, and JPEG crunch only", "resolution & JPEG"],
  ["shift", "Shift", "Re-roll frame timing tears and RGB registration offsets only", "tears & offsets"],
  ["memory", "Corrupt", "Re-roll buffer and frame-memory corruption only: interlace, block shifts, repeated rows, and ghost frames", "memory faults"]
];

export const RANDOM_MODES = [
  ["bent", "Gentle (light)", "Light bending that preserves the original image structure"],
  ["damaged", "Broken (medium)", "Strong local distortion while the general structure remains"],
  ["shorted", "Wrecked (heavy)", "Abstract, destructive damage with little of the source left intact"],
  ["explore", "Explore (unbounded)", "One coherent roll drawn from the full gentle-to-wrecked range"]
];

// Every damage module has one family owner. That makes family randomizers true
// refinement tools: they can clear and rebuild their own domain without the
// macro mapper silently changing modules elsewhere.
export const RANDOM_FAMILY_MODULES = {
  physics: ["irCut", "ccdClock", "afeBend", "railSag", "busBend", "masterClock", "addressBus"],
  color: ["falseColor", "colorBend", "gradientWash", "awbSeizure"],
  melt: ["verticalSmear", "pixelSort"],
  burn: ["exposureFault", "contourRings", "edgeBurn"],
  noise: ["sensorNoise", "ampGlow"],
  cheap: ["cheapCamera", "dctCrunch", "bayerFault"],
  shift: ["syncFault", "chromaShift"],
  memory: ["memoryFault", "bufferGhost"]
};

const FAMILY_WEIGHTS = {
  physics: [1, 1.15, 1.05, 0.95, 1.15, 1, 1.05],
  color: [1.35, 1.15, 0.8, 0.7],
  melt: [1.35, 1],
  burn: [1.25, 0.9, 1.05],
  noise: [1.45, 0.7],
  cheap: [1.3, 1.1, 0.8],
  shift: [1.25, 1],
  memory: [1.3, 1]
};

// Physics modules compound in the raw domain, so three simultaneous circuits
// are already more destructive than a much larger post-process stack.
const FAMILY_MAX_ACTIVE = {
  physics: 3,
  color: 3,
  melt: 2,
  burn: 3,
  noise: 2,
  cheap: 3,
  shift: 2,
  memory: 2
};

const EDGE_PALETTE_NAMES = ["cyan", "magenta", "green", "black", "white", "red", "yellow"];
const FAMILY_TAGS = RANDOM_FAMILIES.flatMap(([family, label]) => [family.toUpperCase(), label.toUpperCase()]);
const FAMILY_LABELS = Object.fromEntries(RANDOM_FAMILIES.map(([family, label]) => [family, label]));

function resolveMode(modeName, rng) {
  const name = Object.hasOwn(MODE_PROFILES, modeName) ? modeName : "damaged";
  const profile = MODE_PROFILES[name];
  if (name === "explore") {
    const severity = rng();
    return {
      name,
      severity,
      parameterMin: 0,
      parameterMax: 1
    };
  }
  return {
    name,
    severity: randomRange(profile.min, profile.max, rng),
    parameterMin: profile.parameterMin,
    parameterMax: profile.parameterMax
  };
}

// Interpolate a parameter from its near-clean value to its destructive value.
// Jitter happens in severity-space, so related controls remain correlated.
function scaled(mode, rng, clean, destroyed, jitter = 0.1) {
  const t = clamp(
    mode.severity + randomRange(-jitter, jitter, rng),
    mode.parameterMin,
    mode.parameterMax
  );
  return clean + (destroyed - clean) * t;
}

function amount(mode, rng, clean = 0, destroyed = 1, jitter = 0.1) {
  return clamp(scaled(mode, rng, clean, destroyed, jitter));
}

function probability(mode, rng, cleanChance, destroyedChance) {
  return rng() < cleanChance + (destroyedChance - cleanChance) * mode.severity;
}

function countForSeverity(mode, rng, max, min = 1) {
  if (max <= min) return min;
  const exact = min + (max - min) * clamp(mode.severity + randomRange(-0.08, 0.08, rng));
  const whole = Math.floor(exact);
  return Math.min(max, whole + (rng() < exact - whole ? 1 : 0));
}

function shuffled(values, rng) {
  const result = [...values];
  for (let index = result.length - 1; index > 0; index -= 1) {
    const swap = randomInt(0, index, rng);
    [result[index], result[swap]] = [result[swap], result[index]];
  }
  return result;
}

function weightedSample(keys, weights, count, rng) {
  const pool = keys.map((key, index) => ({ key, weight: weights[index] ?? 1 }));
  const selected = [];
  while (pool.length && selected.length < count) {
    const total = pool.reduce((sum, item) => sum + item.weight, 0);
    let roll = rng() * total;
    let index = 0;
    for (; index < pool.length - 1; index += 1) {
      roll -= pool[index].weight;
      if (roll <= 0) break;
    }
    selected.push(pool.splice(index, 1)[0].key);
  }
  return selected;
}

function setWhiteBalance(config, mode, rng) {
  config.wbRed = scaled(mode, rng, 1.25, 2.55, 0.16);
  config.wbBlue = scaled(mode, rng, 1.12, 2.05, 0.16);
}

export const MODULE_RANDOMIZERS = {
  irCut(preset, mode, rng) {
    const ir = preset.pipeline.irCut;
    ir.enabled = true;
    ir.strength = amount(mode, rng, 0.1, 0.98);
    ir.spectrum = randomRange(0.15, 0.95, rng);
    ir.wood = scaled(mode, rng, 0.2, 0.95, 0.18);
    ir.haze = probability(mode, rng, 0.2, 0.82) ? amount(mode, rng, 0.02, 0.78) : 0;
    setWhiteBalance(ir, mode, rng);
  },

  ccdClock(preset, mode, rng) {
    const ccd = preset.pipeline.ccdClock;
    ccd.enabled = true;
    ccd.transferLoss = 0;
    ccd.vSkip = 0;
    ccd.hShear = 0;
    ccd.bloom = 0;
    const faults = shuffled(["transferLoss", "vSkip", "hShear", "bloom"], rng);
    const count = countForSeverity(mode, rng, 3);
    for (let index = 0; index < count; index += 1) {
      const key = faults[index];
      const ceiling = key === "bloom" ? 0.78 : key === "vSkip" ? 0.86 : 0.95;
      ccd[key] = amount(mode, rng, 0.035, ceiling);
    }
    setWhiteBalance(ccd, mode, rng);
  },

  afeBend(preset, mode, rng) {
    const afe = preset.pipeline.afeBend;
    afe.enabled = true;
    afe.wave = ["sine", "sine", "square", "saw", "noise"][randomInt(0, 4, rng)];
    afe.freq = randomRange(0.05, 0.95, rng);
    afe.skew = probability(mode, rng, 0.12, 0.72) ? scaled(mode, rng, 0.02, 0.42) * (rng() < 0.5 ? -1 : 1) : 0;
    afe.wobble = amount(mode, rng, 0.02, 0.8);
    afe.cdsSkew = randomRange(0.05, 0.9, rng);
    afe.inject = 0;
    afe.gainMod = 0;
    afe.cdsAmount = 0;
    const couplings = shuffled(["inject", "gainMod", "cdsAmount"], rng);
    const count = countForSeverity(mode, rng, 2);
    for (let index = 0; index < count; index += 1) {
      const key = couplings[index];
      const ceiling = key === "cdsAmount" ? 0.92 : 0.68;
      afe[key] = amount(mode, rng, 0.025, ceiling);
    }
    setWhiteBalance(afe, mode, rng);
  },

  railSag(preset, mode, rng) {
    const sag = preset.pipeline.railSag;
    sag.enabled = true;
    sag.sag = amount(mode, rng, 0.08, 0.95);
    sag.flicker = scaled(mode, rng, 0.08, 0.82, 0.2);
    sag.spikes = probability(mode, rng, 0.1, 0.75) ? amount(mode, rng, 0.02, 0.72) : 0;
    sag.failures = probability(mode, rng, 0.04, 0.88) ? amount(mode, rng, 0.015, 0.88) : 0;
    setWhiteBalance(sag, mode, rng);
  },

  busBend(preset, mode, rng) {
    const bend = preset.pipeline.busBend;
    const pickMask = (pool, count) => {
      const bits = shuffled(pool, rng).slice(0, count);
      return bits.reduce((mask, bit) => mask | (1 << bit), 0);
    };
    const sourcePool = [11, 10, 9, 8, 7, 6, 5, 4, 3, 2];
    const targetPool = [11, 10, 9, 8, 7, 6, 5, 4];
    const bitCount = countForSeverity(mode, rng, 4);
    bend.enabled = true;
    const fnRoll = rng();
    bend.fn = fnRoll < 0.48 ? "bypass" : fnRoll < 0.76 ? "invert" : "divide";
    if (bend.fn === "divide" && rng() < 0.78) {
      const mask = pickMask(targetPool, Math.max(2, bitCount));
      bend.sourceMask = mask;
      bend.targetMask = mask;
    } else {
      bend.sourceMask = pickMask(sourcePool, bitCount);
      bend.targetMask = pickMask(targetPool, Math.max(1, bitCount - (rng() < 0.45 ? 1 : 0)));
    }
    bend.targetGnd = probability(mode, rng, 0.02, 0.28);
    bend.commonBus = probability(mode, rng, 0.03, 0.34);
    bend.pot = randomRange(0.12, 0.92, rng);
    bend.resistance = probability(mode, rng, 0.85, 0.22) ? scaled(mode, rng, 0.82, 0.12, 0.16) : 0;
    bend.injectStrength = amount(mode, rng, 0.18, 0.88);
    bend.jitter = amount(mode, rng, 0.01, 0.28);
    setWhiteBalance(bend, mode, rng);
  },

  masterClock(preset, mode, rng) {
    const clock = preset.pipeline.masterClock;
    clock.enabled = true;
    const magnitude = scaled(mode, rng, 0.012, 0.42, 0.08);
    clock.detune = magnitude * (rng() < 0.5 ? -1 : 1);
    clock.drift = probability(mode, rng, 0.22, 0.86) ? amount(mode, rng, 0.02, 0.88) : 0;
    clock.hLock = scaled(mode, rng, 0.92, 0.18, 0.16);
    clock.shred = probability(mode, rng, 0.02, 0.78) ? amount(mode, rng, 0.01, 0.72) : 0;
    setWhiteBalance(clock, mode, rng);
  },

  addressBus(preset, mode, rng) {
    const memory = preset.pipeline.addressBus;
    memory.enabled = true;
    const rowLead = rng() < 0.62;
    const primary = amount(mode, rng, 0.06, 0.92);
    const secondary = probability(mode, rng, 0.04, 0.72) ? amount(mode, rng, 0.02, 0.72) : 0;
    memory.rows = rowLead ? primary : secondary;
    memory.cols = rowLead ? secondary : primary;
    memory.scale = randomRange(0.12, 0.92, rng);
    memory.lowBit = probability(mode, rng, 0.03, 0.65) ? amount(mode, rng, 0.02, 0.68) : 0;
    memory.duty = scaled(mode, rng, 0.28, 0.95, 0.2);
    setWhiteBalance(memory, mode, rng);
  },

  cheapCamera(preset, mode, rng) {
    const cheap = preset.pipeline.cheapCamera;
    cheap.enabled = true;
    cheap.internalScale = scaled(mode, rng, 0.96, 0.28, 0.08);
    cheap.blur = probability(mode, rng, 0.12, 0.7) ? amount(mode, rng, 0.02, 0.92) : 0;
    cheap.bitDepth = Math.round(scaled(mode, rng, 8, 3, 0.08));
    cheap.dither = amount(mode, rng, 0.04, 0.95);
    cheap.sharpen = amount(mode, rng, 0.08, 1);
  },

  colorBend(preset, mode, rng) {
    const bend = preset.pipeline.colorBend;
    bend.enabled = true;
    bend.hueRotate = 0;
    bend.channelMode = "none";
    bend.invert = "none";
    bend.solarize = 0;
    const operations = shuffled(["hue", "channel", "invert", "solarize"], rng);
    const count = countForSeverity(mode, rng, 4);
    for (let index = 0; index < count; index += 1) {
      const operation = operations[index];
      if (operation === "hue") {
        bend.hueRotate = randomInt(25, 335, rng);
        bend.hueStrength = amount(mode, rng, 0.08, 1);
      } else if (operation === "channel") {
        bend.channelMode = ["gbr", "brg", "grb", "bgr", "rbg"][randomInt(0, 4, rng)];
        bend.channelStrength = amount(mode, rng, 0.08, 1);
      } else if (operation === "invert") {
        bend.invert = ["red", "green", "blue", "all"][randomInt(0, 3, rng)];
        bend.invertStrength = amount(mode, rng, 0.06, 1);
      } else {
        bend.solarize = amount(mode, rng, 0.04, 0.95);
      }
    }
  },

  chromaShift(preset, mode, rng) {
    const chroma = preset.pipeline.chromaShift;
    chroma.enabled = true;
    chroma.amount = amount(mode, rng, 0.012, 0.72);
    chroma.angle = randomInt(0, 359, rng);
    chroma.wobble = probability(mode, rng, 0.15, 0.88) ? amount(mode, rng, 0.01, 0.9) : 0;
  },

  exposureFault(preset, mode, rng) {
    const exposure = preset.pipeline.exposureFault;
    const biases = [
      [1, 0.18, 0.86],
      [0.12, 0.95, 1],
      [1, 0.92, 0.18],
      [0.95, 0.08, 0.12],
      [0.72, 1, 0.18]
    ];
    exposure.enabled = true;
    exposure.gain = scaled(mode, rng, 1.02, 2.18, 0.1);
    exposure.blackCrush = amount(mode, rng, 0.015, 0.76);
    exposure.highlightClip = amount(mode, rng, 0.025, 0.98);
    exposure.contourBands = amount(mode, rng, 0.03, 0.95);
    exposure.fringing = probability(mode, rng, 0.08, 0.78) ? amount(mode, rng, 0.01, 0.88) : 0;
    exposure.clipColorBias = biases[randomInt(0, biases.length - 1, rng)];
  },

  awbSeizure(preset, mode, rng) {
    const awb = preset.pipeline.awbSeizure;
    awb.enabled = true;
    awb.wbSwing = amount(mode, rng, 0.035, 0.98);
    awb.aeSwing = probability(mode, rng, 0.18, 0.82) ? amount(mode, rng, 0.015, 0.76) : 0;
    awb.bandHeight = scaled(mode, rng, 0.62, 0.1, 0.2);
    awb.frequency = scaled(mode, rng, 0.18, 0.92, 0.2);
  },

  contourRings(preset, mode, rng) {
    const rings = preset.pipeline.contourRings;
    rings.enabled = true;
    rings.strength = amount(mode, rng, 0.035, 1);
    rings.scale = randomRange(0.2, 1, rng);
    rings.bandSharpness = scaled(mode, rng, 0.18, 1, 0.18);
    rings.tonalBias = randomRange(0.16, 0.92, rng);
    rings.colorBleed = amount(mode, rng, 0.03, 0.95);
  },

  falseColor(preset, mode, rng) {
    const color = preset.pipeline.falseColor;
    color.enabled = true;
    color.mode = FALSE_COLOR_MODES[randomInt(0, FALSE_COLOR_MODES.length - 1, rng)];
    color.strength = amount(mode, rng, 0.035, 1);
    color.posterizeLevels = Math.round(scaled(mode, rng, 13, 3, 0.1));
    color.smoothness = rng() < 0.5 ? randomRange(0, 0.25, rng) : randomRange(0.45, 0.95, rng);
    color.channelSwap = amount(mode, rng, 0, 0.72);
    color.hueWarp = amount(mode, rng, 0.02, 1);
    color.saturation = scaled(mode, rng, 1.05, 2.95, 0.12);
  },

  gradientWash(preset, mode, rng) {
    const wash = preset.pipeline.gradientWash;
    wash.enabled = true;
    wash.mode = FALSE_COLOR_MODES[randomInt(0, FALSE_COLOR_MODES.length - 1, rng)];
    wash.strength = amount(mode, rng, 0.04, 0.94);
    wash.angle = randomInt(0, 359, rng);
    wash.scale = randomRange(0.25, 1, rng);
    wash.keepLuma = scaled(mode, rng, 0.96, 0.32, 0.14);
    wash.wobble = amount(mode, rng, 0.02, 0.92);
  },

  pixelSort(preset, mode, rng) {
    const sort = preset.pipeline.pixelSort;
    sort.enabled = true;
    const reach = amount(mode, rng, 0.015, 1);
    // Pixel-sort strength selects whole columns rather than blending pixels;
    // ease its low end so Gentle produces isolated drips, not a barcode.
    sort.strength = reach ** 1.65;
    sort.threshold = randomRange(0.24, 0.76, rng);
    sort.window = scaled(mode, rng, 0.1, 0.9, 0.18);
    sort.direction = rng() > 0.72 ? "up" : "down";
    sort.mode = rng() > 0.78 ? "dark" : "bright";
    sort.maxRun = 0.03 + 0.97 * reach ** 1.45;
  },

  edgeBurn(preset, mode, rng) {
    const edge = preset.pipeline.edgeBurn;
    edge.enabled = true;
    edge.strength = amount(mode, rng, 0.025, 1);
    edge.threshold = scaled(mode, rng, 0.3, 0.045, 0.16);
    edge.darkOutline = amount(mode, rng, 0.015, 0.92);
    const count = countForSeverity(mode, rng, 5, 2);
    edge.palette = shuffled(EDGE_PALETTE_NAMES, rng).slice(0, count);
  },

  verticalSmear(preset, mode, rng) {
    const smear = preset.pipeline.verticalSmear;
    smear.enabled = true;
    const reach = amount(mode, rng, 0.015, 1);
    smear.strength = reach ** 1.35;
    smear.threshold = scaled(mode, rng, 0.78, 0.18, 0.14);
    smear.decay = scaled(mode, rng, 0.86, 0.995, 0.08);
    smear.length = 0.02 + 0.98 * reach ** 1.4;
    smear.spread = amount(mode, rng, 0.015, 0.75);
    smear.contrast = amount(mode, rng, 0.08, 1);
    const curtain = probability(mode, rng, 0.04, 0.82);
    smear.curtainStrength = curtain ? amount(mode, rng, 0.02, 1) : 0;
    smear.curtainDensity = curtain ? amount(mode, rng, 0.015, 0.92) : 0;
    smear.curtainDrop = curtain ? amount(mode, rng, 0.05, 1) : 0;
    smear.jitter = amount(mode, rng, 0.005, 0.88);
    smear.edgeBias = scaled(mode, rng, 0.16, 1, 0.16);
  },

  sensorNoise(preset, mode, rng) {
    const noise = preset.pipeline.sensorNoise;
    noise.enabled = true;
    noise.amount = amount(mode, rng, 0.012, 0.9);
    noise.colorAmount = scaled(mode, rng, 0.28, 1, 0.2);
    noise.shadowBias = randomRange(0.25, 0.92, rng);
    noise.striping = amount(mode, rng, 0.005, 0.82);
    noise.hotPixels = amount(mode, rng, 0.002, 0.52);
    noise.deadColumns = probability(mode, rng, 0.01, 0.72) ? amount(mode, rng, 0.005, 0.72) : 0;
    noise.deadClusters = probability(mode, rng, 0.01, 0.68) ? amount(mode, rng, 0.005, 0.62) : 0;
    noise.speckleSize = mode.severity > 0.72 && rng() < 0.45 ? randomInt(2, 4, rng) : 1;
  },

  ampGlow(preset, mode, rng) {
    const glow = preset.pipeline.ampGlow;
    glow.enabled = true;
    glow.strength = amount(mode, rng, 0.04, 0.96);
    glow.corner = "seeded";
    glow.hue = rng() > 0.5 ? randomRange(0, 0.35, rng) : randomRange(0.6, 1, rng);
    glow.spread = scaled(mode, rng, 0.28, 0.92, 0.18);
  },

  memoryFault(preset, mode, rng) {
    const memory = preset.pipeline.memoryFault;
    memory.enabled = true;
    memory.interlace = 0;
    memory.blockShift = 0;
    memory.rowRepeat = 0;
    memory.scanlineDropout = 0;
    const faults = shuffled(["interlace", "blockShift", "rowRepeat", "scanlineDropout"], rng);
    const ceilings = { interlace: 0.92, blockShift: 1, rowRepeat: 0.9, scanlineDropout: 0.78 };
    const count = countForSeverity(mode, rng, 4);
    for (let index = 0; index < count; index += 1) {
      const key = faults[index];
      memory[key] = amount(mode, rng, 0.02, ceilings[key]);
    }
  },

  dctCrunch(preset, mode, rng) {
    const dct = preset.pipeline.dctCrunch;
    dct.enabled = true;
    dct.quality = scaled(mode, rng, 0.94, 0.045, 0.08);
    dct.chromaSubsample = probability(mode, rng, 0.25, 0.9) ? amount(mode, rng, 0.04, 1) : 0;
    dct.dcDrift = 0;
    dct.acScramble = 0;
    dct.blockRepeat = 0;
    const faults = shuffled(["dcDrift", "acScramble", "blockRepeat"], rng);
    const count = mode.severity < 0.2 ? 0 : countForSeverity(mode, rng, 3, 0);
    const ceilings = { dcDrift: 0.92, acScramble: 0.82, blockRepeat: 0.68 };
    for (let index = 0; index < count; index += 1) {
      const key = faults[index];
      dct[key] = amount(mode, rng, 0.01, ceilings[key]);
    }
    dct.generations = mode.severity > 0.62 && rng() < mode.severity ? randomInt(2, 6, rng) : 1;
  },

  bayerFault(preset, mode, rng) {
    const bayer = preset.pipeline.bayerFault;
    bayer.enabled = true;
    bayer.phaseError = randomInt(1, 3, rng);
    bayer.strength = amount(mode, rng, 0.045, 1);
    bayer.zipper = amount(mode, rng, 0.015, 0.95);
  },

  bufferGhost(preset, mode, rng) {
    const ghost = preset.pipeline.bufferGhost;
    ghost.enabled = true;
    ghost.amount = amount(mode, rng, 0.025, 0.96);
    ghost.blockSize = scaled(mode, rng, 0.18, 0.86, 0.22);
    ghost.ghostShift = amount(mode, rng, 0.02, 0.92);
    ghost.ghostZoom = probability(mode, rng, 0.08, 0.76) ? amount(mode, rng, 0.01, 0.82) : 0;
    ghost.fieldMode = probability(mode, rng, 0.04, 0.62);
  },

  syncFault(preset, mode, rng) {
    const sync = preset.pipeline.syncFault;
    sync.enabled = true;
    sync.tearCount = amount(mode, rng, 0.025, 1);
    sync.tearShift = amount(mode, rng, 0.025, 0.94);
    sync.wobbleAmount = probability(mode, rng, 0.15, 0.9) ? amount(mode, rng, 0.01, 0.84) : 0;
    sync.wobbleFrequency = randomRange(0.12, 0.9, rng);
    sync.drift = amount(mode, rng, 0.02, 0.92);
  },

  osdOverlay(preset, mode, rng) {
    const osd = preset.pipeline.osdOverlay;
    osd.enabled = true;
    osd.datestamp = rng() > 0.12;
    osd.hudIcons = rng() > 0.45;
    osd.glitchText = probability(mode, rng, 0.08, 0.82) ? amount(mode, rng, 0.01, 0.9) : 0;
    osd.scale = randomRange(0.3, 0.7, rng);
    osd.color = ["orange", "orange", "green", "white"][randomInt(0, 3, rng)];
  },

  basicAdjustments(preset, mode, rng) {
    const adjust = preset.pipeline.basicAdjustments;
    const reach = 0.25 + mode.severity * 0.75;
    adjust.enabled = true;
    adjust.brightness = randomRange(-0.22, 0.22, rng) * reach;
    adjust.contrast = randomRange(-0.28, 0.42, rng) * reach;
    adjust.saturation = randomRange(-0.32, 0.42, rng) * reach;
    adjust.vibrance = randomRange(-0.22, 0.45, rng) * reach;
    adjust.temperature = randomRange(-0.28, 0.28, rng) * reach;
    adjust.tint = randomRange(-0.18, 0.18, rng) * reach;
    adjust.gamma = randomRange(1 - 0.22 * reach, 1 + 0.22 * reach, rng);
    adjust.shadows = randomRange(-0.3, 0.34, rng) * reach;
    adjust.highlights = randomRange(-0.34, 0.28, rng) * reach;
  }
};

export function randomizeModule(currentPreset, moduleKey, modeName = "damaged", options = {}) {
  const randomizer = MODULE_RANDOMIZERS[moduleKey];
  if (!randomizer) return currentPreset;
  const preset = clonePreset(currentPreset);
  const randomSeed = Number.isFinite(options.randomSeed)
    ? options.randomSeed
    : Math.floor(Math.random() * 2147483647);
  const rng = createRng(randomSeed);
  randomizer(preset, resolveMode(modeName, rng), rng);
  return preset;
}

export function randomizePreset(currentPreset, family = "global", modeName = "damaged", options = {}) {
  if (family !== "global" && !RANDOM_FAMILY_MODULES[family]) return currentPreset;
  const preset = clonePreset(currentPreset);
  const randomSeed = Number.isFinite(options.randomSeed)
    ? options.randomSeed
    : Math.floor(Math.random() * 2147483647);
  // A refinement roll needs fresh parameter choices without moving seeded
  // damage in every unrelated module. Only a whole new camera adopts the roll
  // seed as its render seed.
  if (family === "global") preset.seed = randomSeed;
  const rng = createRng(randomSeed);
  const mode = resolveMode(modeName, rng);

  if (family === "global") randomizeGlobal(preset, mode, rng);
  else rollFamily(preset, family, mode, rng, { clear: true });

  preset.name = family === "global" ? randomCameraName(rng) : tagFamilyName(preset.name, family);
  preset.cameraModel = preset.name;
  preset.description = "";
  preset.createdAt = new Date().toISOString();
  return preset;
}

function tagFamilyName(name, family) {
  const parts = String(name).trim().split(/\s+/);
  while (parts.length > 1 && FAMILY_TAGS.includes(parts[parts.length - 1])) parts.pop();
  return `${parts.join(" ")} ${(FAMILY_LABELS[family] || family).toUpperCase()}`;
}

const ARCHETYPES = [
  { core: ["color", "burn"], guests: ["melt", "noise", "cheap", "shift", "memory", "physics"] },
  { core: ["melt", "color"], guests: ["burn", "shift", "noise", "memory", "cheap", "physics"] },
  { core: ["physics", "noise"], guests: ["shift", "color", "burn", "cheap", "memory", "melt"] },
  { core: ["cheap", "noise"], guests: ["color", "memory", "shift", "burn", "physics", "melt"] },
  { core: ["shift", "memory"], guests: ["color", "melt", "cheap", "noise", "physics", "burn"] },
  { core: ["burn", "melt"], guests: ["color", "noise", "shift", "cheap", "memory", "physics"] }
];

function randomizeGlobal(preset, mode, rng) {
  const pipeline = preset.pipeline;
  const defaults = defaultPipeline();
  for (const modules of Object.values(RANDOM_FAMILY_MODULES)) {
    for (const key of modules) {
      pipeline[key] = defaults[key];
      pipeline[key].enabled = false;
    }
  }
  // OSD is set dressing rather than damage, but it should not leak from the
  // previous camera into a whole-camera roll. Classic adjustments are kept as
  // the user's finishing grade.
  pipeline.osdOverlay = defaults.osdOverlay;

  const archetype = ARCHETYPES[randomInt(0, ARCHETYPES.length - 1, rng)];
  let target;
  if (mode.name === "bent") target = randomInt(1, 2, rng);
  else if (mode.name === "damaged") target = randomInt(3, 4, rng);
  else if (mode.name === "shorted") target = randomInt(6, 7, rng);
  else target = clamp(Math.round(1 + mode.severity * 7 + randomRange(-0.45, 0.45, rng)), 1, 8);
  const families = [];
  for (const family of archetype.core) {
    if (families.length < target) families.push(family);
  }
  const remaining = shuffled(archetype.guests.filter((family) => !families.includes(family)), rng);
  while (families.length < target && remaining.length) families.push(remaining.shift());

  for (const family of families) {
    const max = RANDOM_FAMILY_MODULES[family].length;
    const globalReach = clamp((mode.severity - 0.28) / 0.72);
    const desired = 1 + globalReach * (Math.min(max, 3) - 1) * 0.72;
    const base = Math.floor(desired);
    const count = Math.min(max, base + (rng() < desired - base ? 1 : 0));
    rollFamily(preset, family, mode, rng, { clear: false, count });
  }
}

function rollFamily(preset, family, mode, rng, { clear = true, count = null } = {}) {
  const keys = RANDOM_FAMILY_MODULES[family];
  if (!keys) return;
  if (clear) {
    for (const key of keys) preset.pipeline[key].enabled = false;
  }
  const moduleCount = count ?? countForSeverity(mode, rng, FAMILY_MAX_ACTIVE[family]);
  const selected = weightedSample(keys, FAMILY_WEIGHTS[family], moduleCount, rng);
  for (const key of selected) MODULE_RANDOMIZERS[key](preset, mode, rng);
}

function randomCameraName(rng) {
  const prefixes = ["Bent", "Dead", "Shorted", "Overheat", "Menu", "Flash", "CCD"];
  const cores = ["CCD", "Pocket", "Digicam", "Sensor", "Compact", "Memory"];
  const suffix = String(randomInt(1, 99, rng)).padStart(2, "0");
  return `${prefixes[randomInt(0, prefixes.length - 1, rng)]} ${cores[randomInt(0, cores.length - 1, rng)]}-${suffix}`;
}
