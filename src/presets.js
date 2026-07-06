import { clamp, clone, mergeDeep } from "./utils.js";

export const APP_VERSION = "0.1.0";

export const MACRO_DEFS = [
  ["bend", "Bend"],
  ["colorFault", "Color Fault"],
  ["melt", "Melt"],
  ["burn", "Burn"],
  ["noise", "Noise"],
  ["cheapness", "Cheapness"],
  ["chaos", "Chaos"]
];

export const FALSE_COLOR_MODES = [
  "solarized-ccd",
  "thermal-bleach",
  "pink-blue",
  "toxic-green",
  "rainbow",
  "acid-sunset",
  "infrared",
  "candy-shop",
  "poison-dart"
];

export const CHANNEL_MODES = ["none", "gbr", "brg", "grb", "bgr", "rbg"];
export const INVERT_MODES = ["none", "red", "green", "blue", "all"];

export const ADVANCED_DEFS = [
  {
    group: "Cheap Camera",
    key: "cheapCamera",
    controls: [
      ["pipeline.cheapCamera.enabled", "Enabled", "boolean"],
      ["pipeline.cheapCamera.internalScale", "Internal Scale", "range", 0.25, 1, 0.01],
      ["pipeline.cheapCamera.blur", "Lens Blur", "range", 0, 1, 0.01],
      ["pipeline.cheapCamera.bitDepth", "Bit Depth", "range", 3, 8, 1],
      ["pipeline.cheapCamera.dither", "Dither", "range", 0, 1, 0.01],
      ["pipeline.cheapCamera.sharpen", "Sharpen", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Sync Fault",
    key: "syncFault",
    controls: [
      ["pipeline.syncFault.enabled", "Enabled", "boolean"],
      ["pipeline.syncFault.tearCount", "Tear Count", "range", 0, 1, 0.01],
      ["pipeline.syncFault.tearShift", "Tear Shift", "range", 0, 1, 0.01],
      ["pipeline.syncFault.wobbleAmount", "Wobble", "range", 0, 1, 0.01],
      ["pipeline.syncFault.wobbleFrequency", "Wobble Freq", "range", 0, 1, 0.01],
      ["pipeline.syncFault.drift", "Phase Drift", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Bayer Fault",
    key: "bayerFault",
    controls: [
      ["pipeline.bayerFault.enabled", "Enabled", "boolean"],
      ["pipeline.bayerFault.phaseError", "Phase Error", "range", 0, 3, 1],
      ["pipeline.bayerFault.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.bayerFault.zipper", "Zipper Edges", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Buffer Ghost",
    key: "bufferGhost",
    controls: [
      ["pipeline.bufferGhost.enabled", "Enabled", "boolean"],
      ["__ghostSource", "Ghost Source", "ghost"],
      ["pipeline.bufferGhost.amount", "Amount", "range", 0, 1, 0.01],
      ["pipeline.bufferGhost.blockSize", "Block Size", "range", 0, 1, 0.01],
      ["pipeline.bufferGhost.ghostShift", "Ghost Shift", "range", 0, 1, 0.01],
      ["pipeline.bufferGhost.ghostZoom", "Ghost Zoom", "range", 0, 1, 0.01],
      ["pipeline.bufferGhost.fieldMode", "Field Interlace", "boolean"]
    ]
  },
  {
    group: "Color Bend",
    key: "colorBend",
    controls: [
      ["pipeline.colorBend.enabled", "Enabled", "boolean"],
      ["pipeline.colorBend.hueRotate", "Hue Rotate", "range", 0, 360, 1],
      ["pipeline.colorBend.hueStrength", "Hue Strength", "range", 0, 1, 0.01],
      ["pipeline.colorBend.channelMode", "Channel Swap", "select", CHANNEL_MODES],
      ["pipeline.colorBend.channelStrength", "Swap Strength", "range", 0, 1, 0.01],
      ["pipeline.colorBend.invert", "Invert Channel", "select", INVERT_MODES],
      ["pipeline.colorBend.invertStrength", "Invert Strength", "range", 0, 1, 0.01],
      ["pipeline.colorBend.solarize", "Solarize", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Chroma Shift",
    key: "chromaShift",
    controls: [
      ["pipeline.chromaShift.enabled", "Enabled", "boolean"],
      ["pipeline.chromaShift.amount", "Amount", "range", 0, 1, 0.01],
      ["pipeline.chromaShift.angle", "Angle", "range", 0, 360, 1],
      ["pipeline.chromaShift.wobble", "Row Wobble", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Exposure Fault",
    key: "exposureFault",
    controls: [
      ["pipeline.exposureFault.enabled", "Enabled", "boolean"],
      ["pipeline.exposureFault.gain", "Gain", "range", 0.65, 2.2, 0.01],
      ["pipeline.exposureFault.blackCrush", "Black Crush", "range", 0, 1, 0.01],
      ["pipeline.exposureFault.highlightClip", "Highlight Clip", "range", 0, 1, 0.01],
      ["pipeline.exposureFault.contourBands", "Contour Bands", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Contour Rings",
    key: "contourRings",
    controls: [
      ["pipeline.contourRings.enabled", "Enabled", "boolean"],
      ["pipeline.contourRings.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.contourRings.scale", "Ring Scale", "range", 0, 1, 0.01],
      ["pipeline.contourRings.bandSharpness", "Band Sharpness", "range", 0, 1, 0.01],
      ["pipeline.contourRings.tonalBias", "Tonal Bias", "range", 0, 1, 0.01],
      ["pipeline.contourRings.colorBleed", "Color Bleed", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "False Color",
    key: "falseColor",
    controls: [
      ["pipeline.falseColor.enabled", "Enabled", "boolean"],
      ["pipeline.falseColor.mode", "Palette", "select", FALSE_COLOR_MODES],
      ["pipeline.falseColor.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.falseColor.posterizeLevels", "Posterize", "range", 3, 14, 1],
      ["pipeline.falseColor.smoothness", "Smoothness", "range", 0, 1, 0.01],
      ["pipeline.falseColor.channelSwap", "Channel Swap", "range", 0, 1, 0.01],
      ["pipeline.falseColor.hueWarp", "Hue Warp", "range", 0, 1, 0.01],
      ["pipeline.falseColor.saturation", "Saturation", "range", 0.5, 3, 0.01]
    ]
  },
  {
    group: "Gradient Wash",
    key: "gradientWash",
    controls: [
      ["pipeline.gradientWash.enabled", "Enabled", "boolean"],
      ["pipeline.gradientWash.mode", "Palette", "select", FALSE_COLOR_MODES],
      ["pipeline.gradientWash.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.gradientWash.angle", "Angle", "range", 0, 360, 1],
      ["pipeline.gradientWash.scale", "Band Scale", "range", 0.05, 1, 0.01],
      ["pipeline.gradientWash.keepLuma", "Keep Luma", "range", 0, 1, 0.01],
      ["pipeline.gradientWash.wobble", "Wobble", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Pixel Sort",
    key: "pixelSort",
    controls: [
      ["pipeline.pixelSort.enabled", "Enabled", "boolean"],
      ["pipeline.pixelSort.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.pixelSort.threshold", "Threshold", "range", 0.05, 0.95, 0.01],
      ["pipeline.pixelSort.window", "Band Window", "range", 0.02, 1, 0.01],
      ["pipeline.pixelSort.direction", "Direction", "select", ["down", "up"]],
      ["pipeline.pixelSort.mode", "Trigger", "select", ["bright", "dark"]],
      ["pipeline.pixelSort.maxRun", "Max Run", "range", 0.05, 1, 0.01]
    ]
  },
  {
    group: "Edge Burn",
    key: "edgeBurn",
    controls: [
      ["pipeline.edgeBurn.enabled", "Enabled", "boolean"],
      ["pipeline.edgeBurn.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.edgeBurn.threshold", "Threshold", "range", 0.02, 0.45, 0.01],
      ["pipeline.edgeBurn.darkOutline", "Dark Outline", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Vertical Smear",
    key: "verticalSmear",
    controls: [
      ["pipeline.verticalSmear.enabled", "Enabled", "boolean"],
      ["pipeline.verticalSmear.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.verticalSmear.threshold", "Threshold", "range", 0.05, 0.95, 0.01],
      ["pipeline.verticalSmear.decay", "Decay", "range", 0.65, 0.995, 0.001],
      ["pipeline.verticalSmear.length", "Trail Length", "range", 0, 1, 0.01],
      ["pipeline.verticalSmear.spread", "Column Spread", "range", 0, 1, 0.01],
      ["pipeline.verticalSmear.contrast", "Trail Contrast", "range", 0, 1, 0.01],
      ["pipeline.verticalSmear.curtainStrength", "Curtain Strength", "range", 0, 1, 0.01],
      ["pipeline.verticalSmear.curtainDensity", "Curtain Starts", "range", 0, 1, 0.01],
      ["pipeline.verticalSmear.curtainDrop", "Curtain Drop", "range", 0, 1, 0.01],
      ["pipeline.verticalSmear.jitter", "Jitter", "range", 0, 1, 0.01],
      ["pipeline.verticalSmear.edgeBias", "Edge Bias", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Sensor Noise",
    key: "sensorNoise",
    controls: [
      ["pipeline.sensorNoise.enabled", "Enabled", "boolean"],
      ["pipeline.sensorNoise.amount", "Amount", "range", 0, 1, 0.01],
      ["pipeline.sensorNoise.colorAmount", "Color Amount", "range", 0, 1, 0.01],
      ["pipeline.sensorNoise.shadowBias", "Shadow Bias", "range", 0, 1, 0.01],
      ["pipeline.sensorNoise.striping", "Striping", "range", 0, 1, 0.01],
      ["pipeline.sensorNoise.hotPixels", "Hot Pixels", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Memory Fault",
    key: "memoryFault",
    controls: [
      ["pipeline.memoryFault.enabled", "Enabled", "boolean"],
      ["pipeline.memoryFault.interlace", "Interlace", "range", 0, 1, 0.01],
      ["pipeline.memoryFault.blockShift", "Block Shift", "range", 0, 1, 0.01],
      ["pipeline.memoryFault.rowRepeat", "Row Repeat", "range", 0, 1, 0.01],
      ["pipeline.memoryFault.scanlineDropout", "Scanline Dropout", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "DCT Crunch",
    key: "dctCrunch",
    controls: [
      ["pipeline.dctCrunch.enabled", "Enabled", "boolean"],
      ["pipeline.dctCrunch.quality", "Quality", "range", 0, 1, 0.01],
      ["pipeline.dctCrunch.chromaSubsample", "Chroma Subsample", "range", 0, 1, 0.01],
      ["pipeline.dctCrunch.dcDrift", "DC Drift", "range", 0, 1, 0.01],
      ["pipeline.dctCrunch.acScramble", "AC Scramble", "range", 0, 1, 0.01],
      ["pipeline.dctCrunch.blockRepeat", "Block Repeat", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "OSD Overlay",
    key: "osdOverlay",
    controls: [
      ["pipeline.osdOverlay.enabled", "Enabled", "boolean"],
      ["pipeline.osdOverlay.datestamp", "Datestamp", "boolean"],
      ["pipeline.osdOverlay.hudIcons", "HUD Icons", "boolean"],
      ["pipeline.osdOverlay.glitchText", "Glitch Text", "range", 0, 1, 0.01],
      ["pipeline.osdOverlay.scale", "Scale", "range", 0, 1, 0.01],
      ["pipeline.osdOverlay.color", "Color", "select", ["orange", "green", "white"]]
    ]
  }
];

function defaultMacros() {
  return {
    bend: 0.62,
    colorFault: 0.82,
    melt: 0.45,
    burn: 0.65,
    noise: 0.36,
    cheapness: 0.28,
    chaos: 0.32
  };
}

function defaultPipeline() {
  return {
    cheapCamera: {
      enabled: true,
      internalScale: 0.8,
      blur: 0,
      bitDepth: 6,
      dither: 0.4,
      sharpen: 0.35
    },
    syncFault: {
      enabled: false,
      tearCount: 0.4,
      tearShift: 0.4,
      wobbleAmount: 0.25,
      wobbleFrequency: 0.4,
      drift: 0.35
    },
    bayerFault: {
      enabled: false,
      phaseError: 1,
      strength: 0.7,
      zipper: 0.3
    },
    bufferGhost: {
      enabled: false,
      amount: 0.5,
      blockSize: 0.35,
      ghostShift: 0.35,
      ghostZoom: 0.15,
      fieldMode: false
    },
    colorBend: {
      enabled: false,
      hueRotate: 0,
      hueStrength: 1,
      channelMode: "none",
      channelStrength: 1,
      invert: "none",
      invertStrength: 1,
      solarize: 0
    },
    chromaShift: {
      enabled: false,
      amount: 0,
      angle: 0,
      wobble: 0.2
    },
    exposureFault: {
      enabled: true,
      gain: 1.25,
      blackCrush: 0.25,
      highlightClip: 0.62,
      contourBands: 0.45,
      clipColorBias: [1, 0.18, 0.86]
    },
    contourRings: {
      enabled: true,
      strength: 0.42,
      scale: 0.54,
      bandSharpness: 0.62,
      tonalBias: 0.58,
      colorBleed: 0.5
    },
    falseColor: {
      enabled: true,
      mode: "solarized-ccd",
      strength: 0.82,
      posterizeLevels: 7,
      smoothness: 0,
      channelSwap: 0.28,
      hueWarp: 0.48,
      saturation: 1.95
    },
    gradientWash: {
      enabled: false,
      mode: "rainbow",
      strength: 0,
      angle: 35,
      scale: 0.7,
      keepLuma: 0.75,
      wobble: 0.3
    },
    pixelSort: {
      enabled: false,
      strength: 0,
      threshold: 0.6,
      window: 0.35,
      direction: "down",
      mode: "bright",
      maxRun: 0.6
    },
    edgeBurn: {
      enabled: true,
      strength: 0.48,
      threshold: 0.12,
      darkOutline: 0.45,
      palette: ["cyan", "magenta", "green", "black", "white"]
    },
    verticalSmear: {
      enabled: true,
      strength: 0.58,
      threshold: 0.5,
      decay: 0.94,
      length: 0.54,
      spread: 0.2,
      contrast: 0.5,
      curtainStrength: 0.36,
      curtainDensity: 0.34,
      curtainDrop: 0.46,
      jitter: 0.18,
      edgeBias: 0.52,
      direction: "down"
    },
    sensorNoise: {
      enabled: true,
      amount: 0.3,
      colorAmount: 0.78,
      shadowBias: 0.55,
      striping: 0.25,
      hotPixels: 0.12,
      speckleSize: 1
    },
    memoryFault: {
      enabled: false,
      interlace: 0,
      blockShift: 0,
      rowRepeat: 0,
      scanlineDropout: 0
    },
    dctCrunch: {
      enabled: false,
      quality: 0.55,
      chromaSubsample: 0.6,
      dcDrift: 0,
      acScramble: 0,
      blockRepeat: 0
    },
    osdOverlay: {
      enabled: false,
      datestamp: true,
      hudIcons: true,
      glitchText: 0,
      scale: 0.5,
      color: "orange"
    },
    output: {
      exportScale: 1,
      format: "png",
      preserveOriginalResolution: true
    }
  };
}

function migratePipeline(input = {}) {
  const pipeline = clone(input);
  if (pipeline.skyRings && !pipeline.contourRings) {
    const { cloudBias, ...rings } = pipeline.skyRings;
    pipeline.contourRings = {
      ...rings,
      tonalBias: cloudBias ?? rings.tonalBias ?? 0.5
    };
  }
  delete pipeline.skyRings;
  return pipeline;
}

export function applyMacrosToPipeline(preset) {
  const m = preset.macros;
  const p = preset.pipeline;
  const bend = clamp(m.bend);
  const chaos = clamp(m.chaos);

  p.cheapCamera.enabled = m.cheapness > 0.04;
  p.cheapCamera.internalScale = clamp(1 - m.cheapness * 0.62, 0.32, 1);
  p.cheapCamera.bitDepth = Math.round(8 - m.cheapness * 4.5);
  p.cheapCamera.dither = clamp(0.2 + m.cheapness * 0.55 + m.noise * 0.25);
  p.cheapCamera.sharpen = clamp(m.cheapness * 0.85 + bend * 0.1);

  p.exposureFault.enabled = m.burn > 0.02 || bend > 0.2;
  p.exposureFault.gain = 0.85 + m.burn * 0.92 + bend * 0.22;
  p.exposureFault.blackCrush = clamp(m.burn * 0.34 + bend * 0.18);
  p.exposureFault.highlightClip = clamp(m.burn * 0.9 + chaos * 0.18);
  p.exposureFault.contourBands = clamp(m.burn * 0.62 + m.colorFault * 0.24);

  p.contourRings.enabled = m.burn > 0.12 || m.colorFault > 0.22;
  p.contourRings.strength = clamp(m.burn * 0.48 + m.colorFault * 0.34 + bend * 0.12);
  p.contourRings.scale = clamp(0.28 + m.colorFault * 0.34 + chaos * 0.22);
  p.contourRings.bandSharpness = clamp(m.burn * 0.55 + m.colorFault * 0.32);
  p.contourRings.tonalBias = clamp(0.24 + m.burn * 0.48 + bend * 0.12);
  p.contourRings.colorBleed = clamp(m.colorFault * 0.5 + chaos * 0.22);

  p.falseColor.enabled = m.colorFault > 0.03 || bend > 0.25;
  p.falseColor.strength = clamp(0.12 + m.colorFault * 0.88);
  p.falseColor.posterizeLevels = Math.round(12 - m.colorFault * 7);
  p.falseColor.channelSwap = clamp(m.colorFault * 0.34 + chaos * 0.2);
  p.falseColor.hueWarp = clamp(m.colorFault * 0.48 + chaos * 0.26);
  p.falseColor.saturation = 1 + m.colorFault * 1.45 + bend * 0.42;

  p.edgeBurn.enabled = m.colorFault > 0.08 || m.burn > 0.08;
  p.edgeBurn.strength = clamp(m.colorFault * 0.45 + m.burn * 0.32 + bend * 0.18);
  p.edgeBurn.threshold = clamp(0.22 - bend * 0.08 - m.burn * 0.07, 0.06, 0.28);
  p.edgeBurn.darkOutline = clamp(m.burn * 0.42 + m.colorFault * 0.28);

  p.pixelSort.enabled = m.melt > 0.22;
  p.pixelSort.strength = clamp((m.melt - 0.15) * 1.15 + chaos * 0.1);
  p.pixelSort.threshold = clamp(0.72 - m.melt * 0.28 - m.burn * 0.08, 0.18, 0.86);

  p.verticalSmear.enabled = m.melt > 0.03;
  p.verticalSmear.strength = clamp(m.melt * 1.05 + bend * 0.14 + chaos * 0.2);
  p.verticalSmear.threshold = clamp(0.68 - m.melt * 0.42 - m.burn * 0.14, 0.16, 0.84);
  p.verticalSmear.decay = clamp(0.84 + m.melt * 0.12 + chaos * 0.035, 0.74, 0.992);
  p.verticalSmear.length = clamp(m.melt * 0.82 + m.burn * 0.18 + chaos * 0.16);
  p.verticalSmear.spread = clamp(m.melt * 0.32 + chaos * 0.25);
  p.verticalSmear.contrast = clamp(m.melt * 0.78 + bend * 0.16 + chaos * 0.18);
  p.verticalSmear.curtainStrength = clamp(m.melt * 0.72 + m.burn * 0.18 + chaos * 0.18);
  p.verticalSmear.curtainDensity = clamp(m.melt * 0.48 + chaos * 0.28);
  p.verticalSmear.curtainDrop = clamp(m.melt * 0.7 + m.burn * 0.16 + chaos * 0.14);
  p.verticalSmear.jitter = clamp(m.melt * 0.26 + chaos * 0.38);
  p.verticalSmear.edgeBias = clamp(m.melt * 0.62 + bend * 0.28);

  p.sensorNoise.enabled = m.noise > 0.02 || m.cheapness > 0.1;
  p.sensorNoise.amount = clamp(m.noise * 0.78 + chaos * 0.12);
  p.sensorNoise.colorAmount = clamp(0.4 + m.colorFault * 0.42 + chaos * 0.22);
  p.sensorNoise.shadowBias = clamp(0.35 + m.noise * 0.42);
  p.sensorNoise.striping = clamp(m.noise * 0.22 + m.melt * 0.22);
  p.sensorNoise.hotPixels = clamp(m.noise * 0.16 + chaos * 0.16);

  p.dctCrunch.enabled = m.cheapness > 0.55;
  p.dctCrunch.quality = clamp(1 - (m.cheapness - 0.3) * 1.05 - chaos * 0.08, 0.08, 0.95);
  p.dctCrunch.chromaSubsample = clamp((m.cheapness - 0.3) * 1.8);
  p.dctCrunch.acScramble = clamp((chaos - 0.55) * 0.5);
  p.dctCrunch.blockRepeat = clamp((chaos - 0.6) * 0.4);

  p.memoryFault.enabled = chaos > 0.45;
  p.memoryFault.interlace = clamp((chaos - 0.38) * 0.85);
  p.memoryFault.blockShift = clamp((chaos - 0.45) * 0.62);
  p.memoryFault.rowRepeat = clamp((chaos - 0.42) * 0.5);
  p.memoryFault.scanlineDropout = clamp((chaos - 0.52) * 0.56);

  p.chromaShift.enabled = p.chromaShift.enabled || chaos > 0.6;
  if (chaos > 0.6) {
    p.chromaShift.amount = Math.max(p.chromaShift.amount, clamp((chaos - 0.55) * 0.5));
  }

  p.syncFault.enabled = chaos > 0.62;
  p.syncFault.tearCount = clamp((chaos - 0.55) * 1.6);
  p.syncFault.tearShift = clamp(0.2 + chaos * 0.45);
  p.syncFault.wobbleAmount = clamp((chaos - 0.62) * 0.9 + m.melt * 0.06);

  p.bufferGhost.enabled = chaos > 0.7;
  p.bufferGhost.amount = clamp((chaos - 0.62) * 0.9);

  return preset;
}

export function createPreset({
  name,
  cameraModel = name,
  description = "",
  tags = [],
  seed = 1,
  macros = defaultMacros(),
  pipeline = {}
}) {
  const preset = {
    schemaVersion: 1,
    appVersion: APP_VERSION,
    name,
    cameraModel,
    description,
    tags,
    seed,
    createdAt: new Date().toISOString(),
    thumbnail: null,
    exampleOutputs: [],
    macros: { ...defaultMacros(), ...macros },
    pipeline: defaultPipeline()
  };
  applyMacrosToPipeline(preset);
  mergeDeep(preset.pipeline, migratePipeline(pipeline));
  return preset;
}

export function createDefaultPreset() {
  return createPreset({
    name: "Bent CCD-03",
    description: "Magenta and cyan clipping with medium vertical melt.",
    tags: ["ccd", "melt", "false-color"],
    seed: 381941
  });
}

export const BUILT_IN_PRESETS = [
  createDefaultPreset(),
  createPreset({
    name: "Dead Flash Compact",
    description: "Blown highlights, hard edge burn, and noisy shadows.",
    tags: ["flash", "burn", "noise"],
    seed: 9871,
    macros: {
      bend: 0.72,
      colorFault: 0.76,
      melt: 0.22,
      burn: 0.92,
      noise: 0.48,
      cheapness: 0.24,
      chaos: 0.36
    },
    pipeline: {
      exposureFault: { clipColorBias: [0.95, 0.9, 1] },
      contourRings: {
        strength: 0.72,
        scale: 0.64,
        bandSharpness: 0.78,
        tonalBias: 0.82,
        colorBleed: 0.6
      },
      falseColor: { mode: "thermal-bleach" }
    }
  }),
  createPreset({
    name: "Memory Card Fever",
    description: "Block and scanline faults with moderate color damage.",
    tags: ["memory", "scanline", "buffer"],
    seed: 77221,
    macros: {
      bend: 0.58,
      colorFault: 0.58,
      melt: 0.16,
      burn: 0.34,
      noise: 0.52,
      cheapness: 0.48,
      chaos: 0.86
    },
    pipeline: {
      memoryFault: {
        enabled: true,
        blockShift: 0.55,
        rowRepeat: 0.42,
        scanlineDropout: 0.38
      },
      verticalSmear: { enabled: false, strength: 0.08 }
    }
  }),
  createPreset({
    name: "Overheated Sensor",
    description: "Heavy downward smear, dense noise, and pink-blue bias.",
    tags: ["sensor", "smear", "overheat"],
    seed: 440117,
    macros: {
      bend: 0.86,
      colorFault: 0.9,
      melt: 0.84,
      burn: 0.72,
      noise: 0.4,
      cheapness: 0.34,
      chaos: 0.58
    },
    pipeline: {
      exposureFault: { clipColorBias: [1, 0.12, 0.76] },
      falseColor: { mode: "pink-blue" },
      sensorNoise: { amount: 0.3, shadowBias: 0.45 },
      verticalSmear: {
        strength: 0.86,
        threshold: 0.44,
        decay: 0.982,
        length: 0.9,
        spread: 0.36,
        contrast: 0.8,
        curtainStrength: 0.82,
        curtainDensity: 0.6,
        curtainDrop: 0.88,
        edgeBias: 0.9
      }
    }
  }),
  createPreset({
    name: "IR Bloom",
    description: "Infrared negative bloom: magenta foliage over deep blue with sorted drips.",
    tags: ["infrared", "bloom", "sort"],
    seed: 660913,
    macros: {
      bend: 0.4,
      colorFault: 0.66,
      melt: 0.42,
      burn: 0.3,
      noise: 0.44,
      cheapness: 0.3,
      chaos: 0.24
    },
    pipeline: {
      falseColor: {
        mode: "infrared",
        strength: 0.88,
        smoothness: 0.85,
        posterizeLevels: 9,
        channelSwap: 0,
        hueWarp: 0.1,
        saturation: 1.7
      },
      contourRings: { enabled: false },
      edgeBurn: { strength: 0.2 },
      pixelSort: { strength: 0.55, threshold: 0.42, window: 0.4, maxRun: 0.7 },
      sensorNoise: { amount: 0.34, colorAmount: 0.9, shadowBias: 0.7 }
    }
  }),
  createPreset({
    name: "Rainbow Burst",
    description: "Diagonal rainbow wash over grainy skies, luminance preserved.",
    tags: ["rainbow", "wash", "grain"],
    seed: 240471,
    macros: {
      bend: 0.3,
      colorFault: 0.4,
      melt: 0.06,
      burn: 0.32,
      noise: 0.55,
      cheapness: 0.36,
      chaos: 0.14
    },
    pipeline: {
      falseColor: { enabled: false },
      contourRings: { enabled: false },
      gradientWash: {
        enabled: true,
        mode: "rainbow",
        strength: 0.82,
        angle: 32,
        scale: 0.85,
        keepLuma: 0.66,
        wobble: 0.55
      },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { strength: 0.18 },
      sensorNoise: { amount: 0.42, colorAmount: 1, shadowBias: 0.4, striping: 0.08 },
      cheapCamera: { internalScale: 0.9, bitDepth: 6, dither: 0.85, sharpen: 0.2 }
    }
  }),
  createPreset({
    name: "Acid Lake",
    description: "Red-orange solar sky bleeding into cyan trees and pink water.",
    tags: ["acid", "sunset", "gradient"],
    seed: 118221,
    macros: {
      bend: 0.42,
      colorFault: 0.78,
      melt: 0.1,
      burn: 0.5,
      noise: 0.3,
      cheapness: 0.22,
      chaos: 0.12
    },
    pipeline: {
      falseColor: {
        mode: "acid-sunset",
        strength: 0.86,
        smoothness: 0.72,
        posterizeLevels: 10,
        channelSwap: 0.14,
        hueWarp: 0.22,
        saturation: 2.1
      },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { strength: 0.32, palette: ["cyan", "green", "magenta"] },
      sensorNoise: { amount: 0.22, colorAmount: 0.85 }
    }
  }),
  createPreset({
    name: "Candy Snow",
    description: "Pastel duotone: teal shadows, pink highlights, soft grain.",
    tags: ["candy", "pastel", "duotone"],
    seed: 872530,
    macros: {
      bend: 0.28,
      colorFault: 0.6,
      melt: 0.08,
      burn: 0.24,
      noise: 0.34,
      cheapness: 0.3,
      chaos: 0.08
    },
    pipeline: {
      falseColor: {
        mode: "candy-shop",
        strength: 0.85,
        smoothness: 0.8,
        posterizeLevels: 8,
        channelSwap: 0,
        hueWarp: 0.05,
        saturation: 1.35
      },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false },
      exposureFault: { gain: 1.1, blackCrush: 0.12, highlightClip: 0.3 },
      sensorNoise: { amount: 0.26, colorAmount: 0.6, shadowBias: 0.5 }
    }
  }),
  createPreset({
    name: "Pixel Melter",
    description: "Heavy sorted drips pulling highlights down the frame.",
    tags: ["sort", "melt", "drip"],
    seed: 505861,
    macros: {
      bend: 0.5,
      colorFault: 0.52,
      melt: 0.85,
      burn: 0.4,
      noise: 0.4,
      cheapness: 0.2,
      chaos: 0.3
    },
    pipeline: {
      falseColor: {
        mode: "infrared",
        strength: 0.55,
        smoothness: 0.6,
        saturation: 1.6
      },
      pixelSort: {
        enabled: true,
        strength: 0.88,
        threshold: 0.4,
        window: 0.5,
        direction: "down",
        maxRun: 0.85
      },
      verticalSmear: { strength: 0.4, curtainStrength: 0.5 },
      contourRings: { enabled: false }
    }
  }),
  createPreset({
    name: "Interlace Crash",
    description: "Row-interlace tearing, block shifts, and split chroma fringes.",
    tags: ["interlace", "buffer", "fringe"],
    seed: 331299,
    macros: {
      bend: 0.44,
      colorFault: 0.36,
      melt: 0.06,
      burn: 0.3,
      noise: 0.38,
      cheapness: 0.4,
      chaos: 0.72
    },
    pipeline: {
      falseColor: { strength: 0.3, smoothness: 0.5, saturation: 1.5 },
      chromaShift: { enabled: true, amount: 0.4, angle: 0, wobble: 0.5 },
      memoryFault: {
        enabled: true,
        interlace: 0.85,
        blockShift: 0.5,
        rowRepeat: 0.3,
        scanlineDropout: 0.16
      },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      contourRings: { enabled: false }
    }
  }),
  createPreset({
    name: "Night Stalker",
    description: "Crushed blacks with toxic green speckle and hot red cores.",
    tags: ["night", "crush", "toxic"],
    seed: 771040,
    macros: {
      bend: 0.5,
      colorFault: 0.5,
      melt: 0.05,
      burn: 0.7,
      noise: 0.66,
      cheapness: 0.34,
      chaos: 0.2
    },
    pipeline: {
      exposureFault: {
        gain: 1.4,
        blackCrush: 0.82,
        highlightClip: 0.5,
        clipColorBias: [1, 0.16, 0.14]
      },
      falseColor: {
        mode: "toxic-green",
        strength: 0.6,
        posterizeLevels: 6,
        smoothness: 0.25,
        saturation: 1.9
      },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      contourRings: { enabled: false },
      sensorNoise: { amount: 0.5, colorAmount: 0.95, shadowBias: 0.85, hotPixels: 0.3 }
    }
  }),
  createPreset({
    name: "Poison Corridor",
    description: "Melted lens blur posterized into toxic green contour slime.",
    tags: ["blur", "posterize", "slime"],
    seed: 448101,
    macros: {
      bend: 0.4,
      colorFault: 0.62,
      melt: 0.12,
      burn: 0.36,
      noise: 0.24,
      cheapness: 0.62,
      chaos: 0.14
    },
    pipeline: {
      cheapCamera: {
        enabled: true,
        internalScale: 0.55,
        blur: 0.62,
        bitDepth: 5,
        dither: 0.3,
        sharpen: 0.15
      },
      falseColor: {
        mode: "poison-dart",
        strength: 0.78,
        posterizeLevels: 7,
        smoothness: 0.15,
        saturation: 1.5
      },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      contourRings: { enabled: false },
      edgeBurn: { enabled: false }
    }
  }),
  createPreset({
    name: "Negative Bloom",
    description: "Full channel inversion with saturation push and soft solarize.",
    tags: ["negative", "invert", "solar"],
    seed: 913377,
    macros: {
      bend: 0.36,
      colorFault: 0.4,
      melt: 0.3,
      burn: 0.28,
      noise: 0.4,
      cheapness: 0.24,
      chaos: 0.16
    },
    pipeline: {
      colorBend: {
        enabled: true,
        hueRotate: 24,
        hueStrength: 1,
        invert: "all",
        invertStrength: 1,
        solarize: 0.2
      },
      falseColor: { strength: 0.12, smoothness: 0.7, saturation: 1.9 },
      contourRings: { enabled: false },
      pixelSort: { strength: 0.4, threshold: 0.5 },
      sensorNoise: { amount: 0.32, colorAmount: 0.9 }
    }
  }),
  createPreset({
    name: "Codec Rot",
    description: "Corrupted JPEG decode: block hue drift, scrambled macroblocks, chunky chroma.",
    tags: ["jpeg", "dct", "codec"],
    seed: 622377,
    macros: {
      bend: 0.32,
      colorFault: 0.24,
      melt: 0.02,
      burn: 0.3,
      noise: 0.22,
      cheapness: 0.62,
      chaos: 0.5
    },
    pipeline: {
      dctCrunch: {
        enabled: true,
        quality: 0.42,
        chromaSubsample: 0.8,
        dcDrift: 0.58,
        acScramble: 0.55,
        blockRepeat: 0.25
      },
      cheapCamera: { internalScale: 0.88, blur: 0, bitDepth: 7, dither: 0.25, sharpen: 0.2 },
      falseColor: { enabled: false },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false },
      memoryFault: { enabled: false },
      sensorNoise: { amount: 0.12, striping: 0.04, hotPixels: 0.03 }
    }
  }),
  createPreset({
    name: "Hold Vertical",
    description: "Lost sync: frame-wrap tears, wavy rolling shutter, interlace fringes.",
    tags: ["sync", "tear", "wobble"],
    seed: 274199,
    macros: {
      bend: 0.4,
      colorFault: 0.3,
      melt: 0.08,
      burn: 0.3,
      noise: 0.36,
      cheapness: 0.36,
      chaos: 0.72
    },
    pipeline: {
      syncFault: {
        enabled: true,
        tearCount: 0.7,
        tearShift: 0.55,
        wobbleAmount: 0.32,
        wobbleFrequency: 0.45,
        drift: 0.55
      },
      chromaShift: { enabled: true, amount: 0.22, angle: 0, wobble: 0.35 },
      memoryFault: { enabled: true, interlace: 0.28, blockShift: 0.14, rowRepeat: 0.1, scanlineDropout: 0.06 },
      falseColor: { strength: 0.28, smoothness: 0.55, saturation: 1.4 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      dctCrunch: { enabled: false },
      sensorNoise: { amount: 0.2, striping: 0.12 }
    }
  }),
  createPreset({
    name: "Tourist Compact '03",
    description: "Overcompressed vacation snap with orange datestamp and HUD burn-in.",
    tags: ["osd", "datestamp", "jpeg"],
    seed: 190803,
    macros: {
      bend: 0.28,
      colorFault: 0.3,
      melt: 0.03,
      burn: 0.38,
      noise: 0.3,
      cheapness: 0.66,
      chaos: 0.14
    },
    pipeline: {
      osdOverlay: {
        enabled: true,
        datestamp: true,
        hudIcons: true,
        glitchText: 0.12,
        scale: 0.5,
        color: "orange"
      },
      dctCrunch: {
        enabled: true,
        quality: 0.48,
        chromaSubsample: 0.85,
        dcDrift: 0,
        acScramble: 0.08,
        blockRepeat: 0
      },
      falseColor: { strength: 0.22, smoothness: 0.6, saturation: 1.45 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { strength: 0.16 },
      sensorNoise: { amount: 0.2, colorAmount: 0.6, striping: 0.06 }
    }
  }),
  createPreset({
    name: "Zipper Mosaic",
    description: "Misaligned demosaic: green-magenta checkerboards and zipper edges.",
    tags: ["bayer", "demosaic", "checkerboard"],
    seed: 815242,
    macros: {
      bend: 0.35,
      colorFault: 0.2,
      melt: 0.02,
      burn: 0.3,
      noise: 0.28,
      cheapness: 0.4,
      chaos: 0.15
    },
    pipeline: {
      bayerFault: { enabled: true, phaseError: 3, strength: 0.85, zipper: 0.6 },
      cheapCamera: { internalScale: 0.85, bitDepth: 7, dither: 0.3, sharpen: 0.55 },
      falseColor: { enabled: false },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false },
      exposureFault: { gain: 1.15, blackCrush: 0.15, highlightClip: 0.35 },
      sensorNoise: { amount: 0.14, colorAmount: 0.7, striping: 0.05 }
    }
  }),
  createPreset({
    name: "Double Buffer",
    description: "Uncleared frame buffer: a shifted stale frame bleeds through in blocks.",
    tags: ["ghost", "buffer", "stale"],
    seed: 733917,
    macros: {
      bend: 0.38,
      colorFault: 0.34,
      melt: 0.05,
      burn: 0.34,
      noise: 0.3,
      cheapness: 0.34,
      chaos: 0.72
    },
    pipeline: {
      bufferGhost: {
        enabled: true,
        amount: 0.62,
        blockSize: 0.5,
        ghostShift: 0.55,
        ghostZoom: 0.28,
        fieldMode: false
      },
      syncFault: { enabled: false },
      memoryFault: { enabled: true, interlace: 0.12, blockShift: 0.1, rowRepeat: 0.08, scanlineDropout: 0.04 },
      chromaShift: { enabled: true, amount: 0.14, angle: 0, wobble: 0.3 },
      falseColor: { strength: 0.24, smoothness: 0.6, saturation: 1.4 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      dctCrunch: { enabled: false },
      sensorNoise: { amount: 0.18, striping: 0.08 }
    }
  }),
  createPreset({
    name: "Cheap Menu Solar",
    description: "Posterized false color with minimal geometry damage.",
    tags: ["solar", "posterize", "menu"],
    seed: 19253,
    macros: {
      bend: 0.42,
      colorFault: 0.88,
      melt: 0.08,
      burn: 0.48,
      noise: 0.22,
      cheapness: 0.58,
      chaos: 0.18
    },
    pipeline: {
      verticalSmear: { enabled: false, strength: 0 },
      falseColor: { mode: "solarized-ccd", posterizeLevels: 5 }
    }
  })
];

export function normalizePreset(input) {
  const preset = createPreset({
    name: input?.name || "Imported Preset",
    cameraModel: input?.cameraModel || input?.name || "Imported Preset",
    description: input?.description || "",
    tags: Array.isArray(input?.tags) ? input.tags : [],
    seed: Number.isFinite(input?.seed) ? input.seed : Date.now(),
    macros: input?.macros || defaultMacros(),
    pipeline: input?.pipeline || {}
  });
  preset.thumbnail = input?.thumbnail || null;
  preset.exampleOutputs = Array.isArray(input?.exampleOutputs)
    ? input.exampleOutputs
    : [];
  preset.createdAt = input?.createdAt || new Date().toISOString();
  return preset;
}

export function clonePreset(preset) {
  return clone(preset);
}
