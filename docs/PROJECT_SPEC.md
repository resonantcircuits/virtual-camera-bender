# Project Spec

## Product Goal

Build a web-based still-image editor that turns normal input images into convincing circuit-bent early-2000s compact digital camera images.

The output should usually remain somewhat recognizable, but can be fairly destructive. The tool should make it easy to discover, save, tweak, and export strong glitch looks.

The long-term product can support video; the companion command-line tool already exists (`src/cli.js`) and shares the still-image engine.

## Creative Target

The local, uncommitted reference images in `reference-images/` point to a specific class of digital camera failure:

- saturated false color instead of natural color
- hard clipping and blown highlights
- cyan, magenta, green, yellow, red, and blue channel damage
- posterized tonal regions and contour bands
- vertical readout streaks, especially from highlights and edges
- dense colored sensor noise in skies, shadows, and gradients
- black crushed silhouettes that preserve scene structure
- occasional buffer, scanline, or block corruption

This is not generic datamoshing or VHS damage. Effects should feel plausibly camera-born: sensor failure, CCD/CMOS readout damage, cheap image processing, broken color response, corrupted buffers, and low-end compression.

## Non-Goals For The First Version

- live webcam input
- OBS or system-level virtual camera output
- real-time video performance
- node-graph editing
- physically accurate electronics simulation
- pixel-perfect deterministic rendering across all future versions

## MVP Scope

All items below are implemented. The first working version includes:

- upload a single source image
- preview the processed result
- export the processed image at original resolution by default
- optionally downscale/upscale as an effect
- randomize by effect family
- global randomize across all families
- save and load JSON presets
- include preset metadata, notes, tags, thumbnail, seed, and example output references
- ship with a small set of fictional camera presets
- expose simple macro controls by default
- hide dense per-module controls in an advanced panel

## Interface Direction

The UI should visually reference old compact digital camera menus, but functionality is more important than strict skeuomorphism.

Default controls should be broad and fast to understand:

- `Bend`: overall strength
- `Color Fault`: broken color response
- `Melt`: vertical smear and pixel dragging
- `Burn`: clipping and highlight overload
- `Noise`: sensor grain, speckle, and striping
- `Cheapness`: resolution, bit depth, sharpening, compression feel
- `Chaos`: breadth and severity of random faults

Advanced controls should expose the individual circuit modules after the core workflow is usable.

## Effect Families

### False Color / Broken CCD Palette

Aggressive channel remapping, solarization, posterization, and neon camera response. This is a primary part of the reference look and should work even without geometric corruption.

### Highlight Overload

Blown highlights should turn into hard clipped color regions, rings, halos, contour bands, or white cores with colored edges.

### Vertical Readout Smear

Column streaks should drag information downward from highlights, edges, silhouettes, trees, clouds, and other high-contrast features. This is one of the most important circuit-bent camera signatures.

### Sensor Noise / Dither

Dense colored speckle should appear in smooth regions and dark areas. It should be controllable enough to avoid simply looking like generic film grain.

### Contour / Edge Burn

Edges should pick up cyan, magenta, green, black, or white outlines. This preserves readability while still making the image feel broken.

### Cheap Camera Mode

Optional downscale, nearest or soft upscale, bit-depth reduction, harsh sharpening, low-end demosaic feel, and compression-like color breakup.

### Memory Faults

Block offsets, repeated rows, scanline corruption, missing chunks, buffer-region swaps, and row interlacing with per-channel offsets. This should be available but not dominate every default preset.

### Color Bend (implemented later than the original spec)

Whole-image color surgery: luminance-preserving hue rotation, hard channel reordering, per-channel or full inversion, and solarize tone folding. Added because many reference images are essentially hue-rotated or channel-inverted photos.

### Gradient Wash (implemented later than the original spec)

Position-driven color fields (diagonal rainbow bands over skies) with luminance preservation, matching the smooth rainbow-wash reference images.

