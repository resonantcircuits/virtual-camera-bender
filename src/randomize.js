import { applyMacrosToPipeline, clonePreset, FALSE_COLOR_MODES } from "./presets.js";
import { clamp, createRng, randomInt, randomRange } from "./utils.js";

const MODES = {
  bent: { min: 0.08, max: 0.5, chaos: 0.35 },
  damaged: { min: 0.22, max: 0.78, chaos: 0.6 },
  shorted: { min: 0.45, max: 1, chaos: 0.88 },
  explore: { min: 0, max: 1, chaos: 1 }
};

export const RANDOM_FAMILIES = [
  ["global", "Global", "Build a whole new camera: re-rolls all macros and modules, new name and seed"],
  ["physics", "Physics", "Re-roll the physics rail only: analog front end and data-bus bend circuits — keeps the rest"],
  ["color", "Color", "Re-roll color only: palette, hue/channel bends, gradient wash, WB hunting — keeps the rest"],
  ["melt", "Melt", "Re-roll smear and pixel-sort drips — keeps the rest"],
  ["burn", "Burn", "Re-roll exposure clipping, contour rings, and edge fringes — keeps the rest"],
  ["noise", "Noise", "Re-roll sensor noise, speckle, dead pixels, and amp glow — keeps the rest"],
  ["cheap", "Cheap", "Re-roll resolution, bit depth, blur, dither, and JPEG crunch — keeps the rest"],
  ["memory", "Memory", "Re-roll interlace, block shifts, sync tears, ghost frames, and scanline faults — keeps the rest"]
];

export const RANDOM_MODES = [
  ["bent", "Bent (light)", "Gentle: subtle damage, image stays readable"],
  ["damaged", "Damaged (medium)", "Medium: clearly broken but structured"],
  ["shorted", "Shorted (heavy)", "Violent: heavy damage, high intensity"],
  ["explore", "Explore (unbounded)", "Anything goes: full range from clean to destroyed"]
];

const EDGE_PALETTE_NAMES = ["cyan", "magenta", "green", "black", "white", "red", "yellow"];
const FAMILY_TAGS = RANDOM_FAMILIES.map(([family]) => family.toUpperCase());

// Fresh draw inside the mode's intensity band.
function value(mode, rng, bias = 0) {
  return clamp(randomRange(mode.min, mode.max, rng) + bias);
}

// Symmetric wander for secondary macros — never a one-way ratchet.
function drift(macros, key, span, rng) {
  macros[key] = clamp(macros[key] + randomRange(-span, span, rng));
}

// Range whose ceiling scales with the mode, so "bent" stays gentle.
function intensity(mode, rng, low, high) {
  return randomRange(low, low + (high - low) * clamp(mode.max, 0.35, 1), rng);
}

