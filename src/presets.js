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
      ["pipeline.exposureFault.contourBands", "Contour Bands", "range", 0, 1, 0.01],
      ["pipeline.exposureFault.fringing", "Purple Fringing", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "AWB Seizure",
    key: "awbSeizure",
    controls: [
      ["pipeline.awbSeizure.enabled", "Enabled", "boolean"],
      ["pipeline.awbSeizure.wbSwing", "WB Swing", "range", 0, 1, 0.01],
      ["pipeline.awbSeizure.aeSwing", "AE Swing", "range", 0, 1, 0.01],
      ["pipeline.awbSeizure.bandHeight", "Band Height", "range", 0, 1, 0.01],
      ["pipeline.awbSeizure.frequency", "Frequency", "range", 0, 1, 0.01]
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
      ["pipeline.sensorNoise.hotPixels", "Hot Pixels", "range", 0, 1, 0.01],
      ["pipeline.sensorNoise.deadColumns", "Dead Columns", "range", 0, 1, 0.01],
      ["pipeline.sensorNoise.deadClusters", "Dead Clusters", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Amp Glow",
    key: "ampGlow",
    controls: [
      ["pipeline.ampGlow.enabled", "Enabled", "boolean"],
      ["pipeline.ampGlow.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.ampGlow.corner", "Corner", "select", ["seeded", "top-left", "top-right", "bottom-left", "bottom-right"]],
      ["pipeline.ampGlow.hue", "Hue", "range", 0, 1, 0.01],
      ["pipeline.ampGlow.spread", "Spread", "range", 0, 1, 0.01]
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
      ["pipeline.dctCrunch.blockRepeat", "Block Repeat", "range", 0, 1, 0.01],
      ["pipeline.dctCrunch.generations", "Generations", "range", 1, 6, 1]
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

export const ADVANCED_CONTROL_HELP = {
  "pipeline.cheapCamera.enabled": "Turns the cheap camera degradation stage on or off.",
  "pipeline.cheapCamera.internalScale": "Downscales internally before upscaling; lower values make chunkier pixels and stronger low-end camera artifacts.",
  "pipeline.cheapCamera.blur": "Softens the source like a poor lens before later damage is applied.",
  "pipeline.cheapCamera.bitDepth": "Limits color precision; lower values create harder posterization and channel stepping.",
  "pipeline.cheapCamera.dither": "Adds patterned noise around reduced color steps so flat bands break up.",
  "pipeline.cheapCamera.sharpen": "Adds harsh compact-camera edge sharpening after the cheap processing pass.",

  "pipeline.syncFault.enabled": "Turns timing and frame-sync damage on or off.",
  "pipeline.syncFault.tearCount": "Controls how many horizontal timing tears appear.",
  "pipeline.syncFault.tearShift": "Controls how far torn bands are displaced sideways.",
  "pipeline.syncFault.wobbleAmount": "Adds rolling-shutter style horizontal wobble.",
  "pipeline.syncFault.wobbleFrequency": "Controls how tightly the wobble waves repeat vertically.",
  "pipeline.syncFault.drift": "Offsets scan phase over the frame, making timing errors slide and bend.",

  "pipeline.bayerFault.enabled": "Turns broken Bayer/demosaic sampling on or off.",
  "pipeline.bayerFault.phaseError": "Shifts the Bayer sampling phase; higher values produce stronger wrong-color checker patterns.",
  "pipeline.bayerFault.strength": "Blends the demosaic fault into the image.",
  "pipeline.bayerFault.zipper": "Adds jagged color zippering around edges.",

  "pipeline.bufferGhost.enabled": "Turns stale-frame buffer ghosting on or off.",
  "__ghostSource": "Optional second image used as the stale buffer frame; without one the module reuses the current image.",
  "pipeline.bufferGhost.amount": "Blends more or less of the stale frame into the result.",
  "pipeline.bufferGhost.blockSize": "Controls the size of the copied ghost blocks.",
  "pipeline.bufferGhost.ghostShift": "Offsets the stale frame before it is mixed in.",
  "pipeline.bufferGhost.ghostZoom": "Scales the stale frame before it is mixed in.",
  "pipeline.bufferGhost.fieldMode": "Splits the ghost into alternating interlaced fields.",

  "pipeline.colorBend.enabled": "Turns whole-image color channel surgery on or off.",
  "pipeline.colorBend.hueRotate": "Rotates hue by degrees before channel faults are mixed in.",
  "pipeline.colorBend.hueStrength": "Controls how strongly the hue rotation affects the image.",
  "pipeline.colorBend.channelMode": "Chooses a hard RGB channel reorder.",
  "pipeline.colorBend.channelStrength": "Blends between original channels and the selected channel reorder.",
  "pipeline.colorBend.invert": "Chooses which channel, or all channels, to invert.",
  "pipeline.colorBend.invertStrength": "Blends between original color and the selected inversion.",
  "pipeline.colorBend.solarize": "Folds tones back on themselves for hard digital solarization.",

  "pipeline.chromaShift.enabled": "Turns RGB channel offsetting on or off.",
  "pipeline.chromaShift.amount": "Controls how far color channels separate from each other.",
  "pipeline.chromaShift.angle": "Sets the direction of the channel offset.",
  "pipeline.chromaShift.wobble": "Varies the channel offset by row for unstable color fringing.",

  "pipeline.exposureFault.enabled": "Turns gain, crush, clipping, and exposure contour faults on or off.",
  "pipeline.exposureFault.gain": "Raises or lowers signal gain before clipping.",
  "pipeline.exposureFault.blackCrush": "Pushes shadows toward blocked-up black.",
  "pipeline.exposureFault.highlightClip": "Clips bright areas into blown-out digital regions.",
  "pipeline.exposureFault.contourBands": "Adds hard tone bands around clipped and high-contrast areas.",
  "pipeline.exposureFault.fringing": "Adds a violet blooming rim around clipped highlights.",

  "pipeline.awbSeizure.enabled": "Turns white-balance and exposure hunting bands on or off.",
  "pipeline.awbSeizure.wbSwing": "Controls how far bands pump between warm and cold color.",
  "pipeline.awbSeizure.aeSwing": "Controls how far bands pump between bright and dark.",
  "pipeline.awbSeizure.bandHeight": "Sets the height of each hunting band.",
  "pipeline.awbSeizure.frequency": "Controls how quickly the hunting oscillates down the frame.",

  "pipeline.contourRings.enabled": "Turns luminance contour rings on or off.",
  "pipeline.contourRings.strength": "Controls overall ring intensity.",
  "pipeline.contourRings.scale": "Controls ring spacing; higher values broaden the contour pattern.",
  "pipeline.contourRings.bandSharpness": "Makes contour bands harder and more graphic.",
  "pipeline.contourRings.tonalBias": "Targets rings toward different brightness ranges.",
  "pipeline.contourRings.colorBleed": "Pushes contour rings into false color instead of mostly brightness changes.",

  "pipeline.falseColor.enabled": "Turns palette remapping and posterized false color on or off.",
  "pipeline.falseColor.mode": "Chooses the false-color palette.",
  "pipeline.falseColor.strength": "Blends between original color and the false-color mapping.",
  "pipeline.falseColor.posterizeLevels": "Sets the number of tone steps before palette mapping.",
  "pipeline.falseColor.smoothness": "Blends between hard poster bands and smoother gradients.",
  "pipeline.falseColor.channelSwap": "Adds extra RGB channel swapping inside the false-color pass.",
  "pipeline.falseColor.hueWarp": "Warps palette lookup so hues bend away from natural color.",
  "pipeline.falseColor.saturation": "Boosts or reduces the intensity of the remapped colors.",

  "pipeline.gradientWash.enabled": "Turns position-based color wash bands on or off.",
  "pipeline.gradientWash.mode": "Chooses the palette used by the wash.",
  "pipeline.gradientWash.strength": "Controls how much of the wash is blended over the image.",
  "pipeline.gradientWash.angle": "Sets the direction of the color bands.",
  "pipeline.gradientWash.scale": "Controls the width of the wash bands.",
  "pipeline.gradientWash.keepLuma": "Preserves original brightness so silhouettes and contrast survive the wash.",
  "pipeline.gradientWash.wobble": "Warps the wash bands with procedural noise.",

  "pipeline.pixelSort.enabled": "Turns vertical pixel sorting streaks on or off.",
  "pipeline.pixelSort.strength": "Controls how strongly sorted runs replace the original pixels.",
  "pipeline.pixelSort.threshold": "Sets the brightness trigger for starting sorted runs.",
  "pipeline.pixelSort.window": "Controls the local band size used for sorted runs.",
  "pipeline.pixelSort.direction": "Chooses whether sorted streaks run upward or downward.",
  "pipeline.pixelSort.mode": "Chooses whether bright or dark pixels trigger sorting.",
  "pipeline.pixelSort.maxRun": "Limits the maximum length of a sorted streak.",

  "pipeline.edgeBurn.enabled": "Turns colored edge halos and dark outlines on or off.",
  "pipeline.edgeBurn.strength": "Controls edge halo intensity.",
  "pipeline.edgeBurn.threshold": "Sets edge sensitivity; lower values catch more edges.",
  "pipeline.edgeBurn.darkOutline": "Adds blackened edge outlining before color is applied.",

  "pipeline.verticalSmear.enabled": "Turns CCD-style vertical readout smear on or off.",
  "pipeline.verticalSmear.strength": "Controls overall smear intensity.",
  "pipeline.verticalSmear.threshold": "Sets which bright areas start a vertical smear.",
  "pipeline.verticalSmear.decay": "Controls how slowly smear trails fade as they travel.",
  "pipeline.verticalSmear.length": "Controls how far smear trails extend.",
  "pipeline.verticalSmear.spread": "Bleeds smear into neighboring columns.",
  "pipeline.verticalSmear.contrast": "Boosts contrast inside smear trails.",
  "pipeline.verticalSmear.curtainStrength": "Adds falling curtain-like streaks independent of bright highlights.",
  "pipeline.verticalSmear.curtainDensity": "Controls how many curtain streaks start.",
  "pipeline.verticalSmear.curtainDrop": "Controls how far curtain streaks fall.",
  "pipeline.verticalSmear.jitter": "Adds column-to-column instability to smear paths.",
  "pipeline.verticalSmear.edgeBias": "Biases smear response toward detected edges.",

  "pipeline.sensorNoise.enabled": "Turns synthetic sensor noise and hot pixels on or off.",
  "pipeline.sensorNoise.amount": "Controls total noise strength.",
  "pipeline.sensorNoise.colorAmount": "Controls how colorful the noise is.",
  "pipeline.sensorNoise.shadowBias": "Pushes more noise into darker areas.",
  "pipeline.sensorNoise.striping": "Adds row and column fixed-pattern striping.",
  "pipeline.sensorNoise.hotPixels": "Adds isolated bright sensor defects.",
  "pipeline.sensorNoise.deadColumns": "Adds 1px vertical columns stuck at a single color.",
  "pipeline.sensorNoise.deadClusters": "Adds small rectangles of dead or stuck pixels.",

  "pipeline.ampGlow.enabled": "Turns thermal amplifier glow on or off.",
  "pipeline.ampGlow.strength": "Controls how bright the corner glow gets.",
  "pipeline.ampGlow.corner": "Chooses which corner glows, or lets the seed decide.",
  "pipeline.ampGlow.hue": "Blends the glow tint from purple toward hot orange.",
  "pipeline.ampGlow.spread": "Controls how far the glow creeps into the frame.",

  "pipeline.memoryFault.enabled": "Turns memory-card style row and block corruption on or off.",
  "pipeline.memoryFault.interlace": "Mixes alternating rows for interlaced field damage.",
  "pipeline.memoryFault.blockShift": "Shifts rectangular blocks out of place.",
  "pipeline.memoryFault.rowRepeat": "Repeats scanlines from nearby rows.",
  "pipeline.memoryFault.scanlineDropout": "Drops or darkens horizontal scanlines.",

  "pipeline.dctCrunch.enabled": "Turns JPEG/DCT block corruption on or off.",
  "pipeline.dctCrunch.quality": "Controls compression quality; lower values make harsher blocks.",
  "pipeline.dctCrunch.chromaSubsample": "Reduces color detail while preserving more luma structure.",
  "pipeline.dctCrunch.dcDrift": "Drifts block-average color across the scan order.",
  "pipeline.dctCrunch.acScramble": "Scrambles detail coefficients inside selected blocks.",
  "pipeline.dctCrunch.blockRepeat": "Repeats macroblocks for stuttering codec damage.",
  "pipeline.dctCrunch.generations": "Re-saves the image N times so compression damage compounds.",

  "pipeline.osdOverlay.enabled": "Turns the camera UI overlay on or off.",
  "pipeline.osdOverlay.datestamp": "Draws a seeded compact-camera date stamp.",
  "pipeline.osdOverlay.hudIcons": "Draws REC, ISO, battery, and focus marks.",
  "pipeline.osdOverlay.glitchText": "Corrupts OSD glyphs and offsets duplicate text.",
  "pipeline.osdOverlay.scale": "Controls OSD pixel size relative to the image.",
  "pipeline.osdOverlay.color": "Chooses the OSD overlay color."
};

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
      fringing: 0,
      clipColorBias: [1, 0.18, 0.86]
    },
    awbSeizure: {
      enabled: false,
      wbSwing: 0.55,
      aeSwing: 0.3,
      bandHeight: 0.3,
      frequency: 0.45
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
      deadColumns: 0,
      deadClusters: 0,
      speckleSize: 1
    },
    ampGlow: {
      enabled: false,
      strength: 0.55,
      corner: "seeded",
      hue: 0.25,
      spread: 0.5
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
      blockRepeat: 0,
      generations: 1
    },
    osdOverlay: {
      enabled: false,
      datestamp: true,
      hudIcons: true,
      glitchText: 0,
      scale: 0.5,
      color: "white"
    },
    output: {
      exportScale: 1,
      format: "png",
      preserveOriginalResolution: false
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
