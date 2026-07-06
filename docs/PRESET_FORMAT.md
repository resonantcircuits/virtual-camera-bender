# Preset Format

Presets are human-readable JSON, shared between the web app and the command-line renderer (`node src/cli.js render --preset <file>` for stills, `render-video` for clips).

The schema below matches the current implementation (`src/presets.js`). It is stable but not locked: presets missing newer fields load fine — `normalizePreset` fills in defaults — so files saved by older versions keep working.

```json
{
  "schemaVersion": 1,
  "appVersion": "0.1.0",
  "name": "Bent CCD-03",
  "cameraModel": "Bent CCD-03",
  "description": "Strong magenta/cyan clipping with medium vertical melt.",
  "tags": ["ccd", "melt", "false-color"],
  "seed": 381941,
  "createdAt": "2026-07-05T00:00:00.000Z",
  "thumbnail": {
    "type": "data-url",
    "mime": "image/webp",
    "data": ""
  },
  "exampleOutputs": [],
  "macros": {
    "bend": 0.78,
    "colorFault": 0.86,
    "melt": 0.58,
    "burn": 0.72,
    "noise": 0.42,
    "cheapness": 0.35,
    "chaos": 0.44
  },
  "temporal": {
    "mode": "locked",
    "holdFrames": 12,
    "driftAmount": 0.0,
    "driftSpeed": 0.3,
    "ghostFrame": 0
  },
  "pipeline": {
    "cheapCamera": {
      "enabled": true,
      "internalScale": 0.75,
      "blur": 0.0,
      "bitDepth": 6,
      "dither": 0.4,
      "sharpen": 0.42
    },
    "syncFault": {
      "enabled": false,
      "tearCount": 0.4,
      "tearShift": 0.4,
      "wobbleAmount": 0.25,
      "wobbleFrequency": 0.4,
      "drift": 0.35
    },
    "bayerFault": {
      "enabled": false,
      "phaseError": 1,
      "strength": 0.7,
      "zipper": 0.3
    },
    "bufferGhost": {
      "enabled": false,
      "amount": 0.5,
      "blockSize": 0.35,
      "ghostShift": 0.35,
      "ghostZoom": 0.15,
      "fieldMode": false
    },
    "colorBend": {
      "enabled": false,
      "hueRotate": 0,
      "hueStrength": 1.0,
      "channelMode": "none",
      "channelStrength": 1.0,
      "invert": "none",
      "invertStrength": 1.0,
      "solarize": 0.0
    },
    "chromaShift": {
      "enabled": false,
      "amount": 0.0,
      "angle": 0,
      "wobble": 0.2
    },
    "exposureFault": {
      "enabled": true,
      "gain": 1.24,
      "blackCrush": 0.28,
      "highlightClip": 0.71,
      "contourBands": 0.45,
      "fringing": 0.0,
      "clipColorBias": [1.0, 0.22, 0.88]
    },
    "awbSeizure": {
      "enabled": false,
      "wbSwing": 0.55,
      "aeSwing": 0.3,
      "bandHeight": 0.3,
      "frequency": 0.45
    },
    "contourRings": {
      "enabled": true,
      "strength": 0.42,
      "scale": 0.54,
      "bandSharpness": 0.62,
      "tonalBias": 0.58,
      "colorBleed": 0.5
    },
    "falseColor": {
      "enabled": true,
      "mode": "solarized-ccd",
      "strength": 0.82,
      "posterizeLevels": 7,
      "smoothness": 0.0,
      "channelSwap": 0.32,
      "hueWarp": 0.64,
      "saturation": 1.85
    },
    "gradientWash": {
      "enabled": false,
      "mode": "rainbow",
      "strength": 0.0,
      "angle": 35,
      "scale": 0.7,
      "keepLuma": 0.75,
      "wobble": 0.3
    },
    "pixelSort": {
      "enabled": false,
      "strength": 0.0,
      "threshold": 0.6,
      "window": 0.35,
      "direction": "down",
      "mode": "bright",
      "maxRun": 0.6
    },
    "edgeBurn": {
      "enabled": true,
      "strength": 0.52,
      "threshold": 0.12,
      "darkOutline": 0.45,
      "palette": ["cyan", "magenta", "green", "black"]
    },
    "verticalSmear": {
      "enabled": true,
      "strength": 0.63,
      "threshold": 0.58,
      "decay": 0.91,
      "length": 0.7,
      "spread": 0.24,
      "contrast": 0.66,
      "curtainStrength": 0.36,
      "curtainDensity": 0.34,
      "curtainDrop": 0.46,
      "jitter": 0.18,
      "edgeBias": 0.6,
      "direction": "down"
    },
    "sensorNoise": {
      "enabled": true,
      "amount": 0.31,
      "colorAmount": 0.82,
      "shadowBias": 0.67,
      "striping": 0.25,
      "hotPixels": 0.12,
      "deadColumns": 0.0,
      "deadClusters": 0.0,
      "speckleSize": 1
    },
    "ampGlow": {
      "enabled": false,
      "strength": 0.55,
      "corner": "seeded",
      "hue": 0.25,
      "spread": 0.5
    },
    "memoryFault": {
      "enabled": false,
      "interlace": 0.0,
      "blockShift": 0.0,
      "rowRepeat": 0.0,
      "scanlineDropout": 0.0
    },
    "dctCrunch": {
      "enabled": false,
      "quality": 0.55,
      "chromaSubsample": 0.6,
      "dcDrift": 0.0,
      "acScramble": 0.0,
      "blockRepeat": 0.0,
      "generations": 1
    },
    "osdOverlay": {
      "enabled": false,
      "datestamp": true,
      "hudIcons": true,
      "glitchText": 0.0,
      "scale": 0.5,
      "color": "orange"
    },
    "output": {
      "exportScale": 1.0,
      "format": "png",
      "preserveOriginalResolution": false
    }
  }
}
```