export const MODULE_RANDOMIZERS = {
  // The physics modules (afeBend, busBend) never join the stylized-only
  // global rolls as background guests — they lead a build or sit it out
  // (see randomizeGlobal), and have their own Physics family button.
  afeBend(preset, mode, rng) {
    const afe = preset.pipeline.afeBend;
    afe.enabled = true;
    afe.wave = ["sine", "sine", "square", "saw", "noise"][randomInt(0, 4, rng)];
    afe.freq = randomRange(0.05, 0.95, rng);
    afe.skew = rng() > 0.55 ? randomRange(-0.35, 0.35, rng) : 0;
    afe.wobble = randomRange(0, 0.45, rng);
    afe.cdsSkew = randomRange(0.05, 0.8, rng);
    // One coupling leads each roll; stacking all three reliably turns to mush.
    const coupling = rng();
    afe.inject = coupling < 0.6 ? intensity(mode, rng, 0.12, 0.6) : 0;
    afe.gainMod = coupling >= 0.6 && coupling < 0.8 ? intensity(mode, rng, 0.15, 0.65) : 0;
    afe.cdsAmount = coupling >= 0.8 ? intensity(mode, rng, 0.3, 0.9) : 0;
    // Occasionally a light secondary coupling rides along.
    if (afe.inject > 0 && rng() < 0.25) afe.gainMod = intensity(mode, rng, 0.08, 0.3);
    else if (afe.inject === 0 && rng() < 0.25) afe.inject = intensity(mode, rng, 0.08, 0.25);
    afe.wbRed = randomRange(1.6, 2.4, rng);
    afe.wbBlue = randomRange(1.2, 1.9, rng);
  },
  busBend(preset, mode, rng) {
    const bend = preset.pipeline.busBend;
    const pick = (pool, count) => {
      const bits = [...pool];
      let mask = 0;
      for (let i = 0; i < count && bits.length; i += 1) {
        mask |= 1 << bits.splice(randomInt(0, bits.length - 1, rng), 1)[0];
      }
      return mask;
    };
    const srcPool = [11, 10, 9, 8, 7, 6, 5, 4, 3, 2];
    const tgtPool = [11, 10, 9, 8, 7, 6, 5, 4];
    bend.enabled = true;
    const fnRoll = rng();
    bend.fn = fnRoll < 0.45 ? "bypass" : fnRoll < 0.72 ? "invert" : "divide";
    const reach = Math.max(1, Math.round(1 + mode.max * 2.6));
    if (bend.fn === "divide" && rng() < 0.75) {
      // Divide only sings when source and target pins overlap (the flip-flop
      // clocks itself off the pins it corrupts), so bias hard toward that.
      const mask = pick(tgtPool, randomInt(2, Math.max(2, reach + 1), rng));
      bend.sourceMask = mask;
      bend.targetMask = mask;
    } else {
      bend.sourceMask = pick(srcPool, randomInt(1, reach, rng));
      bend.targetMask = pick(tgtPool, randomInt(1, reach, rng));
    }
    bend.targetGnd = rng() < 0.15;
    bend.pot = randomRange(0.08, 0.95, rng);
    bend.injectStrength = randomRange(0.35, 0.8, rng);
    bend.jitter = randomRange(0.03, 0.2, rng);
    bend.wbRed = randomRange(1.6, 2.4, rng);
    bend.wbBlue = randomRange(1.2, 1.9, rng);
  },
  cheapCamera(preset, mode, rng) {
    const cheap = preset.pipeline.cheapCamera;
    cheap.enabled = true;
    cheap.internalScale = randomRange(clamp(1 - mode.max * 0.62, 0.32, 0.9), 0.95, rng);
    cheap.blur = rng() > 0.62 ? intensity(mode, rng, 0.1, 0.8) : 0;
    cheap.bitDepth = randomInt(Math.max(3, Math.round(8 - mode.max * 5)), 8, rng);
    cheap.dither = randomRange(0.1, 0.95, rng);
    cheap.sharpen = intensity(mode, rng, 0.1, 1);
  },
  colorBend(preset, mode, rng) {
    const bend = preset.pipeline.colorBend;
    const channelModes = ["none", "none", "gbr", "brg", "grb", "bgr", "rbg"];
    const invertModes = ["none", "none", "none", "red", "green", "blue", "all"];
    bend.enabled = true;
    bend.hueRotate = rng() > 0.3 ? randomInt(0, 359, rng) : 0;
    bend.hueStrength = randomRange(0.5, 1, rng);
    bend.channelMode = channelModes[randomInt(0, channelModes.length - 1, rng)];
    bend.channelStrength = randomRange(0.5, 1, rng);
    bend.invert = invertModes[randomInt(0, invertModes.length - 1, rng)];
    bend.invertStrength = randomRange(0.6, 1, rng);
    bend.solarize = rng() > 0.65 ? intensity(mode, rng, 0.1, 0.8) : 0;
    if (bend.hueRotate === 0 && bend.channelMode === "none" && bend.invert === "none" && bend.solarize === 0) {
      bend.hueRotate = randomInt(30, 330, rng);
    }
  },
  chromaShift(preset, mode, rng) {
    const chroma = preset.pipeline.chromaShift;
    chroma.enabled = true;
    chroma.amount = intensity(mode, rng, 0.06, 0.6);
    chroma.angle = randomInt(0, 359, rng);
    chroma.wobble = randomRange(0, 0.7, rng);
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
    exposure.gain = 1 + intensity(mode, rng, 0.05, 1.1);
    exposure.blackCrush = intensity(mode, rng, 0.05, 0.6);
    exposure.highlightClip = value(mode, rng, 0.1);
    exposure.contourBands = randomRange(0.18, 0.94, rng);
    exposure.fringing = rng() > 0.6 ? intensity(mode, rng, 0.12, 0.8) : 0;
    exposure.clipColorBias = biases[randomInt(0, biases.length - 1, rng)];
  },
  awbSeizure(preset, mode, rng) {
    const awb = preset.pipeline.awbSeizure;
    awb.enabled = true;
    awb.wbSwing = intensity(mode, rng, 0.25, 0.95);
    awb.aeSwing = rng() > 0.35 ? intensity(mode, rng, 0.1, 0.7) : 0;
    awb.bandHeight = randomRange(0.15, 0.7, rng);
    awb.frequency = randomRange(0.2, 0.85, rng);
  },
  contourRings(preset, mode, rng) {
    const rings = preset.pipeline.contourRings;
    rings.enabled = true;
    rings.strength = value(mode, rng, 0.05);
    rings.scale = randomRange(0.25, 1, rng);
    rings.bandSharpness = randomRange(0.35, 1, rng);
    rings.tonalBias = randomRange(0.18, 0.9, rng);
    rings.colorBleed = randomRange(0.22, 0.9, rng);
  },
  falseColor(preset, mode, rng) {
    const falseColor = preset.pipeline.falseColor;
    falseColor.enabled = true;
    falseColor.mode = FALSE_COLOR_MODES[randomInt(0, FALSE_COLOR_MODES.length - 1, rng)];
    falseColor.strength = value(mode, rng, 0.15);
    falseColor.posterizeLevels = randomInt(4, 10, rng);
    falseColor.smoothness = rng() > 0.45 ? randomRange(0.4, 0.95, rng) : randomRange(0, 0.25, rng);
    falseColor.channelSwap = randomRange(0, 0.5, rng);
    falseColor.hueWarp = randomRange(0.05, 1, rng);
    falseColor.saturation = 1.2 + intensity(mode, rng, 0.1, 1.7);
  },
  gradientWash(preset, mode, rng) {
    const wash = preset.pipeline.gradientWash;
    wash.enabled = true;
    wash.mode = FALSE_COLOR_MODES[randomInt(0, FALSE_COLOR_MODES.length - 1, rng)];
    wash.strength = intensity(mode, rng, 0.25, 0.85);
    wash.angle = randomInt(0, 359, rng);
    wash.scale = randomRange(0.35, 1, rng);
    wash.keepLuma = randomRange(0.55, 0.95, rng);
    wash.wobble = randomRange(0.1, 0.8, rng);
  },
  pixelSort(preset, mode, rng) {
    const sort = preset.pipeline.pixelSort;
    sort.enabled = true;
    sort.strength = value(mode, rng, 0.1);
    sort.threshold = randomRange(0.28, 0.72, rng);
    sort.window = randomRange(0.2, 0.7, rng);
    sort.direction = rng() > 0.75 ? "up" : "down";
    sort.mode = rng() > 0.8 ? "dark" : "bright";
    sort.maxRun = intensity(mode, rng, 0.25, 0.95);
  },
  edgeBurn(preset, mode, rng) {
    const edge = preset.pipeline.edgeBurn;
    edge.enabled = true;
    edge.strength = value(mode, rng, 0.05);
    edge.threshold = randomRange(0.06, 0.3, rng);
    edge.darkOutline = randomRange(0, 0.8, rng);
    const count = randomInt(2, 4, rng);
    const pool = [...EDGE_PALETTE_NAMES];
    edge.palette = [];
    for (let i = 0; i < count; i += 1) {
      edge.palette.push(pool.splice(randomInt(0, pool.length - 1, rng), 1)[0]);
    }
  },
  verticalSmear(preset, mode, rng) {
    const smear = preset.pipeline.verticalSmear;
    smear.enabled = true;
    smear.strength = value(mode, rng, 0.1);
    smear.threshold = randomRange(0.25, 0.7, rng);
    smear.decay = randomRange(0.88, 0.994, rng);
    smear.length = intensity(mode, rng, 0.25, 1);
    smear.spread = randomRange(0.08, 0.62, rng);
    smear.contrast = randomRange(0.35, 1, rng);
    smear.curtainStrength = intensity(mode, rng, 0.15, 1);
    smear.curtainDensity = intensity(mode, rng, 0.15, 0.88);
    smear.curtainDrop = randomRange(0.3, 1, rng);
    smear.jitter = randomRange(0.02, 0.78, rng);
    smear.edgeBias = randomRange(0.35, 1, rng);
  },
  sensorNoise(preset, mode, rng) {
    const noise = preset.pipeline.sensorNoise;
    noise.enabled = true;
    noise.amount = value(mode, rng, -0.05);
    noise.colorAmount = randomRange(0.42, 1, rng);
    noise.shadowBias = randomRange(0.2, 0.9, rng);
    noise.striping = intensity(mode, rng, 0.02, 0.72);
    noise.hotPixels = intensity(mode, rng, 0.02, 0.42);
    noise.deadColumns = rng() > 0.68 ? intensity(mode, rng, 0.08, 0.6) : 0;
    noise.deadClusters = rng() > 0.72 ? intensity(mode, rng, 0.08, 0.5) : 0;
  },
  ampGlow(preset, mode, rng) {
    const glow = preset.pipeline.ampGlow;
    glow.enabled = true;
    glow.strength = intensity(mode, rng, 0.2, 0.9);
    glow.corner = "seeded";
    glow.hue = rng() > 0.5 ? randomRange(0, 0.35, rng) : randomRange(0.6, 1, rng);
    glow.spread = randomRange(0.25, 0.85, rng);
  },
  memoryFault(preset, mode, rng) {
    const memory = preset.pipeline.memoryFault;
    memory.enabled = true;
    memory.interlace = clamp(randomRange(0.08, mode.max * 0.9, rng));
    memory.blockShift = clamp(randomRange(0.1, mode.max, rng));
    memory.rowRepeat = clamp(randomRange(0.06, mode.max * 0.82, rng));
    memory.scanlineDropout = clamp(randomRange(0.04, mode.max * 0.68, rng));
  },
  dctCrunch(preset, mode, rng) {
    const dct = preset.pipeline.dctCrunch;
    dct.enabled = true;
    dct.quality = clamp(1 - intensity(mode, rng, 0.2, 0.95), 0.05, 0.9);
    dct.chromaSubsample = rng() > 0.3 ? randomRange(0.3, 1, rng) : 0;
    dct.dcDrift = rng() > 0.5 ? intensity(mode, rng, 0.1, 0.85) : 0;
    dct.acScramble = rng() > 0.45 ? intensity(mode, rng, 0.05, 0.7) : 0;
    dct.blockRepeat = rng() > 0.6 ? intensity(mode, rng, 0.05, 0.5) : 0;
    dct.generations = rng() > 0.75 ? randomInt(2, 5, rng) : 1;
  },
  bayerFault(preset, mode, rng) {
    const bayer = preset.pipeline.bayerFault;
    bayer.enabled = true;
    bayer.phaseError = randomInt(1, 3, rng);
    bayer.strength = intensity(mode, rng, 0.3, 1);
    bayer.zipper = randomRange(0.1, 0.8, rng);
  },
  bufferGhost(preset, mode, rng) {
    const ghost = preset.pipeline.bufferGhost;
    ghost.enabled = true;
    ghost.amount = intensity(mode, rng, 0.2, 0.9);
    ghost.blockSize = randomRange(0.15, 0.8, rng);
    ghost.ghostShift = randomRange(0.1, 0.8, rng);
    ghost.ghostZoom = rng() > 0.5 ? randomRange(0.05, 0.7, rng) : 0;
    ghost.fieldMode = rng() > 0.72;
  },
  syncFault(preset, mode, rng) {
    const sync = preset.pipeline.syncFault;
    sync.enabled = true;
    sync.tearCount = intensity(mode, rng, 0.2, 1);
    sync.tearShift = randomRange(0.15, 0.85, rng);
    sync.wobbleAmount = rng() > 0.35 ? intensity(mode, rng, 0.08, 0.7) : 0;
    sync.wobbleFrequency = randomRange(0.15, 0.85, rng);
    sync.drift = randomRange(0.1, 0.8, rng);
  },
  osdOverlay(preset, mode, rng) {
    const osd = preset.pipeline.osdOverlay;
    osd.enabled = true;
    osd.datestamp = rng() > 0.12;
    osd.hudIcons = rng() > 0.45;
    osd.glitchText = rng() > 0.4 ? intensity(mode, rng, 0.05, 0.8) : 0;
    osd.scale = randomRange(0.3, 0.7, rng);
    osd.color = ["orange", "orange", "green", "white"][randomInt(0, 3, rng)];
  }
};

