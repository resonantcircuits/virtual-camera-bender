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

// Seed behavior across video frames. "locked" keeps one seed for the whole
// clip, "hold" jumps every ~holdFrames frames, "flicker" rerolls every frame.
export const TEMPORAL_MODES = ["locked", "hold", "flicker"];

export function defaultTemporal() {
  return {
    mode: "locked",
    holdFrames: 12,
    driftAmount: 0,
    driftSpeed: 0.3,
    ghostFrame: 0
  };
}

export const ADVANCED_DEFS = [
  {
    group: "IR Cut",
    key: "irCut",
    physics: true,
    controls: [
      ["pipeline.irCut.strength", "Filter Pull", "range", 0, 1, 0.01],
      ["pipeline.irCut.spectrum", "Red Bias", "range", 0, 1, 0.01],
      ["pipeline.irCut.wood", "Foliage Glow", "range", 0, 1, 0.01],
      ["pipeline.irCut.haze", "Halation", "range", 0, 1, 0.01],
      ["pipeline.irCut.wbRed", "WB Red Gain", "range", 1, 3.5, 0.01],
      ["pipeline.irCut.wbBlue", "WB Blue Gain", "range", 1, 3.5, 0.01]
    ]
  },
  {
    group: "CCD Clock",
    key: "ccdClock",
    physics: true,
    controls: [
      ["pipeline.ccdClock.transferLoss", "Transfer Loss", "range", 0, 1, 0.01],
      ["pipeline.ccdClock.vSkip", "V-Pulse Skips", "range", 0, 1, 0.01],
      ["pipeline.ccdClock.hShear", "H Shear", "range", 0, 1, 0.01],
      ["pipeline.ccdClock.bloom", "Bloom Spill", "range", 0, 1, 0.01],
      ["pipeline.ccdClock.wbRed", "WB Red Gain", "range", 1, 3.5, 0.01],
      ["pipeline.ccdClock.wbBlue", "WB Blue Gain", "range", 1, 3.5, 0.01]
    ]
  },
  {
    group: "AFE Bend",
    key: "afeBend",
    physics: true,
    controls: [
      ["pipeline.afeBend.wave", "Waveform", "select", ["sine", "square", "saw", "noise"]],
      ["pipeline.afeBend.freq", "Frequency", "range", 0, 1, 0.01],
      ["pipeline.afeBend.skew", "Band Skew", "range", -0.5, 0.5, 0.01],
      ["pipeline.afeBend.inject", "Inject", "range", 0, 1, 0.01],
      ["pipeline.afeBend.gainMod", "Gain Wobble", "range", 0, 1, 0.01],
      ["pipeline.afeBend.wobble", "Phase Drift", "range", 0, 1, 0.01],
      ["pipeline.afeBend.cdsAmount", "CDS Ghost", "range", 0, 1, 0.01],
      ["pipeline.afeBend.cdsSkew", "CDS Lag", "range", 0, 1, 0.01],
      ["pipeline.afeBend.wbRed", "WB Red Gain", "range", 1, 3.5, 0.01],
      ["pipeline.afeBend.wbBlue", "WB Blue Gain", "range", 1, 3.5, 0.01]
    ]
  },
  {
    group: "Rail Sag",
    key: "railSag",
    physics: true,
    controls: [
      ["pipeline.railSag.sag", "Rail Droop", "range", 0, 1, 0.01],
      ["pipeline.railSag.flicker", "Flicker", "range", 0, 1, 0.01],
      ["pipeline.railSag.spikes", "Load Spikes", "range", 0, 1, 0.01],
      ["pipeline.railSag.failures", "Failures", "range", 0, 1, 0.01],
      ["pipeline.railSag.wbRed", "WB Red Gain", "range", 1, 3.5, 0.01],
      ["pipeline.railSag.wbBlue", "WB Blue Gain", "range", 1, 3.5, 0.01]
    ]
  },
  {
    group: "Bus Bend",
    key: "busBend",
    physics: true,
    controls: [
      ["pipeline.busBend.sourceMask", "Source DIP", "bits", [11, 10, 9, 8, 7, 6, 5, 4, 3, 2]],
      ["pipeline.busBend.targetMask", "Target Select", "bits", [11, 10, 9, 8, 7, 6, 5, 4]],
      ["pipeline.busBend.targetGnd", "Target GND", "boolean"],
      ["pipeline.busBend.commonBus", "Common Bus", "boolean"],
      ["pipeline.busBend.fn", "Function", "select", ["bypass", "invert", "divide"]],
      ["pipeline.busBend.pot", "Filter Pot", "range", 0, 1, 0.01],
      ["pipeline.busBend.resistance", "Series Resistance", "range", 0, 1, 0.01],
      ["pipeline.busBend.injectStrength", "Inject Strength", "range", 0, 1, 0.01],
      ["pipeline.busBend.jitter", "Comparator Noise", "range", 0, 1, 0.01],
      ["pipeline.busBend.wbRed", "WB Red Gain", "range", 1, 3.5, 0.01],
      ["pipeline.busBend.wbBlue", "WB Blue Gain", "range", 1, 3.5, 0.01]
    ]
  },
  {
    group: "Master Clock",
    key: "masterClock",
    physics: true,
    controls: [
      ["pipeline.masterClock.detune", "Clock Detune", "range", -0.5, 0.5, 0.01],
      ["pipeline.masterClock.drift", "Clock Drift", "range", 0, 1, 0.01],
      ["pipeline.masterClock.hLock", "H-Sync Lock", "range", 0, 1, 0.01],
      ["pipeline.masterClock.shred", "Bayer Shred", "range", 0, 1, 0.01],
      ["pipeline.masterClock.wbRed", "WB Red Gain", "range", 1, 3.5, 0.01],
      ["pipeline.masterClock.wbBlue", "WB Blue Gain", "range", 1, 3.5, 0.01]
    ]
  },
  {
    group: "Address Bus",
    key: "addressBus",
    physics: true,
    controls: [
      ["pipeline.addressBus.rows", "Row Lines", "range", 0, 1, 0.01],
      ["pipeline.addressBus.cols", "Column Lines", "range", 0, 1, 0.01],
      ["pipeline.addressBus.scale", "Tile Scale", "range", 0, 1, 0.01],
      ["pipeline.addressBus.lowBit", "Pixel Grind", "range", 0, 1, 0.01],
      ["pipeline.addressBus.duty", "Contact", "range", 0, 1, 0.01],
      ["pipeline.addressBus.wbRed", "WB Red Gain", "range", 1, 3.5, 0.01],
      ["pipeline.addressBus.wbBlue", "WB Blue Gain", "range", 1, 3.5, 0.01]
    ]
  },
  {
    group: "Cheap Camera",
    key: "cheapCamera",
    controls: [
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
      ["pipeline.bayerFault.phaseError", "Phase Error", "range", 0, 3, 1],
      ["pipeline.bayerFault.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.bayerFault.zipper", "Zipper Edges", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Buffer Ghost",
    key: "bufferGhost",
    controls: [
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
      ["pipeline.chromaShift.amount", "Amount", "range", 0, 1, 0.01],
      ["pipeline.chromaShift.angle", "Angle", "range", 0, 360, 1],
      ["pipeline.chromaShift.wobble", "Row Wobble", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Exposure Fault",
    key: "exposureFault",
    controls: [
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
      ["pipeline.edgeBurn.strength", "Strength", "range", 0, 1, 0.01],
      ["pipeline.edgeBurn.threshold", "Threshold", "range", 0.02, 0.45, 0.01],
      ["pipeline.edgeBurn.darkOutline", "Dark Outline", "range", 0, 1, 0.01]
    ]
  },
  {
    group: "Vertical Smear",
    key: "verticalSmear",
    controls: [
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
      ["pipeline.osdOverlay.datestamp", "Datestamp", "boolean"],
      ["pipeline.osdOverlay.hudIcons", "HUD Icons", "boolean"],
      ["pipeline.osdOverlay.glitchText", "Glitch Text", "range", 0, 1, 0.01],
      ["pipeline.osdOverlay.scale", "Scale", "range", 0, 1, 0.01],
      ["pipeline.osdOverlay.color", "Color", "select", ["orange", "green", "white"]]
    ]
  },
  {
    group: "Basic",
    key: "basicAdjustments",
    classicEdit: true,
    noDrift: true,
    controls: [
      ["pipeline.basicAdjustments.brightness", "Brightness", "range", -1, 1, 0.01],
      ["pipeline.basicAdjustments.contrast", "Contrast", "range", -1, 1, 0.01],
      ["pipeline.basicAdjustments.saturation", "Saturation", "range", -1, 1, 0.01],
      ["pipeline.basicAdjustments.vibrance", "Vibrance", "range", -1, 1, 0.01],
      ["pipeline.basicAdjustments.temperature", "Temperature", "range", -1, 1, 0.01],
      ["pipeline.basicAdjustments.tint", "Tint", "range", -1, 1, 0.01],
      ["pipeline.basicAdjustments.gamma", "Gamma", "range", 0.5, 2, 0.01],
      ["pipeline.basicAdjustments.shadows", "Shadows", "range", -1, 1, 0.01],
      ["pipeline.basicAdjustments.highlights", "Highlights", "range", -1, 1, 0.01]
    ]
  }
];

export const ADVANCED_MODULE_HELP = {
  irCut: {
    short: "Pulls the IR-cut filter so full-spectrum light reaches the sensor.",
    long: "IR Cut is the classic filter-removal mod: near-infrared floods every color channel, so foliage glows white-pink (the Wood effect), blue skies go dark and liquid, and highlights haze softly because IR focuses behind the sensor plane. It is a mood rather than a glitch — and it stacks under the other physics bends the way a converted camera would."
  },
  ccdClock: {
    short: "Corrupts the raw CCD charge-transfer timing before the image is developed.",
    long: "This is a physics-rail effect: rows can stall, skip, shear sideways, or leak highlight charge down the sensor columns. Because it happens in the raw domain, strong settings can also create Bayer phase flips and strange white-balance shifts instead of a simple post-process smear."
  },
  afeBend: {
    short: "Injects oscillator and sampling faults into the camera analog front end.",
    long: "AFE Bend acts before the ADC, where sensor voltage is amplified, sampled, and converted. It can add hum bands, moire, gain pumping, and stale correlated-double-sampling ghosts that look embossed or trailed because the reset sample is coming from the wrong pixel clock."
  },
  railSag: {
    short: "Simulates supply voltage droop, flicker, spikes, and brownout failures.",
    long: "Rail Sag makes the camera electronics run under an unstable power rail. Rows brighten, crush, or fail as the rail dips; load spikes can create short failure bursts, and high failure settings can collapse parts of the ADC into stuck colors, static, or black dropout."
  },
  busBend: {
    short: "Shorts and injects ADC data bits like a real logic-chip circuit bend.",
    long: "Bus Bend works on the raw ADC data bus, tapping source bits and driving target bits through a simple filter/comparator path. It creates hard digital bands, threshold streaks, bit-plane inversions, and contention speckle because brightness bits are being forced into the wrong logic states. Series Resistance softens the shorts into partial, speckled corruption, and Common Bus jumpers every selected pin onto one shared node so they all collide together."
  },
  masterClock: {
    short: "Reclocks the whole camera so the processor captures the pixel stream at the wrong rate.",
    long: "Master Clock emulates swapping the crystal oscillator (the LTC1799 mod). The capture rate no longer matches the sensor readout, so content stretches, lines skew, and the picture rolls vertically; the sync circuit catches and loses lock in visible tears, and heavy detune shreds color because the Bayer pattern is sampled out of phase."
  },
  addressBus: {
    short: "Sticks and bridges the frame buffer's memory address lines so the image reads back aliased.",
    long: "Address Bus corrupts the SDRAM address lines under the captured frame. Because addresses are binary, a stuck or bridged line aliases memory in exact powers of two: mirrored tiles, interleaved row pairs, ping-ponged frame halves — crystalline, deterministic repetition, nothing like random block damage. A flaky contact makes the fault come and go down the readout, tearing the frame in bands instead of folding all of it."
  },
  cheapCamera: {
    short: "Adds low-end camera processing: tiny internal scale, blur, posterization, dither, and harsh sharpening.",
    long: "This stage emulates the compromises of a weak compact sensor pipeline. The image can be softened before processing, resized through a smaller internal buffer, quantized to fewer color steps, dithered, and sharpened into brittle edges."
  },
  syncFault: {
    short: "Breaks frame timing into horizontal tears, shifts, wobble, and scan drift.",
    long: "Sync Fault treats the image like a video signal with unstable horizontal or vertical timing. It moves bands sideways, adds rolling phase wobble, and lets the timing error drift so the frame bends instead of staying perfectly rectangular."
  },
  bayerFault: {
    short: "Misreads the Bayer color mosaic before demosaic, producing wrong-color checker and zipper artifacts.",
    long: "A digital camera sensor only records one color sample at each pixel. Bayer Fault shifts or abuses that sampling phase before reconstruction, so edges pick up zippering and whole regions can resolve into unnatural red/green/blue checker patterns."
  },
  bufferGhost: {
    short: "Mixes a stale frame or second image back into the current image in blocky patches.",
    long: "Buffer Ghost imitates uncleared frame memory. It can pull from the current source, a loaded ghost image, or video ghost lag, then offset, zoom, interlace, and blend that stale buffer through coherent blocks rather than a smooth overlay."
  },
  colorBend: {
    short: "Performs direct RGB channel surgery: hue rotation, channel swaps, inversion, and solarization.",
    long: "Color Bend is the broad color wiring module. It can rotate hue, reorder channels, invert selected channels, and fold tones back on themselves with solarization, making it useful for unnatural camera color faults and aggressive palette shifts."
  },
  chromaShift: {
    short: "Offsets color channels away from each other for fringing and unstable row wobble.",
    long: "Chroma Shift splits RGB channels along a chosen direction and can vary the offset by row. Low settings create lens-like color fringes; high settings feel more like unstable analog color registration."
  },
  exposureFault: {
    short: "Damages gain, shadow crush, highlight clipping, contour bands, and clipped-edge fringing.",
    long: "Exposure Fault pushes the image through a broken auto-exposure or sensor response curve. It can lift or crush the signal, blow highlights into hard regions, draw contour steps around tonal transitions, and add purple bloom around clipped areas."
  },
  awbSeizure: {
    short: "Creates white-balance and auto-exposure hunting bands across the frame.",
    long: "AWB Seizure simulates a camera repeatedly changing its idea of neutral color and exposure while the frame is read. Bands can swing warm/cool and bright/dark, with height and frequency controlling whether the hunt feels broad or frantic."
  },
  contourRings: {
    short: "Turns brightness ranges into graphic contour bands and false-color rings.",
    long: "Contour Rings maps luminance into repeated bands, like a broken tone curve or posterized exposure response. Sharpness, scale, tonal bias, and color bleed decide whether it reads as subtle sensor banding or heavy topographic color rings."
  },
  falseColor: {
    short: "Remaps tones into synthetic camera palettes with posterization and optional channel warping.",
    long: "False Color replaces natural color with generated palettes after reducing or smoothing tonal steps. It is useful for thermal-looking mappings, toy-camera palettes, and harsh digital color modes that still follow the image's brightness structure."
  },
  gradientWash: {
    short: "Lays position-based palette bands over the image, with optional luminance preservation and wobble.",
    long: "Gradient Wash is a spatial color overlay rather than a tonal remap. It sweeps palette bands across the frame at an angle, can preserve the original lightness, and can wobble the bands with noise for less perfect, more damaged color drift."
  },
  pixelSort: {
    short: "Sorts vertical runs of pixels triggered by brightness or darkness thresholds.",
    long: "Pixel Sort finds local runs that cross the selected trigger and replaces them with sorted color values. Window size and max run keep the sorting constrained, while direction and threshold decide whether streaks fall from highlights, shadows, or dense local bands."
  },
  edgeBurn: {
    short: "Detects edges, darkens outlines, and burns colored halos into high-contrast details.",
    long: "Edge Burn exaggerates transitions in the source image. It can draw dark compact-camera outlines first, then add colored halos over detected edges, which makes texture, silhouettes, and compression-like damage feel more electrically overdriven."
  },
  verticalSmear: {
    short: "Creates CCD-style vertical streaks from highlights, curtains, edge bias, and column instability.",
    long: "Vertical Smear models readout charge leaking down sensor columns. Bright areas can trail vertically with controllable decay, spread, length, and contrast; curtain controls add independent falling streaks so the effect can move beyond highlight-only smear."
  },
  sensorNoise: {
    short: "Adds grain, color noise, striping, hot pixels, dead columns, and stuck sensor clusters.",
    long: "Sensor Noise combines random noise with fixed-pattern defects. It can bias noise into shadows, colorize it, add row/column striping, and place persistent hot or dead sensor defects that stay tied to image coordinates."
  },
  ampGlow: {
    short: "Adds thermal amplifier glow from a corner of the sensor.",
    long: "Amp Glow imitates heat or electronics leakage near the sensor edge. Strength controls brightness, corner selects the source, hue moves from cooler purple into hot orange, and spread controls how far the glow creeps into the frame."
  },
  memoryFault: {
    short: "Adds memory-card and buffer corruption: interlace, shifted blocks, repeated rows, and scanline dropout.",
    long: "Memory Fault happens after the image exists as rows and blocks. It can mix fields, displace rectangular blocks, repeat nearby scanlines, and drop or darken scanlines, so it reads more like storage or transfer damage than sensor physics."
  },
  dctCrunch: {
    short: "Corrupts JPEG-style DCT blocks with compression, chroma loss, DC drift, scrambled detail, and repeated macroblocks.",
    long: "DCT Crunch pushes the image through block-compression logic. Lower quality and chroma subsampling make normal codec damage, while DC drift, AC scramble, block repeat, and multiple generations turn it into unstable macroblock corruption."
  },
  osdOverlay: {
    short: "Draws compact-camera UI artifacts such as dates, REC marks, HUD icons, and glitched text.",
    long: "OSD Overlay adds the camera's own display layer after the image damage. It can stamp dates, battery and recording marks, focus brackets, and corrupted glyphs, with scale and color matching old consumer camera overlays."
  },
  basicAdjustments: {
    short: "Applies ordinary photo-editing adjustments after the full camera chain.",
    long: "Basic is deliberately not an emulation module. It runs at the very end as a practical classic editing pass for brightness, contrast, saturation, color temperature, tint, gamma, shadows, and highlights. Macros and random families leave it alone so you can polish the finished result."
  }
};

export const ADVANCED_CONTROL_HELP = {
  "pipeline.irCut.enabled": "Turns the IR-cut filter removal on or off (full-spectrum light reaching the sensor: glowing foliage, dark skies, soft IR haze).",
  "pipeline.irCut.strength": "How far the IR-cut filter is pulled out of the optical path: 0 seated, 1 fully removed.",
  "pipeline.irCut.spectrum": "How unevenly the color filter dyes pass infrared: low is deep NIR where every channel sees it (ghostly white), high leaks mostly into red (pink full-spectrum cast).",
  "pipeline.irCut.wood": "The Wood effect: how strongly chlorophyll reflects near-infrared. High values make foliage and greenery glow white-pink.",
  "pipeline.irCut.haze": "IR halation: infrared focuses behind the sensor plane, so the leaked light blooms softly past edges.",
  "pipeline.irCut.wbRed": "Simulated camera red white-balance gain applied when the full-spectrum raw is developed.",
  "pipeline.irCut.wbBlue": "Simulated camera blue white-balance gain applied when the full-spectrum raw is developed.",

  "pipeline.ccdClock.enabled": "Turns the charge-transfer clock bend on or off (melt, row stalls, shear bands, bloom spikes at the sensor itself).",
  "pipeline.ccdClock.transferLoss": "Charge left behind each vertical shift: bright areas wash downward, from soft bleed to paint-like drips that run most of the frame.",
  "pipeline.ccdClock.vSkip": "Skipped or doubled vertical clock pulses: readout stalls (stretched repeated rows) or jumps (compressed skips). Odd-length stalls flip the Bayer phase and color-swap everything below.",
  "pipeline.ccdClock.hShear": "Horizontal register glitches: bands of rows shear sideways with wraparound; odd offsets add rainbow demosaic fringes.",
  "pipeline.ccdClock.bloom": "Anti-blooming failure: lowers the full-well depth so clipped highlights overflow and spill vertical light spikes up and down the column.",
  "pipeline.ccdClock.wbRed": "Simulated camera red white-balance gain applied when the bent raw is developed.",
  "pipeline.ccdClock.wbBlue": "Simulated camera blue white-balance gain applied when the bent raw is developed.",

  "pipeline.afeBend.enabled": "Turns the analog-front-end bend on or off (oscillator injection, gain wobble, CDS ghosting before the ADC).",
  "pipeline.afeBend.wave": "Oscillator shape injected into the analog path: sine hums, square slams, saw sweeps, noise is sample-and-hold static.",
  "pipeline.afeBend.freq": "Oscillator frequency in cycles per sensor row (log sweep): low = rolling horizontal hum bands, high = moire against the pixel clock.",
  "pipeline.afeBend.skew": "Extra oscillator phase creep per row; tilts the interference bands.",
  "pipeline.afeBend.inject": "How much oscillator voltage is added into the pixel stream.",
  "pipeline.afeBend.gainMod": "Oscillator modulating the analog gain / ADC reference instead: exposure that pumps rather than bands that add.",
  "pipeline.afeBend.wobble": "Seeded oscillator phase drift so bands breathe and wander instead of locking.",
  "pipeline.afeBend.cdsAmount": "Correlated-double-sampling fault: the reset sample lands on a stale pixel, turning the frame into an embossed derivative with negative trails.",
  "pipeline.afeBend.cdsSkew": "How stale the CDS reset sample is, in clocks (log 1-48): trail length of the emboss ghost.",
  "pipeline.afeBend.wbRed": "Simulated camera red white-balance gain applied when the bent raw is developed.",
  "pipeline.afeBend.wbBlue": "Simulated camera blue white-balance gain applied when the bent raw is developed.",

  "pipeline.railSag.enabled": "Turns the supply-rail bend on or off (voltage droop, load spikes, and brownout row failures at the ADC).",
  "pipeline.railSag.sag": "How far the supply rail droops under load. Rows that dip below regulation bloom bright and lift their shadows before failing outright.",
  "pipeline.railSag.flicker": "Rail noise roughness: low is slow drifting brownout bands, high is jagged row-to-row flicker.",
  "pipeline.railSag.spikes": "Sudden load transients that plunge the rail for a few rows and recover — isolated failure bursts even on an otherwise healthy rail.",
  "pipeline.railSag.failures": "How eagerly sagging rows fail outright: ADC latch-up (stuck-code color bands), comparator collapse (static), or full dropout (black).",
  "pipeline.railSag.wbRed": "Simulated camera red white-balance gain applied when the bent raw is developed.",
  "pipeline.railSag.wbBlue": "Simulated camera blue white-balance gain applied when the bent raw is developed.",

  "pipeline.busBend.enabled": "Turns the physics-based ADC data-bus bend on or off.",
  "pipeline.busBend.sourceMask": "DIP switches tapping ADC output bits D11 (brightness MSB) down to D2. Multiple switches short those bits together.",
  "pipeline.busBend.targetMask": "Selects which ADC bits receive the bent signal. Multiple targets short together and fight the injection.",
  "pipeline.busBend.targetGnd": "Adds the GND position of the target selector, dragging the target node toward logic low.",
  "pipeline.busBend.commonBus": "Jumpers the source and target banks onto one shared bus so every selected pin collides together, instead of coupling through the cap and chip.",
  "pipeline.busBend.resistance": "Series protection resistor in the patch line: zero shorts pins hard to the bus, higher values let each pin mostly keep its own value with speckled partial corruption between.",
  "pipeline.busBend.fn": "Bypass passes the filtered analog signal; invert drives a logic NOT of it; divide clocks a flip-flop that halves the signal frequency.",
  "pipeline.busBend.pot": "The high-pass filter pot. Low settings clamp targets hard and pass only sharp edges; high settings let long decaying streaks through.",
  "pipeline.busBend.injectStrength": "How strongly the injected signal wins bus contention against the ADC's own pin drivers.",
  "pipeline.busBend.jitter": "Analog comparator noise; breaks contended-voltage ties into speckle.",
  "pipeline.busBend.wbRed": "Simulated camera red white-balance gain applied after the corrupted raw is developed.",
  "pipeline.busBend.wbBlue": "Simulated camera blue white-balance gain applied after the corrupted raw is developed.",

  "pipeline.masterClock.enabled": "Turns the master-clock reclock bend on or off (wrong capture rate: stretch, skew, roll, sync tears).",
  "pipeline.masterClock.detune": "How far the swapped oscillator sits from the correct rate. Negative underclocks, positive overclocks; small values skew, large values roll and shred color.",
  "pipeline.masterClock.drift": "Slow wobble of the clock rate: wavy tape-style skew that breathes instead of holding a straight lean.",
  "pipeline.masterClock.hLock": "How strongly the sync circuit pulls lines back into phase. High values hold the picture with occasional snap tears; low values let it roll freely.",
  "pipeline.masterClock.shred": "Fraction of rows whose sample clock slips at pixel level: those lines capture the color mosaic out of phase and develop into colored static.",
  "pipeline.masterClock.wbRed": "Simulated camera red white-balance gain applied when the bent raw is developed.",
  "pipeline.masterClock.wbBlue": "Simulated camera blue white-balance gain applied when the bent raw is developed.",

  "pipeline.addressBus.enabled": "Turns the frame-buffer address-line bend on or off (mirrored tiles, interleaved rows, ping-ponged halves).",
  "pipeline.addressBus.rows": "How badly the row address lines are damaged: more faults fold, interleave, and repeat horizontal bands of the frame.",
  "pipeline.addressBus.cols": "How badly the column address lines are damaged: more faults mirror and interleave the frame side to side.",
  "pipeline.addressBus.scale": "Which address lines the faults land on: low hits the fine lines (pixel-pair and small-tile interleaves), high folds whole halves of the frame.",
  "pipeline.addressBus.lowBit": "Chance a fault lands on the lowest address line — the only one that breaks the color mosaic, so aliased regions develop color-swapped.",
  "pipeline.addressBus.duty": "How much of the readout the bad contact is actually failing: 1 is a solid short folding the whole frame, lower values tear in and out in bands.",
  "pipeline.addressBus.wbRed": "Simulated camera red white-balance gain applied when the bent raw is developed.",
  "pipeline.addressBus.wbBlue": "Simulated camera blue white-balance gain applied when the bent raw is developed.",

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
  "pipeline.osdOverlay.color": "Chooses the OSD overlay color.",

  "pipeline.basicAdjustments.enabled": "Turns classic photo adjustments on or off. This is plain editing, not camera damage.",
  "pipeline.basicAdjustments.brightness": "Raises or lowers the final image brightness after every effect has rendered.",
  "pipeline.basicAdjustments.contrast": "Expands or compresses contrast around mid-gray in the finished image.",
  "pipeline.basicAdjustments.saturation": "Globally reduces color toward grayscale or boosts color intensity.",
  "pipeline.basicAdjustments.vibrance": "Boosts muted colors more than already-saturated ones, or gently mutes color when negative.",
  "pipeline.basicAdjustments.temperature": "Warms the result toward red/yellow or cools it toward blue.",
  "pipeline.basicAdjustments.tint": "Pushes the result toward magenta when positive or green when negative.",
  "pipeline.basicAdjustments.gamma": "Adjusts midtone brightness without moving pure black and white as much.",
  "pipeline.basicAdjustments.shadows": "Lifts or deepens the darker parts of the finished image.",
  "pipeline.basicAdjustments.highlights": "Recovers or brightens the lighter parts of the finished image."
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

export function defaultPipeline() {
  return {
    irCut: {
      enabled: false,
      strength: 0.6,
      spectrum: 0.6,
      wood: 0.5,
      haze: 0.35,
      wbRed: 2,
      wbBlue: 1.5
    },
    ccdClock: {
      enabled: false,
      transferLoss: 0.35,
      vSkip: 0.2,
      hShear: 0.25,
      bloom: 0.2,
      wbRed: 2,
      wbBlue: 1.5
    },
    afeBend: {
      enabled: false,
      wave: "sine",
      freq: 0.18,
      skew: 0,
      inject: 0.35,
      gainMod: 0,
      wobble: 0.1,
      cdsAmount: 0,
      cdsSkew: 0.3,
      wbRed: 2,
      wbBlue: 1.5
    },
    railSag: {
      enabled: false,
      sag: 0.45,
      flicker: 0.35,
      spikes: 0.25,
      failures: 0.35,
      wbRed: 2,
      wbBlue: 1.5
    },
    busBend: {
      enabled: false,
      sourceMask: 0,
      targetMask: 0,
      targetGnd: false,
      commonBus: false,
      fn: "bypass",
      pot: 0.5,
      resistance: 0,
      injectStrength: 0.55,
      jitter: 0.08,
      wbRed: 2,
      wbBlue: 1.5
    },
    masterClock: {
      enabled: false,
      detune: 0.08,
      drift: 0.3,
      hLock: 0.6,
      shred: 0.2,
      wbRed: 2,
      wbBlue: 1.5
    },
    addressBus: {
      enabled: false,
      rows: 0.55,
      cols: 0.3,
      scale: 0.55,
      lowBit: 0.15,
      duty: 0.85,
      wbRed: 2,
      wbBlue: 1.5
    },
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
    basicAdjustments: {
      enabled: true,
      brightness: 0,
      contrast: 0,
      saturation: 0,
      vibrance: 0,
      temperature: 0,
      tint: 0,
      gamma: 1,
      shadows: 0,
      highlights: 0
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
  pipeline = {},
  temporal = {}
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
    temporal: { ...defaultTemporal(), ...temporal },
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
    pipeline: input?.pipeline || {},
    temporal: input?.temporal || {}
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
