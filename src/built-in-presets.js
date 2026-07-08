import { createPreset } from "./presets.js";

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
    name: "Dark Frame Leak",
    description: "Long-exposure amp glow creeping from a corner over stuck columns and hot speckle.",
    tags: ["amp-glow", "long-exposure", "defects"],
    seed: 941773,
    macros: {
      bend: 0.34,
      colorFault: 0.3,
      melt: 0.04,
      burn: 0.46,
      noise: 0.62,
      cheapness: 0.3,
      chaos: 0.16
    },
    pipeline: {
      ampGlow: { enabled: true, strength: 0.72, corner: "seeded", hue: 0.18, spread: 0.6 },
      exposureFault: { gain: 1.3, blackCrush: 0.55, highlightClip: 0.4, clipColorBias: [1, 0.5, 0.9] },
      sensorNoise: {
        amount: 0.42,
        colorAmount: 0.9,
        shadowBias: 0.85,
        striping: 0.14,
        hotPixels: 0.42,
        deadColumns: 0.3,
        deadClusters: 0.22
      },
      falseColor: { strength: 0.24, smoothness: 0.6, saturation: 1.5 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false }
    }
  }),
  createPreset({
    name: "AWB Panic",
    description: "White balance and exposure hunting mid-readout: warm/cold bands pumping down the frame.",
    tags: ["awb", "bands", "hunting"],
    seed: 517209,
    macros: {
      bend: 0.36,
      colorFault: 0.34,
      melt: 0.04,
      burn: 0.32,
      noise: 0.34,
      cheapness: 0.38,
      chaos: 0.2
    },
    pipeline: {
      awbSeizure: { enabled: true, wbSwing: 0.82, aeSwing: 0.48, bandHeight: 0.38, frequency: 0.58 },
      exposureFault: { gain: 1.18, blackCrush: 0.18, highlightClip: 0.4, fringing: 0.35 },
      falseColor: { strength: 0.2, smoothness: 0.65, saturation: 1.45 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { strength: 0.14 },
      sensorNoise: { amount: 0.22, colorAmount: 0.7, striping: 0.1 }
    }
  }),
  createPreset({
    name: "Copy Of A Copy",
    description: "Saved and reopened until it rots: five generations of recompression with violet-fringed clipping.",
    tags: ["jpeg", "generations", "recompression"],
    seed: 662450,
    macros: {
      bend: 0.3,
      colorFault: 0.26,
      melt: 0.02,
      burn: 0.36,
      noise: 0.24,
      cheapness: 0.6,
      chaos: 0.24
    },
    pipeline: {
      dctCrunch: {
        enabled: true,
        quality: 0.58,
        chromaSubsample: 0.7,
        dcDrift: 0.16,
        acScramble: 0.14,
        blockRepeat: 0.08,
        generations: 5
      },
      cheapCamera: { internalScale: 0.9, bitDepth: 7, dither: 0.2, sharpen: 0.6 },
      exposureFault: { gain: 1.2, blackCrush: 0.2, highlightClip: 0.45, fringing: 0.4 },
      falseColor: { strength: 0.16, smoothness: 0.7, saturation: 1.35 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false },
      sensorNoise: { amount: 0.14, colorAmount: 0.55, striping: 0.05 }
    }
  }),
  createPreset({
    name: "Chrome Spill",
    description: "Solarized metal: folded tones with drifting teal hue and hard tone bands.",
    tags: ["solarize", "chrome", "fold"],
    seed: 128733,
    macros: {
      bend: 0.45,
      colorFault: 0.3,
      melt: 0.06,
      burn: 0.5,
      noise: 0.3,
      cheapness: 0.25,
      chaos: 0.15
    },
    pipeline: {
      colorBend: { enabled: true, hueRotate: 210, hueStrength: 0.7, solarize: 0.7 },
      falseColor: { mode: "pink-blue", strength: 0.3, smoothness: 0.85, posterizeLevels: 10, channelSwap: 0, hueWarp: 0.1, saturation: 2 },
      contourRings: { enabled: true, strength: 0.65, scale: 0.5, bandSharpness: 0.75, tonalBias: 0.45, colorBleed: 0.5 },
      exposureFault: { gain: 1.7, blackCrush: 0.2, highlightClip: 0.6 },
      chromaShift: { enabled: true, amount: 0.15, angle: 90, wobble: 0.2 },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { strength: 0.3 },
      sensorNoise: { amount: 0.2, colorAmount: 0.6 }
    }
  }),
  createPreset({
    name: "Field Echo",
    description: "Interlaced stale-frame echo: alternating scan fields hold a shifted ghost.",
    tags: ["ghost", "interlace", "fields"],
    seed: 559023,
    macros: {
      bend: 0.35,
      colorFault: 0.3,
      melt: 0.05,
      burn: 0.3,
      noise: 0.3,
      cheapness: 0.35,
      chaos: 0.3
    },
    pipeline: {
      bufferGhost: {
        enabled: true,
        amount: 0.8,
        blockSize: 0.2,
        ghostShift: 0.6,
        ghostZoom: 0.35,
        fieldMode: true
      },
      chromaShift: { enabled: true, amount: 0.3, angle: 0, wobble: 0.45 },
      memoryFault: { enabled: true, interlace: 0.4, blockShift: 0, rowRepeat: 0, scanlineDropout: 0 },
      falseColor: { strength: 0.25, smoothness: 0.6, saturation: 1.5 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      sensorNoise: { amount: 0.2, striping: 0.15 }
    }
  }),
  createPreset({
    name: "Gravity Leak",
    description: "Shadows sorted upward: dark regions rise out of the frame in pink-blue haze.",
    tags: ["sort", "up", "shadows"],
    seed: 903311,
    macros: {
      bend: 0.4,
      colorFault: 0.5,
      melt: 0.6,
      burn: 0.3,
      noise: 0.35,
      cheapness: 0.25,
      chaos: 0.2
    },
    pipeline: {
      pixelSort: {
        enabled: true,
        strength: 0.9,
        threshold: 0.35,
        window: 0.45,
        direction: "up",
        mode: "dark",
        maxRun: 0.8
      },
      falseColor: { mode: "pink-blue", strength: 0.7, smoothness: 0.65, saturation: 1.8, posterizeLevels: 9 },
      verticalSmear: { enabled: false },
      contourRings: { enabled: false },
      edgeBurn: { strength: 0.18 },
      sensorNoise: { amount: 0.28, colorAmount: 0.8 }
    }
  }),
  createPreset({
    name: "Ember Core",
    description: "Hot amplifier glow devouring a corner, crushed blacks and red-orange clipping.",
    tags: ["amp-glow", "heat", "dark"],
    seed: 337719,
    macros: {
      bend: 0.35,
      colorFault: 0.3,
      melt: 0.05,
      burn: 0.6,
      noise: 0.5,
      cheapness: 0.3,
      chaos: 0.1
    },
    pipeline: {
      ampGlow: { enabled: true, strength: 0.95, corner: "bottom-right", hue: 0.95, spread: 0.85 },
      exposureFault: { gain: 1.35, blackCrush: 0.6, highlightClip: 0.55, clipColorBias: [1, 0.45, 0.1] },
      falseColor: { mode: "thermal-bleach", strength: 0.3, smoothness: 0.7, saturation: 1.6 },
      sensorNoise: { amount: 0.45, colorAmount: 0.7, shadowBias: 0.9, hotPixels: 0.5 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false }
    }
  }),
  createPreset({
    name: "Broken Panel",
    description: "Stuck columns, dead pixel clusters, and dropped scanlines on a failing display.",
    tags: ["defects", "columns", "panel"],
    seed: 774092,
    macros: {
      bend: 0.3,
      colorFault: 0.25,
      melt: 0.03,
      burn: 0.3,
      noise: 0.5,
      cheapness: 0.4,
      chaos: 0.35
    },
    pipeline: {
      sensorNoise: {
        amount: 0.3,
        colorAmount: 0.6,
        shadowBias: 0.5,
        striping: 0.6,
        hotPixels: 0.2,
        deadColumns: 0.85,
        deadClusters: 0.55
      },
      memoryFault: { enabled: true, interlace: 0, blockShift: 0.08, rowRepeat: 0.2, scanlineDropout: 0.35 },
      chromaShift: { enabled: true, amount: 0.12, angle: 0, wobble: 0.25 },
      falseColor: { strength: 0.2, smoothness: 0.6, saturation: 1.4 },
      exposureFault: { gain: 1.15, blackCrush: 0.2, highlightClip: 0.35 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false }
    }
  }),
  createPreset({
    name: "Tape Nausea",
    description: "Seasick playback: heavy rolling-shutter waves, split chroma, hunting exposure.",
    tags: ["vhs", "wobble", "tracking"],
    seed: 481560,
    macros: {
      bend: 0.35,
      colorFault: 0.35,
      melt: 0.1,
      burn: 0.3,
      noise: 0.4,
      cheapness: 0.45,
      chaos: 0.5
    },
    pipeline: {
      syncFault: {
        enabled: true,
        tearCount: 0.25,
        tearShift: 0.3,
        wobbleAmount: 0.65,
        wobbleFrequency: 0.6,
        drift: 0.8
      },
      chromaShift: { enabled: true, amount: 0.35, angle: 0, wobble: 0.6 },
      memoryFault: { enabled: true, interlace: 0.5, blockShift: 0, rowRepeat: 0.08, scanlineDropout: 0.12 },
      awbSeizure: { enabled: true, wbSwing: 0.35, aeSwing: 0.25, bandHeight: 0.5, frequency: 0.3 },
      cheapCamera: { blur: 0.3 },
      falseColor: { strength: 0.3, smoothness: 0.7, saturation: 1.3 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      dctCrunch: { enabled: false },
      edgeBurn: { enabled: false }
    }
  }),
  createPreset({
    name: "Xerox Heat",
    description: "Four-tone thermal print with brutal black outlines and oversharpened grain.",
    tags: ["print", "posterize", "outline"],
    seed: 215404,
    macros: {
      bend: 0.4,
      colorFault: 0.7,
      melt: 0.04,
      burn: 0.6,
      noise: 0.25,
      cheapness: 0.4,
      chaos: 0.1
    },
    pipeline: {
      falseColor: {
        mode: "thermal-bleach",
        strength: 0.95,
        posterizeLevels: 4,
        smoothness: 0,
        channelSwap: 0,
        hueWarp: 0.15,
        saturation: 2.2
      },
      edgeBurn: { enabled: true, strength: 0.8, threshold: 0.08, darkOutline: 0.9, palette: ["black", "white", "magenta"] },
      exposureFault: { gain: 1.3, blackCrush: 0.4, highlightClip: 0.6, contourBands: 0.7 },
      cheapCamera: { internalScale: 0.7, bitDepth: 5, dither: 0.6, sharpen: 0.7 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      sensorNoise: { amount: 0.2, colorAmount: 0.4 }
    }
  }),
  createPreset({
    name: "Oil Slick '99",
    description: "Tight iridescent horizontal bands warping over preserved shadows.",
    tags: ["iridescent", "wash", "bands"],
    seed: 660242,
    macros: {
      bend: 0.35,
      colorFault: 0.45,
      melt: 0.04,
      burn: 0.35,
      noise: 0.3,
      cheapness: 0.25,
      chaos: 0.1
    },
    pipeline: {
      gradientWash: {
        enabled: true,
        mode: "acid-sunset",
        strength: 0.75,
        angle: 90,
        scale: 0.22,
        keepLuma: 0.85,
        wobble: 0.95
      },
      colorBend: { enabled: true, hueRotate: 0, hueStrength: 0, solarize: 0.35 },
      falseColor: { enabled: false },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { strength: 0.2 },
      sensorNoise: { amount: 0.25, colorAmount: 0.7 }
    }
  }),
  createPreset({
    name: "Pocket 4-Bit",
    description: "Handheld console camera: sixteen colors, chunky pixels, heavy dither.",
    tags: ["lowres", "dither", "4bit"],
    seed: 88190,
    macros: {
      bend: 0.3,
      colorFault: 0.3,
      melt: 0.02,
      burn: 0.3,
      noise: 0.35,
      cheapness: 0.9,
      chaos: 0.08
    },
    pipeline: {
      cheapCamera: { internalScale: 0.3, blur: 0.15, bitDepth: 3, dither: 0.95, sharpen: 0.3 },
      falseColor: { mode: "toxic-green", strength: 0.55, posterizeLevels: 5, smoothness: 0.2, saturation: 1.3 },
      dctCrunch: { enabled: false },
      exposureFault: { gain: 1.2, blackCrush: 0.25, highlightClip: 0.4 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false },
      sensorNoise: { amount: 0.3, colorAmount: 0.3 }
    }
  }),
  createPreset({
    name: "Hue Avalanche",
    description: "DC drift landslide: block colors sliding into the wrong hues down the scan.",
    tags: ["dct", "drift", "blocks"],
    seed: 412873,
    macros: {
      bend: 0.3,
      colorFault: 0.3,
      melt: 0.03,
      burn: 0.3,
      noise: 0.25,
      cheapness: 0.55,
      chaos: 0.45
    },
    pipeline: {
      dctCrunch: {
        enabled: true,
        quality: 0.3,
        chromaSubsample: 0.7,
        dcDrift: 0.95,
        acScramble: 0.35,
        blockRepeat: 0.35,
        generations: 2
      },
      falseColor: { strength: 0.25, smoothness: 0.5, saturation: 1.6 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false },
      memoryFault: { enabled: false },
      sensorNoise: { amount: 0.15, striping: 0.05 }
    }
  }),
  createPreset({
    name: "Deep Fried Upload",
    description: "Reposted to death: nuclear gain, crispy sharpening, four generations of JPEG.",
    tags: ["deepfry", "jpeg", "saturation"],
    seed: 692035,
    macros: {
      bend: 0.4,
      colorFault: 0.6,
      melt: 0.04,
      burn: 0.85,
      noise: 0.5,
      cheapness: 0.7,
      chaos: 0.3
    },
    pipeline: {
      exposureFault: { gain: 1.9, blackCrush: 0.5, highlightClip: 0.85, contourBands: 0.6, fringing: 0.6, clipColorBias: [1, 0.55, 0.1] },
      falseColor: { mode: "acid-sunset", strength: 0.38, smoothness: 0.85, saturation: 2.5, posterizeLevels: 10, channelSwap: 0, hueWarp: 0.12 },
      dctCrunch: {
        enabled: true,
        quality: 0.2,
        chromaSubsample: 0.9,
        dcDrift: 0.08,
        acScramble: 0.06,
        blockRepeat: 0.1,
        generations: 4
      },
      cheapCamera: { internalScale: 0.75, bitDepth: 6, dither: 0.5, sharpen: 1 },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { strength: 0.5, darkOutline: 0.2 },
      sensorNoise: { amount: 0.35, colorAmount: 0.8 }
    }
  }),
  createPreset({
    name: "Prism Fever",
    description: "Rainbow-mapped tones with contour rings bleeding full spectral color.",
    tags: ["rainbow", "rings", "spectral"],
    seed: 150377,
    macros: {
      bend: 0.4,
      colorFault: 0.8,
      melt: 0.05,
      burn: 0.5,
      noise: 0.3,
      cheapness: 0.25,
      chaos: 0.2
    },
    pipeline: {
      falseColor: { mode: "rainbow", strength: 0.68, posterizeLevels: 11, smoothness: 0.7, hueWarp: 0.35, saturation: 2.2 },
      contourRings: { enabled: true, strength: 0.9, scale: 0.8, bandSharpness: 0.85, tonalBias: 0.45, colorBleed: 0.95 },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { strength: 0.25 },
      sensorNoise: { amount: 0.3, colorAmount: 0.9 }
    }
  }),
  createPreset({
    name: "Almost Fine",
    description: "Plausibly bad: faint chroma split, mild compression, gentle noise. Nothing you could return it for.",
    tags: ["subtle", "plausible", "worn"],
    seed: 271828,
    macros: {
      bend: 0.15,
      colorFault: 0.1,
      melt: 0.02,
      burn: 0.18,
      noise: 0.22,
      cheapness: 0.35,
      chaos: 0.06
    },
    pipeline: {
      chromaShift: { enabled: true, amount: 0.08, angle: 10, wobble: 0.15 },
      dctCrunch: { enabled: true, quality: 0.62, chromaSubsample: 0.5, dcDrift: 0, acScramble: 0, blockRepeat: 0, generations: 2 },
      falseColor: { enabled: false },
      contourRings: { enabled: false },
      verticalSmear: { enabled: false },
      pixelSort: { enabled: false },
      edgeBurn: { enabled: false },
      exposureFault: { gain: 1.12, blackCrush: 0.12, highlightClip: 0.25, contourBands: 0.1 },
      sensorNoise: { amount: 0.18, colorAmount: 0.5, shadowBias: 0.6, striping: 0.04 }
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
  }),
  // --- busBend physics presets: simulated logic-chip bend on the ADC data
  // bus (PowerShot A520 source/target selector bend). All other plausibility
  // modules stay off or minimal so the circuit's own artifacts carry the look.
  createPreset({
    name: "Rainbow Bus Tap",
    description: "Low ADC bits filtered into the brightness MSBs: banded psychedelic contour fields.",
    tags: ["bus-bend", "physics", "hpf", "contour"],
    seed: 52011,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      busBend: { enabled: true, sourceMask: 96, targetMask: 2560, fn: "bypass", pot: 0.45 },
      dctCrunch: { enabled: true, quality: 0.72, chromaSubsample: 0.45, generations: 1 }
    }
  }),
  createPreset({
    name: "Logic Negative",
    description: "A NAND gate inverts the bus edges into the mid bits: mauve posterized negatives.",
    tags: ["bus-bend", "physics", "invert", "negative"],
    seed: 52021,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      busBend: { enabled: true, sourceMask: 3072, targetMask: 896, fn: "invert", pot: 0.5 },
      dctCrunch: { enabled: true, quality: 0.72, chromaSubsample: 0.45, generations: 1 }
    }
  }),
  createPreset({
    name: "Divide By Two",
    description: "A flip-flop halves the bus frequency back onto its own pins: chaotic shifting scanlines.",
    tags: ["bus-bend", "physics", "divide", "scanline"],
    seed: 52031,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      busBend: { enabled: true, sourceMask: 960, targetMask: 960, fn: "divide", pot: 0.5 },
      dctCrunch: { enabled: true, quality: 0.72, chromaSubsample: 0.45, generations: 1 }
    },
    temporal: { mode: "flicker" }
  }),
  createPreset({
    name: "Ground Loop",
    description: "Mains hum injected into the analog path: rolling horizontal interference bands.",
    tags: ["afe-bend", "physics", "hum", "bands"],
    seed: 52051,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      afeBend: { enabled: true, wave: "sine", freq: 0.12, inject: 0.32, wobble: 0.3 },
      dctCrunch: { enabled: true, quality: 0.74, chromaSubsample: 0.4, generations: 1 }
    },
    temporal: { driftAmount: 0.18, driftSpeed: 0.5 }
  }),
  createPreset({
    name: "Carrier Clash",
    description: "A square-wave carrier beating against the pixel clock: tilted moire and pumping gain.",
    tags: ["afe-bend", "physics", "moire", "oscillator"],
    seed: 52061,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      afeBend: { enabled: true, wave: "square", freq: 0.72, skew: 0.18, inject: 0.3, gainMod: 0.25, wobble: 0.12 },
      dctCrunch: { enabled: true, quality: 0.72, chromaSubsample: 0.45, generations: 1 }
    }
  }),
  createPreset({
    name: "Reset Ghost",
    description: "CDS sampling the wrong pixel: the frame collapses into an embossed derivative with negative trails.",
    tags: ["afe-bend", "physics", "cds", "emboss"],
    seed: 52071,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      afeBend: { enabled: true, inject: 0, cdsAmount: 0.85, cdsSkew: 0.45 },
      dctCrunch: { enabled: true, quality: 0.76, chromaSubsample: 0.4, generations: 1 }
    }
  }),
  createPreset({
    name: "Cloud Solarizer",
    description: "One MSB gently leaks into the next bit down: pastel solarized skies, clean midtones.",
    tags: ["bus-bend", "physics", "solarize", "subtle"],
    seed: 52041,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      busBend: { enabled: true, sourceMask: 2048, targetMask: 1024, fn: "bypass", pot: 0.7 },
      dctCrunch: { enabled: true, quality: 0.78, chromaSubsample: 0.4, generations: 1 }
    }
  }),
  createPreset({
    name: "Brownout",
    description: "Supply rail sagging under load: broad breathing exposure bands with sparse row failures.",
    tags: ["rail-sag", "physics", "brownout", "bands"],
    seed: 71263,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      railSag: { enabled: true, sag: 0.62, flicker: 0.25, spikes: 0.35, failures: 0.3 },
      dctCrunch: { enabled: true, quality: 0.74, chromaSubsample: 0.4, generations: 1 }
    },
    temporal: { driftAmount: 0.16, driftSpeed: 0.4 }
  }),
  createPreset({
    name: "A540 Melt",
    description: "Vertical transfer clocks failing mid-readout: paint-drip melts, luminance streaks, stalled row stretches.",
    tags: ["ccd-clock", "physics", "melt", "streaks"],
    seed: 27551,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      ccdClock: { enabled: true, transferLoss: 0.45, vSkip: 0.5, hShear: 0, bloom: 0.35 },
      dctCrunch: { enabled: true, quality: 0.74, chromaSubsample: 0.4, generations: 1 }
    },
    temporal: { driftAmount: 0.15, driftSpeed: 0.35 }
  }),
  createPreset({
    name: "A530 Warp",
    description: "Horizontal register glitches: sheared scanline bands, Bayer phase tears, rainbow fringes.",
    tags: ["ccd-clock", "physics", "shear", "phase"],
    seed: 41893,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      ccdClock: { enabled: true, transferLoss: 0.12, vSkip: 0.22, hShear: 0.6, bloom: 0 },
      dctCrunch: { enabled: true, quality: 0.74, chromaSubsample: 0.4, generations: 1 }
    },
    temporal: { driftAmount: 0.12, driftSpeed: 0.45 }
  }),
  createPreset({
    name: "Chromatic Sieve",
    description: "ADC latch-up under a failing rail: stuck-code color bands, static rows, black dropouts.",
    tags: ["rail-sag", "physics", "latch-up", "dropout"],
    seed: 52061,
    macros: { bend: 0, colorFault: 0, melt: 0, burn: 0, noise: 0, cheapness: 0, chaos: 0 },
    pipeline: {
      railSag: { enabled: true, sag: 0.5, flicker: 0.55, spikes: 0.5, failures: 0.85 },
      dctCrunch: { enabled: true, quality: 0.7, chromaSubsample: 0.45, generations: 1 }
    },
    temporal: { driftAmount: 0.12, driftSpeed: 0.5 }
  })
];