export function randomizeModule(currentPreset, moduleKey, modeName = "damaged") {
  const randomizer = MODULE_RANDOMIZERS[moduleKey];
  if (!randomizer) return currentPreset;
  const preset = clonePreset(currentPreset);
  const mode = MODES[modeName] || MODES.damaged;
  const rng = createRng(Math.floor(Math.random() * 2147483647));
  randomizer(preset, mode, rng);
  return preset;
}

export function randomizePreset(currentPreset, family = "global", modeName = "damaged") {
  const preset = clonePreset(currentPreset);
  const mode = MODES[modeName] || MODES.damaged;
  preset.seed = Math.floor(Math.random() * 2147483647);
  const rng = createRng(preset.seed);

  if (family === "global") {
    randomizeGlobal(preset, mode, rng);
  } else if (family === "physics") {
    randomizePhysics(preset, mode, rng);
  } else if (family === "color") {
    randomizeColor(preset, mode, rng);
  } else if (family === "melt") {
    randomizeMelt(preset, mode, rng);
  } else if (family === "burn") {
    randomizeBurn(preset, mode, rng);
  } else if (family === "noise") {
    randomizeNoise(preset, mode, rng);
  } else if (family === "cheap") {
    randomizeCheap(preset, mode, rng);
  } else if (family === "memory") {
    randomizeMemory(preset, mode, rng);
  }

  preset.name = family === "global" ? randomCameraName(rng) : tagFamilyName(preset.name, family);
  preset.cameraModel = preset.name;
  preset.createdAt = new Date().toISOString();
  return preset;
}