### Pixel Sort (implemented later than the original spec)

Threshold-triggered column sorting for the classic glitch-drip signature, distinct from the softer vertical readout smear.

### Chroma Shift (implemented later than the original spec)

Spatial RGB mis-registration fringes with per-row wobble.

## Processing Pipeline (as implemented)

The engine (`src/engine-core.js`) runs a fixed camera-circuit pipeline:

```text
Input image
-> cheap-camera downscale + lens blur
-> chroma shift
-> exposure and clipping fault
-> color bend (hue rotate / channel swap / invert / solarize)
-> contour rings
-> false color (posterized or smooth gradient map, 9 palettes)
-> gradient wash
-> edge burn
-> pixel sort
-> vertical smear
-> sensor noise / hot pixels / striping
-> memory and scanline faults (interlace, block shift, row repeat, dropout)
-> final bit crush + dither + sharpen
-> export
```

This gives the project a camera-circuit metaphor without requiring a complex node editor.

The effect engine is separate from the UI: presets drive the engine, and the same engine runs in the web app's render worker and the CLI.

## Technical Direction

Stack as built (deviating from the original TypeScript/WebGL preference in favor of simplicity):

- plain JavaScript ES modules, no build step
- CPU image-buffer passes over `ImageData`, running in a Web Worker in the browser
- the same pure engine module shared by the web app, the worker, and the Node CLI (ffmpeg for decode/encode)
- JSON preset format from day one

WebGL remains a possible future optimization if CPU passes become the bottleneck, but the current worker-based pipeline keeps the UI responsive.

Original-resolution export is the default. The preview uses a lower internal resolution (max 1500 px) for responsiveness; export re-renders at full source resolution.

Video support can later apply the same engine per frame. To avoid unusable flicker, video mode should eventually support stable seeds, temporal smoothing, or slowly evolving parameters.

## Randomization Model

Randomization happens at three levels (all implemented in `src/randomize.js`):

- global randomize: builds a whole new camera — fresh macros, a fresh pick of active modules, new name and seed. Each roll mutes some damage channels so results have distinct characters instead of everything firing at once.
- family randomizers (`Color`, `Melt`, `Burn`, `Noise`, `Cheap`, `Memory`): re-roll one damage domain with fresh draws, leave the rest untouched.
- per-module dice (`R` in each Advanced Circuit group header): re-roll one module's parameters while keeping the global seed, so everything else renders identically.

All draws are fresh values within the mode's intensity band — never cumulative — so repeated presses wander instead of ratcheting toward maximum damage.

Random modes:

- `Bent`: balanced and usable
- `Damaged`: fairly destructive, subject still readable
- `Shorted`: severe and unstable
- `Explore`: broad random space, may produce unusable results

Determinism is desirable but not absolute. Presets store seeds and reproduce the same general result, but future engine changes may alter exact pixels.

## Fictional Camera Presets

Presets are full-system configurations, not separate special effects. They act as starting points: loading one copies its settings into the editor, after which every control and randomizer works from the current state.

The app ships with 14 built-in cameras. The original five:

- `Bent CCD-03`: strong magenta/cyan clipping, medium vertical melt
- `Dead Flash Compact`: blown highlights, hard edge burn, noisy shadows
- `Memory Card Fever`: block and scanline corruption with moderate color faults
- `Overheated Sensor`: heavy vertical smear, pink/blue palette, dense noise
- `Cheap Menu Solar`: posterized false color with minimal geometry damage

Nine more, each modeled on specific reference images:

- `IR Bloom`: infrared-negative magenta bloom with sorted drips
- `Rainbow Burst`: diagonal rainbow wash over grainy skies
- `Acid Lake`: red-orange solar sky, cyan trees, pink water
- `Candy Snow`: pastel duotone — teal shadows, pink highlights
- `Pixel Melter`: heavy sorted drips pulling highlights down the frame
- `Interlace Crash`: row-interlace tearing, block shifts, chroma fringes
- `Night Stalker`: crushed blacks with toxic green speckle
- `Poison Corridor`: melted lens blur posterized into green contour slime
- `Negative Bloom`: full channel inversion with saturation push and solarize

