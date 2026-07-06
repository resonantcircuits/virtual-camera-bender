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
-> sync fault (frame-wrap tears + rolling-shutter wobble)
-> bayer fault (wrong-phase demosaic checkerboards + zipper edges)
-> buffer ghost (stale-frame blocks / field interlace, self or loaded second image)
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
-> DCT crunch (JPEG quantization, DC drift, AC scramble, block stutter, chroma subsampling)
-> final bit crush + dither + sharpen
-> OSD overlay (datestamp + HUD burn-in)
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

The app ships with 19 built-in cameras. The original five:

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

Five more added with the Phase 5 digital-brain modules (2026-07-06):

- `Codec Rot`: corrupted JPEG decode — block hue drift, scrambled macroblocks, chunky chroma
- `Hold Vertical`: lost sync — frame-wrap tears, wavy rolling shutter, interlace fringes
- `Tourist Compact '03`: overcompressed vacation snap with orange datestamp and HUD burn-in
- `Zipper Mosaic`: misaligned demosaic — green-magenta checkerboards and zipper edges
- `Double Buffer`: uncleared frame buffer — a shifted stale frame bleeds through in blocks

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

### Phase 5: New Effect Families (ideated 2026-07-05; items 1-5 done 2026-07-06)

The current engine covers the analog signal path (sensor readout, exposure, color response, noise) well. The remaining gaps are the camera's digital brain: codec, firmware, and timing faults. Each entry below has enough implementation detail to start cold.

Priority order by impact-per-effort: **1) DCT corruption, 2) OSD/datestamp, 3) sync tear + rolling wobble**, then the rest.

**1. `dctCrunch` — JPEG/DCT corruption — done**
The most recognizable missing digital-camera glitch; reference image 23 (mountain quadrants; the original spec said 22 but the numbering was off by one) is this family. Implemented late-pipeline (after memoryFault, before finalCrunch): 8x8 blocks per YCbCr plane through a separable orthonormal DCT, then coefficient damage: JPEG-style quantization (`quality`, frequency-weighted steps), `acScramble` (zero/shuffle/inject AC coefficients in fbm-gated block patches), `dcDrift` (random-walk DC offset along block-scan order, walk rate scaled by total block count so the slide spans the frame at any resolution, chroma wandering further than luma, rare hard jumps for quadrant-style breaks), `blockRepeat` (held-macroblock stutter, patch-gated), `chromaSubsample` (2x2 chroma averaging while luma stays sharp). Macro coupling: cheapness drives enable/quality/chromaSubsample, chaos adds acScramble/blockRepeat; dcDrift stays preset-driven. Built-in preset: `Codec Rot`.

**2. `osdOverlay` — camera UI burn-in — done**
Orange corner datestamp (seeded `'03 1 16` style), REC dot, ISO readout, battery icon, and focus brackets drawn as the final pipeline stage from an embedded 5x7 bitmap font, straight into ImageData (worker + CLI safe), with a drop shadow, size scaled by image dimension. `glitchText` swaps glyphs, tears baselines, and doubles the overlay. Params: `datestamp`, `hudIcons`, `glitchText`, `scale`, `color` (orange/green/white). No macro coupling and excluded from global randomize (set dressing — it only turns on by hand, via its module dice, or a preset). Built-in preset: `Tourist Compact '03`.

**3. `syncFault` — sync tear + rolling-shutter wobble — done**
Applied right after cheapCamera. (a) frame wrap: below each seeded tear row the frame shifts sideways and wraps, with a 2-6 row corrupted transition band (stuttered cells, split chroma, specks); (b) rolling wobble: per-row sine displacement with progressive phase and fbm phase drift plus amplitude envelope so verticals wander. Params: `tearCount`, `tearShift`, `wobbleAmount`, `wobbleFrequency`, `drift`. Macro coupling: chaos > 0.62 enables tears/wobble. Built-in preset: `Hold Vertical`.

**4. `bayerFault` — demosaic corruption — done**
The image is resampled into an RGGB mosaic at the true grid phase, then bilinear-demosaiced assuming a shifted phase (`phaseError` 0-3: horizontal/vertical/diagonal misalignment) so channels reconstruct from the wrong sensor wells — green/magenta pixel checkerboards, wholesale channel swaps, zipper edges. Phase 0 is a correct-phase demosaic: just the cheap-camera softening. `zipper` adds alternating-pixel green/magenta shimmer along source edges; `strength` blends. Applied early (after syncFault, before chromaShift). Preset-driven (no macro coupling); rolled by the Cheap family (35%) and a rare guest in global randomize (15%). Built-in preset: `Zipper Mosaic`.

**5. `bufferGhost` — stale-frame ghosting — done**
Frame buffer not cleared: fbm-gated blocks (or odd scan fields with `fieldMode` → comb tearing) show a stale frame, blended per-block. The stale frame is a shifted/zoomed snapshot of the image itself by default, or — extending the original idea — a second image loaded by the user (Ghost Source `LOAD` button in the Buffer Ghost panel; `--ghost <image>` in the CLI). The ghost travels as a render resource (`processCircuitBendImageData(image, preset, { ghost })`), sampled bilinearly in normalized coordinates so any resolution works; it is session state, never serialized into preset JSON. Params: `amount`, `blockSize`, `ghostShift`, `ghostZoom`, `fieldMode`. Macro coupling: chaos > 0.7 enables it. Built-in preset: `Double Buffer`.

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