// "Bent CCD-03 COLOR" stays "Bent CCD-03 MELT" on the next press —
// family tags replace each other instead of piling up.
function tagFamilyName(name, family) {
  const parts = String(name).trim().split(/\s+/);
  while (parts.length > 1 && FAMILY_TAGS.includes(parts[parts.length - 1])) {
    parts.pop();
  }
  return `${parts.join(" ")} ${family.toUpperCase()}`;
}

function randomizeGlobal(preset, mode, rng) {
  // A new camera sometimes ships with a bent circuit inside: most builds are
  // stylized-only, some are led by the physics rail (with the stylized damage
  // pulled back so the circuit look reads), and a few stack both.
  const physicsRoll = rng();
  const physicsLed = physicsRoll < 0.25;
  const fullStack = !physicsLed && physicsRoll < 0.35;

  preset.macros = {
    bend: value(mode, rng, 0.06),
    colorFault: value(mode, rng, 0.1),
    melt: value(mode, rng, -0.04),
    burn: value(mode, rng, 0.04),
    noise: value(mode, rng, -0.08),
    cheapness: value(mode, rng, -0.14),
    chaos: clamp(randomRange(mode.min * 0.3, mode.chaos, rng))
  };

  if (physicsLed) {
    preset.macros.bend *= 0.3;
    preset.macros.colorFault *= 0.25;
    preset.macros.melt *= 0.25;
    preset.macros.burn *= 0.3;
    preset.macros.chaos *= 0.3;
    preset.macros.noise *= 0.6;
    preset.macros.cheapness *= 0.7;
  }

  // Mute a couple of damage channels so each roll has its own character
  // instead of every module firing at once.
  const muteKeys = ["colorFault", "melt", "burn", "noise", "cheapness", "chaos"];
  const mutes = randomInt(1, 3, rng);
  for (let i = 0; i < mutes; i += 1) {
    const key = muteKeys[randomInt(0, muteKeys.length - 1, rng)];
    preset.macros[key] *= randomRange(0, 0.3, rng);
  }

  applyMacrosToPipeline(preset);

  // Re-roll flavor only for modules the macros actually enabled.
  const pipeline = preset.pipeline;
  [
    "cheapCamera",
    "exposureFault",
    "contourRings",
    "falseColor",
    "edgeBurn",
    "pixelSort",
    "verticalSmear",
    "sensorNoise",
    "memoryFault",
    "dctCrunch",
    "syncFault",
    "bufferGhost"
  ].forEach((key) => {
    if (pipeline[key].enabled) MODULE_RANDOMIZERS[key](preset, mode, rng);
  });

  // Character modules are rare guests, not permanent residents.
  // osdOverlay is deliberately untouched: it only turns on by hand.
  pipeline.colorBend.enabled = false;
  pipeline.gradientWash.enabled = false;
  pipeline.bayerFault.enabled = false;
  pipeline.ampGlow.enabled = false;
  pipeline.awbSeizure.enabled = false;
  if (rng() < 0.35) MODULE_RANDOMIZERS.colorBend(preset, mode, rng);
  if (rng() < 0.22) MODULE_RANDOMIZERS.gradientWash(preset, mode, rng);
  if (rng() < 0.15) MODULE_RANDOMIZERS.bayerFault(preset, mode, rng);
  if (rng() < 0.16) MODULE_RANDOMIZERS.ampGlow(preset, mode, rng);
  if (rng() < 0.14) MODULE_RANDOMIZERS.awbSeizure(preset, mode, rng);
  if (rng() < 0.18 + mode.chaos * 0.2) MODULE_RANDOMIZERS.chromaShift(preset, mode, rng);
  else pipeline.chromaShift.enabled = false;

  // The global roll owns the physics rail decision either way.
  pipeline.afeBend.enabled = false;
  pipeline.busBend.enabled = false;
  if (physicsLed || fullStack) rollPhysicsRail(preset, mode, rng);
}