## Roadmap

### Phase 0: Specification — done

- define product and aesthetic target
- inspect visual references
- define first pipeline and preset format

### Phase 1: Still Image Prototype — done

- scaffold web app
- load image into processing canvas
- implement core effect engine (12 modules)
- add macro controls
- add randomizers
- export PNG/WebP/JPEG

### Phase 2: Presets And Advanced Controls — mostly done

- save/load preset JSON — done
- fictional camera presets (14 built-in) — done
- advanced controls per circuit module — done
- live preset thumbnails rendered from the loaded image — done
- dedicated preset gallery view — open

### Phase 3: Quality Pass — mostly done

- tune effects against reference images — done
- before/after comparison (A/B split slider + hold-to-compare) — done
- original-resolution export via the render worker — done
- viewport-locked desktop layout with scrolling side panels — done
- undo/redo, per-module solo/bypass/randomize, in-app help — done
- automated regression tests wired into CI — open (engine and UI are currently exercised by ad-hoc scripts)

### Phase 4: Video And CLI — partially done

- reuse preset format in a CLI (`src/cli.js`, ffmpeg-backed) — done
- process video frame-by-frame — open
- add temporal stability controls — open
- support batch image processing — open

### Phase 5: New Effect Families (backlog, ideated 2026-07-05)

The current engine covers the analog signal path (sensor readout, exposure, color response, noise) well. The remaining gaps are the camera's digital brain: codec, firmware, and timing faults. Each entry below has enough implementation detail to start cold.

Priority order by impact-per-effort: **1) DCT corruption, 2) OSD/datestamp, 3) sync tear + rolling wobble**, then the rest.

**1. `dctCrunch` — JPEG/DCT corruption (highest priority)**
The most recognizable missing digital-camera glitch; reference image 22 (mountain quadrants) is this family. Implement as a late-pipeline module (after memoryFault, before finalCrunch): split image into 8x8 (or 16x16) blocks, DCT each block (a simple separable 8x8 DCT in JS is fine), then damage coefficients: quantize harshly (quality param), randomly zero or scramble AC coefficients in seeded block regions, and apply "DC drift" — a slowly accumulating offset to DC terms along block-scan order so color slides block-by-block into wrong hues (the rainbow gradient-drift look). Also add standalone `chromaSubsample` param (force chroma to quarter res while luma stays sharp — cheap, instantly "2003 JPEG"). Params: `quality`, `dcDrift`, `acScramble`, `blockRepeat` (macroblock stutter), `chromaSubsample`.

**2. `osdOverlay` — camera UI burn-in (small effort, huge character)**
Orange corner datestamp (`'06 7 5` style), battery icon, `ISO 80`, focus brackets, REC dot, optionally glitched: wrong glyphs, mojibake, doubled/torn overlay. Implement as the final pipeline stage drawing from a tiny embedded bitmap font (5x7 px, scaled by image size; draw into ImageData directly so it works in worker + CLI). Params: `datestamp` (bool + seeded date), `hudIcons`, `glitchText` (0-1 corruption of glyphs), `scale`, `color` (orange/green/white). Note: this is set dressing, keep it out of most default presets, off by default in macros.

**3. `syncFault` — sync tear + rolling-shutter wobble (small effort, new geometry)**
Everything in the current pipeline except blockShift preserves vertical alignment; this adds coherent geometric damage. Two sub-effects in one module, applied early (right after cheapCamera): (a) frame wrap — below a seeded row N, shift all rows horizontally by a constant and wrap around, with a torn transition band of 2-6 corrupted rows; (b) rolling wobble — per-row horizontal sine displacement with progressive phase (jello), plus low-frequency row phase drift so verticals go wavy. Params: `tearCount`, `tearShift`, `wobbleAmount`, `wobbleFrequency`, `drift`.