## Module Reference

- `colorBend`: whole-image color surgery — luminance-preserving `hueRotate` (degrees), hard channel reordering (`channelMode`: `gbr`, `brg`, `grb`, `bgr`, `rbg`), per-channel or full inversion (`invert`: `red`/`green`/`blue`/`all`), and `solarize` tone folding.
- `chromaShift`: spatial RGB mis-registration; red and blue planes shift in opposite directions along `angle`, with optional per-row `wobble`.
- `gradientWash`: position-driven color field (rainbow sky wash). `keepLuma` re-applies the source luminance so silhouettes survive; `wobble` warps the bands with noise.
- `pixelSort`: threshold-triggered column sorting (classic glitch drips). `window` is the luma band that triggers a run, `maxRun` caps run length as a fraction of image height.
- `falseColor.smoothness`: `0` = hard posterized bands, `1` = continuous gradient map.
- `cheapCamera.blur`: pre-effect box blur (melted plastic lens). `dither`: ordered Bayer dither strength applied during final bit-depth crunch.
- `memoryFault.interlace`: odd-row displacement in noise-gated bands with per-channel offsets (VHS/readout tearing).
- `syncFault`: timing damage, applied early. `tearCount`/`tearShift`: frame-wrap tears — below each seeded row the frame shifts sideways and wraps, with a short corrupted transition band. `wobbleAmount`/`wobbleFrequency`: per-row rolling-shutter sine displacement; `drift` adds low-frequency phase wander so verticals go wavy instead of ringing evenly.
- `bayerFault`: demosaic corruption, applied early. The image is resampled into an RGGB mosaic and demosaiced assuming the wrong grid phase (`phaseError` 0-3: 0 = correct phase, just cheap-demosaic softening; 1/2/3 = horizontal/vertical/diagonal misalignment → green/magenta checkerboards and channel swaps). `zipper` adds alternating-pixel shimmer along edges; `strength` blends with the source.
- `bufferGhost`: stale-frame ghosting, applied early. Blocks (or odd scan fields with `fieldMode`) show a "previous frame" — by default a shifted/zoomed snapshot of the image itself (`ghostShift`, `ghostZoom`, direction seeded). A different image can be supplied as the stale frame: the Ghost Source `LOAD` button in the app's Buffer Ghost panel, or `--ghost <image>` in the CLI. The ghost image is session state, not stored in the preset — presets stay portable and fall back to the self-frame.
- `exposureFault.fringing`: CCD charge-overflow blooming — a soft violet rim just outside clipped highlights (dilated highlight mask, tinted).
- `awbSeizure`: auto-WB/AE hunting mid-readout, applied after exposureFault. Horizontal bands of rows pump warm/cold (`wbSwing`) and bright/dark (`aeSwing`) down the frame — low-frequency sine plus seeded per-band jitter, with rare overshoot bands. `bandHeight` sets band size, `frequency` the oscillation rate.
- `sensorNoise.deadColumns` / `sensorNoise.deadClusters`: stuck sensor defects — 1px vertical columns (some starting partway down) and small rectangles locked to one color (hot white, dead black, or a saturated palette color), distinct from random hot pixels.
- `ampGlow`: thermal amplifier glow, applied after sensorNoise. A grainy radial glow creeping in from one corner (`corner`: `seeded` lets the seed pick, or `top-left`/`top-right`/`bottom-left`/`bottom-right`). `hue` blends the tint from purple (0) to hot orange (1); `spread` sets how far it reaches. Most visible on dark frames.
- `dctCrunch`: JPEG/DCT corruption on 8x8 blocks in YCbCr, applied late. `quality`: 1 = clean, 0 = pure block mosaic. `chromaSubsample`: force chroma toward quarter resolution while luma stays sharp. `dcDrift`: accumulating DC offset along block-scan order — color slides block-by-block into wrong hues. `acScramble`: zero/shuffle/inject AC coefficients in seeded block patches. `blockRepeat`: macroblock stutter (held blocks repeat in scan order). `generations` (1-6): re-saves the frame N times with a fresh seed and drifting parameters per pass, so damage compounds like a JPEG opened and saved repeatedly.
- `osdOverlay`: camera UI burn-in drawn last from an embedded 5x7 bitmap font. `datestamp`: seeded orange corner date (`'03 1 16` style). `hudIcons`: REC dot, ISO readout, battery, focus brackets. `glitchText`: 0-1 glyph corruption (wrong glyphs, tears, doubling). `color`: `orange`, `green`, or `white`.