// Roll the physics rail: usually one circuit leads, occasionally both stack
// (in signal order, analog front end before the data bus). Stacked circuits
// are damped — two full-strength physics bends erase the subject entirely.
function rollPhysicsRail(preset, mode, rng) {
  const roll = rng();
  if (roll < 0.18) {
    MODULE_RANDOMIZERS.afeBend(preset, mode, rng);
    MODULE_RANDOMIZERS.busBend(preset, mode, rng);
    const afe = preset.pipeline.afeBend;
    const bus = preset.pipeline.busBend;
    afe.inject *= 0.45;
    afe.gainMod *= 0.45;
    afe.cdsAmount *= 0.5;
    bus.injectStrength *= 0.7;
    bus.pot = clamp(bus.pot, 0.25, 1);
  } else if (roll < 0.58) {
    MODULE_RANDOMIZERS.afeBend(preset, mode, rng);
  } else {
    MODULE_RANDOMIZERS.busBend(preset, mode, rng);
  }
}

function randomizePhysics(preset, mode, rng) {
  preset.pipeline.afeBend.enabled = false;
  preset.pipeline.busBend.enabled = false;
  rollPhysicsRail(preset, mode, rng);
}

function randomizeColor(preset, mode, rng) {
  preset.macros.colorFault = value(mode, rng, 0.08);
  drift(preset.macros, "bend", 0.1, rng);
  applyMacrosToPipeline(preset);
  MODULE_RANDOMIZERS.falseColor(preset, mode, rng);
  if (rng() < 0.55) MODULE_RANDOMIZERS.colorBend(preset, mode, rng);
  else preset.pipeline.colorBend.enabled = false;
  if (rng() < 0.4) MODULE_RANDOMIZERS.gradientWash(preset, mode, rng);
  else preset.pipeline.gradientWash.enabled = false;
  if (rng() < 0.25) MODULE_RANDOMIZERS.awbSeizure(preset, mode, rng);
  else preset.pipeline.awbSeizure.enabled = false;
}