**4. `bayerFault` — demosaic corruption (sleeper pick, unique texture)**
Re-interpret the image as if the Bayer grid were misaligned: sample into an RGGB mosaic, then demosaic with the wrong phase offset → green/magenta pixel checkerboards, zipper edges, moiré rainbows on fine texture. Nothing else in the pipeline produces pixel-scale crosshatch. Early pipeline (before exposure). Params: `phaseError` (0-3 offset), `strength` (blend), `zipper` (edge aliasing boost).

**5. `bufferGhost` — stale-frame ghosting**
Frame buffer not cleared: blocks/bands of a "different frame" bleed through. Fake the previous frame with a transformed copy of the same image (shifted/zoomed/earlier pipeline stage snapshot). Blend in seeded blocks or interlaced fields (odd/even rows from different transforms → comb tearing). Params: `amount`, `blockSize`, `ghostShift`, `ghostZoom`, `fieldMode` (bool).

**6. `ampGlow` — thermal corner glow**
Long-exposure sensor amplifier heat: purple/orange glow creeping from one seeded corner/edge, palette-tinted. Cheap radial gradient added late. Params: `strength`, `corner` (seeded), `hue`, `spread`.

**7. Dead columns / pixel clusters (extend `sensorNoise`)**
Single-color vertical hairlines (1px columns stuck at a color) and small dead rectangles — most common real CCD defect, distinct from random hot pixels. Add `deadColumns` (0-1 count control) and `deadClusters` params to sensorNoise.

**8. Purple fringing / blooming halos (extend `exposureFault` or `edgeBurn`)**
Violet edges specifically around clipped highlights (CCD overflow): dilate the clipped-highlight mask a few px, tint the rim violet/magenta. Param: `fringing` on exposureFault.

**9. `awbSeizure` — auto-WB/AE hunting bands**
White balance or exposure oscillating during readout: horizontal bands that pump warm/cold or bright/dark down the frame (low-frequency sine + seeded noise per band). Params: `wbSwing`, `aeSwing`, `bandHeight`, `frequency`.

**10. Generational recompression (meta-dial)**
A "save count" control that runs the crunch stages (dctCrunch + cheapCamera) N times with slight parameter drift per pass — the "saved and reopened 50 times" look. Pairs with dctCrunch; also gives video mode its degradation-over-time story. Could be a `generations` param on dctCrunch rather than a separate module.

Integration checklist for any new module (this is the established pattern):
- pure function in `src/engine-core.js`, called from `processCircuitBendImageData` (no browser APIs — shared by worker and CLI)
- defaults in `defaultPipeline()` in `src/presets.js` (disabled by default), plus an `ADVANCED_DEFS` group with `key:` for solo/dice/lamp buttons
- macro coupling in `applyMacrosToPipeline` only if it fits an existing macro (chaos → syncFault/bufferGhost; cheapness → dctCrunch strength; otherwise leave preset-driven like colorBend)
- entry in `MODULE_RANDOMIZERS` in `src/randomize.js` (this automatically powers the per-module dice)
- document in `docs/PRESET_FORMAT.md` example JSON + module reference (a sync-check script pattern exists: parse the doc's JSON example and diff module params against `defaultPipeline()`)
- verify: `node src/cli.js render test-images/<img> out.png --set pipeline.<module>.enabled=true ...` and view the PNG; compare against local, uncommitted `reference-images/`
- consider one new built-in preset per major module, modeled on a specific reference image

## Decisions Made (formerly Open Decisions)

- web framework: none — plain ES modules, no build step
- shader layer: Canvas/CPU-first, running in a Web Worker; WebGL deferred until needed
- export formats: PNG, WebP, JPEG
- preset thumbnails: embedded as WebP data URLs in the preset JSON
- reference imagery in the app UI: still open — currently none is shown