Palettes for `falseColor.mode` and `gradientWash.mode`: `solarized-ccd`, `thermal-bleach`, `pink-blue`, `toxic-green`, `rainbow`, `acid-sunset`, `infrared`, `candy-shop`, `poison-dart`.

## Temporal Block (video)

The `temporal` block controls how the camera behaves across video frames (`src/temporal.js`). Still rendering ignores it entirely; presets without it get the defaults shown above.

- `mode`: how the *structural* seed evolves — `locked` (one seed for the whole clip: damage frozen in place), `hold` (seed jumps every ~`holdFrames` frames), `flicker` (fresh seed every frame). Noise textures (sensor grain, speckle, hot pixels, amp-glow grain) reroll every frame regardless of mode, so locked damage still shimmers like a live sensor; fixed-pattern striping, dead columns/clusters, and all geometric damage obey the mode.
- `holdFrames`: average hold length in frames for `hold` mode. Each hold varies ±40% (seeded), so the stutter is irregular.
- `driftAmount` (0-1): every enabled module's continuous parameters wander around their preset values on smooth seeded noise tracks — up to ±30% of each control's range at 1. Integer-stepped params (`bitDepth`, `phaseError`, `generations`, …) never drift. 0 disables drift and reproduces the preset exactly.
- `driftSpeed` (0-1): how fast the wander evolves per frame.
- `ghostFrame`: when > 0, the bufferGhost module receives the *input frame N back* as its stale frame — real inter-frame ghosting. Overrides `--ghost` and the app's loaded ghost image while active; bufferGhost must be enabled to have any effect.

Frame indices count from the start of the (possibly `--start`-trimmed) render, so a given frame's result is deterministic and independent of `--jobs`. The app previews temporal behavior honestly: contact-sheet frames and the selected working frame use their true frame indices through the same scheduling code.

## Notes

- Numeric controls generally use `0.0` to `1.0` ranges unless a physical unit or count is clearer (`hueRotate` and `angle` in degrees, `bitDepth` in bits, `posterizeLevels` as a count).
- Presets store both macro controls and expanded module controls. Macros are generators: moving a macro dial in the app recomputes the module parameters underneath it, while identity-type parameters (palette choice, hue angle, channel/sort modes) are left to the preset.
- Seeds are stored and steer every random pattern in the render (noise, drips, block glitches). Same settings + same seed = same image within one engine version, but exact pixel reproduction across future engine versions is not a hard promise.
- Thumbnails are embedded as WebP data URLs (written by the app on save). If preset files become too large, thumbnails can move to separate files later.
- The CLI can override any field at render time with `--set pipeline.<module>.<param>=<value>` and `--macro <name>=<value>`; this includes the temporal block, e.g. `--set temporal.mode=hold --set temporal.ghostFrame=4`.