function randomizeMelt(preset, mode, rng) {
  preset.macros.melt = value(mode, rng, 0.08);
  drift(preset.macros, "burn", 0.08, rng);
  applyMacrosToPipeline(preset);
  MODULE_RANDOMIZERS.verticalSmear(preset, mode, rng);
  if (rng() < 0.7) MODULE_RANDOMIZERS.pixelSort(preset, mode, rng);
  else preset.pipeline.pixelSort.enabled = false;
}

function randomizeBurn(preset, mode, rng) {
  preset.macros.burn = value(mode, rng, 0.1);
  drift(preset.macros, "colorFault", 0.1, rng);
  applyMacrosToPipeline(preset);
  MODULE_RANDOMIZERS.exposureFault(preset, mode, rng);
  MODULE_RANDOMIZERS.contourRings(preset, mode, rng);
  MODULE_RANDOMIZERS.edgeBurn(preset, mode, rng);
}

function randomizeNoise(preset, mode, rng) {
  preset.macros.noise = value(mode, rng, 0.06);
  applyMacrosToPipeline(preset);
  MODULE_RANDOMIZERS.sensorNoise(preset, mode, rng);
  if (rng() < 0.3) MODULE_RANDOMIZERS.ampGlow(preset, mode, rng);
  else preset.pipeline.ampGlow.enabled = false;
}

function randomizeCheap(preset, mode, rng) {
  preset.macros.cheapness = value(mode, rng, 0.04);
  applyMacrosToPipeline(preset);
  MODULE_RANDOMIZERS.cheapCamera(preset, mode, rng);
  if (rng() < 0.55) MODULE_RANDOMIZERS.dctCrunch(preset, mode, rng);
  else preset.pipeline.dctCrunch.enabled = false;
  if (rng() < 0.35) MODULE_RANDOMIZERS.bayerFault(preset, mode, rng);
  else preset.pipeline.bayerFault.enabled = false;
}

function randomizeMemory(preset, mode, rng) {
  preset.macros.chaos = clamp(randomRange(Math.max(0.4, mode.min), mode.chaos, rng));
  applyMacrosToPipeline(preset);
  MODULE_RANDOMIZERS.memoryFault(preset, mode, rng);
  if (rng() < 0.5) MODULE_RANDOMIZERS.chromaShift(preset, mode, rng);
  else preset.pipeline.chromaShift.enabled = false;
  if (rng() < 0.45) MODULE_RANDOMIZERS.syncFault(preset, mode, rng);
  else preset.pipeline.syncFault.enabled = false;
  if (rng() < 0.4) MODULE_RANDOMIZERS.bufferGhost(preset, mode, rng);
  else preset.pipeline.bufferGhost.enabled = false;
}

function randomCameraName(rng) {
  const prefixes = ["Bent", "Dead", "Shorted", "Overheat", "Menu", "Flash", "CCD"];
  const cores = ["CCD", "Pocket", "Digicam", "Sensor", "Compact", "Memory"];
  const suffix = String(randomInt(1, 99, rng)).padStart(2, "0");
  return `${prefixes[randomInt(0, prefixes.length - 1, rng)]} ${cores[randomInt(0, cores.length - 1, rng)]}-${suffix}`;
}
