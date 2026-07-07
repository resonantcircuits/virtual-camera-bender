# Project Spec

## Product Goal

Build a web-based still-image editor that turns normal input images into convincing circuit-bent early-2000s compact digital camera images.

The output should usually remain somewhat recognizable, but can be fairly destructive. The tool should make it easy to discover, save, tweak, and export strong glitch looks.

Video is supported image-first (Phase 4, shipped 2026-07-06): the app is the design surface — a loaded clip becomes a frame source with a contact sheet, no in-app playback or encoding — and the CLI (`src/cli.js render-video`) renders the clip with the shared engine, driven by the preset's `temporal` block.

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
-> physics rail (one raw round trip: inverse ISP -> 12-bit Bayer raw -> afeBend -> busBend -> forward ISP)
-> cheap-camera downscale + lens blur
-> sync fault (frame-wrap tears + rolling-shutter wobble)
-> bayer fault (wrong-phase demosaic checkerboards + zipper edges)
-> buffer ghost (stale-frame blocks / field interlace, self or loaded second image)
-> chroma shift
-> exposure and clipping fault (incl. violet highlight fringing)
-> AWB/AE seizure (white-balance and exposure hunting bands)
-> color bend (hue rotate / channel swap / invert / solarize)
-> contour rings
-> false color (posterized or smooth gradient map, 9 palettes)
-> gradient wash
-> edge burn
-> pixel sort
-> vertical smear
-> sensor noise / hot pixels / striping / dead columns and clusters
-> amp glow (thermal corner glow)
-> memory and scanline faults (interlace, block shift, row repeat, dropout)
-> DCT crunch (JPEG quantization, DC drift, AC scramble, block stutter, chroma subsampling, N-generation re-save)
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

Video mode (implemented) applies the same engine per frame with temporal scheduling in `src/temporal.js`, shared verbatim by the app preview and the CLI:

- **Two seed tiers.** The structural seed follows `temporal.mode` — `locked` (frozen damage), `hold` (re-rolls every ~`holdFrames` frames, hold lengths jittered ±40%), `flicker` (per frame). The live seed changes every frame regardless and drives only noise textures (sensor grain, speckle, hot pixels, amp-glow grain) via an optional `liveSeed` render option — so locked damage still shimmers like a live sensor instead of reading as a static overlay. Fixed-pattern striping and dead columns/clusters stay structural, like real stuck hardware.
- **Parameter drift.** `driftAmount`/`driftSpeed` wander every enabled module's continuous params around their preset values on smooth seeded noise tracks (±30% of each control's range at full drift; integer-stepped params excluded to avoid pops). Drift runs off the base seed, so tracks stay continuous across hold-mode seed jumps.
- **Frame ghosting.** `temporal.ghostFrame: N` feeds bufferGhost the input frame N back (ring buffer in the render loop) — genuine stale-buffer trails instead of the self-frame approximation.
- **App as design surface.** Loading a video extracts 25 evenly spaced frames into a full-screen contact sheet (`V`), each rendered through the current camera at its true frame index (including its ghost companion frame when ghost lag is set). Clicking a frame makes it the working image carrying its frame index, so the whole still-image editor previews exactly what the CLI renders for that frame. A Temporal panel edits the block; Copy Render Command emits the CLI line. Frame indices in the app come from a measured fps estimate (requestVideoFrameCallback deltas, snapped to common rates); the CLI uses the container's true rate.
- **CLI renderer.** `render-video` streams rawvideo through two ffmpeg pipes (no temp frame files), renders frames on a worker_threads pool (`--jobs`, default cores−1, output written in order — byte-identical results at any job count), preserves source fps and audio (stream copy), encodes H.264/yuv420p (`--crf`, default 18), and supports `--start`/`--duration` trims plus `--max-dimension` for cheap test renders.

## Randomization Model

Randomization happens at three levels (all implemented in `src/randomize.js`):

- global randomize: builds a whole new camera — fresh macros, a fresh pick of active modules, new name and seed. Each roll mutes some damage channels so results have distinct characters instead of everything firing at once. The physics rail participates as archetypes, not background guests: ~65% of builds are stylized-only, ~25% physics-led (one rail circuit rolls, occasionally both, with stylized macros scaled way down so the circuit look reads), ~10% stack both at full strength.
- family randomizers (`Physics`, `Color`, `Melt`, `Burn`, `Noise`, `Cheap`, `Memory`): re-roll one damage domain with fresh draws, leave the rest untouched. Families are *domains*, not modules — future physics modules (ccdClock, addressBus, jpegStream) join the Physics family and the per-module dice rather than adding buttons.
- per-module dice (`R` in each circuit-panel group header): re-roll one module's parameters while keeping the global seed, so everything else renders identically.

All draws are fresh values within the mode's intensity band — never cumulative — so repeated presses wander instead of ratcheting toward maximum damage.

Random modes:

- `Bent`: balanced and usable
- `Damaged`: fairly destructive, subject still readable
- `Shorted`: severe and unstable
- `Explore`: broad random space, may produce unusable results

Determinism is desirable but not absolute. Presets store seeds and reproduce the same general result, but future engine changes may alter exact pixels.

## Fictional Camera Presets

Presets are full-system configurations, not separate special effects. They act as starting points: loading one copies its settings into the editor, after which every control and randomizer works from the current state.

The app ships with 22 built-in cameras. The original five:

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

Three more added with the final Phase 5 analog-fault modules (2026-07-06):

- `Dark Frame Leak`: long-exposure amp glow creeping from a corner over stuck columns and hot speckle
- `AWB Panic`: white balance and exposure hunting mid-readout — warm/cold bands pumping down the frame
- `Copy Of A Copy`: five generations of recompression with violet-fringed clipping

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
- dedicated preset gallery view — done (full-screen overlay, `G` or the Gallery button: every camera rendered live on the loaded image at 300px, filter box, arrow-key navigation, click applies and closes; thumbnails regenerate lazily when the source or ghost changes)

### Phase 3: Quality Pass — mostly done

- tune effects against reference images — done
- before/after comparison (A/B split slider + hold-to-compare) — done
- original-resolution export via the render worker — done
- viewport-locked desktop layout with scrolling side panels — done
- undo/redo, per-module solo/bypass/randomize, in-app help — done
- automated regression tests wired into CI — open (engine and UI are currently exercised by ad-hoc scripts)

### Phase 4: Video And CLI — mostly done

- reuse preset format in a CLI (`src/cli.js`, ffmpeg-backed) — done
- process video frame-by-frame — done (2026-07-06: `render-video` with streaming ffmpeg pipes and a worker-thread pool; see "Video mode" under Technical Direction)
- add temporal stability controls — done (preset `temporal` block: locked/hold/flicker seed modes with a per-frame live tier for noise, parameter drift, frame ghosting; edited in the app's Temporal panel, previewed on the frames contact sheet)
- support batch image processing — open

### Phase 5: New Effect Families (ideated 2026-07-05; all ten items done 2026-07-06)

The current engine covers the analog signal path (sensor readout, exposure, color response, noise) well. The remaining gaps were the camera's digital brain: codec, firmware, and timing faults.

Priority order by impact-per-effort was: **1) DCT corruption, 2) OSD/datestamp, 3) sync tear + rolling wobble**, then the rest.

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

**6. `ampGlow` — thermal corner glow — done**
Long-exposure sensor amplifier heat: a grainy radial glow creeping in from one corner, applied after sensorNoise (so memory/codec faults corrupt it too). `corner` is `seeded` by default (the seed picks one of four corners) or fixed; `hue` blends the tint from purple to hot orange; `spread` sets reach; per-pixel hash grain keeps it noisy like real thermal signal. Preset-driven (no macro coupling); rolled by the Noise family (30%) and a rare guest in global randomize (16%). Built-in preset: `Dark Frame Leak`.

**7. Dead columns / pixel clusters (extend `sensorNoise`) — done**
`deadColumns` (0-1): up to ~16 seeded 1px vertical columns stuck at a color (hot white, dead black, or a saturated palette color), most running the full height, some starting partway down. `deadClusters` (0-1): small stuck rectangles, sized relative to resolution. Both roll with the sensorNoise module dice and the Noise family.

**8. Purple fringing / blooming halos — done**
`fringing` param on exposureFault: the post-clip highlight mask is box-blurred into a halo, and the rim (halo minus core) is tinted violet — CCD charge overflow around blown highlights. Radius scales with fringing and resolution. Used by the `AWB Panic` and `Copy Of A Copy` presets.

**9. `awbSeizure` — auto-WB/AE hunting bands — done**
Applied right after exposureFault. Bands of rows (`bandHeight`) get per-band gains from a low-frequency sine plus seeded jitter: `wbSwing` pumps red/blue against each other (warm/cold), `aeSwing` pumps overall gain (bright/dark), `frequency` sets the oscillation rate, and ~4% of bands overshoot hard before the loop corrects. Preset-driven; rolled by the Color family (25%) and a rare guest in global randomize (14%). Built-in preset: `AWB Panic`.

**10. Generational recompression — done**
Implemented as a `generations` param (1-6) on dctCrunch rather than a separate module: each pass re-runs the full DCT crunch with a fresh seed and drifted parameters (quality slides down ~7% per save; scramble/drift/repeat fade by 0.62^gen so the first save dominates and re-saves add flavor; chroma re-subsamples at reduced blend). Rolled occasionally by the dctCrunch dice (25% chance of 2-5 generations). Built-in preset: `Copy Of A Copy`.

Integration checklist for any new module (this is the established pattern):
- pure function in `src/engine-core.js`, called from `processCircuitBendImageData` (no browser APIs — shared by worker and CLI)
- defaults in `defaultPipeline()` in `src/presets.js` (disabled by default), plus an `ADVANCED_DEFS` group with `key:` for solo/dice/lamp buttons
- macro coupling in `applyMacrosToPipeline` only if it fits an existing macro (chaos → syncFault/bufferGhost; cheapness → dctCrunch strength; otherwise leave preset-driven like colorBend)
- entry in `MODULE_RANDOMIZERS` in `src/randomize.js` (this automatically powers the per-module dice)
- document in `docs/PRESET_FORMAT.md` example JSON + module reference (a sync-check script pattern exists: parse the doc's JSON example and diff module params against `defaultPipeline()`)
- verify: `node src/cli.js render test-images/<img> out.png --set pipeline.<module>.enabled=true ...` and view the PNG; compare against local, uncommitted `reference-images/`
- consider one new built-in preset per major module, modeled on a specific reference image

### Phase 6: Physics-Based Bus Bend (`busBend`) — v1 done 2026-07-07

A departure from the "plausible approximation" philosophy: this module actually simulates the signal path a real logic-chip camera bend attacks. Reference: the PowerShot A520 "source/target selector" bend (r/CircuitBending, u/theglorioustopsail; schematic in local `diagrams/`, result photos in local `reference-images/reference-camera-1/`). That bend taps the parallel data bus between the CCD's 12-bit ADC and the DIGIC processor: a 10-switch DIP selects source bits (D11-D2), a 1µF cap + 1kΩ pot forms a variable high-pass, a NAND-as-NOT or a D-flip-flop divide-by-two conditions the signal, and a 9-position selector injects it back onto target bits (D11-D4, GND) against the ADC's own pin drivers.

Implementation (`src/bus-bend.js`, pure JS, browser-free, called first in the pipeline):

1. **Inverse ISP**: sRGB → linear (LUT) → divide out WB gains → RGGB mosaic → 12-bit raw with black-level pedestal.
2. **Per-clock circuit sim** in row-major readout order: intra-bank hard shorts (drive-fight averaging with comparator-noise thresholding), one-pole RC high-pass whose time constant maps from the pot (normalized to a 3072px sensor row so streak scale is resolution-independent; the pot also sets a DC pull-down load on bypass targets and the surviving edge amplitude), Schmitt-conditioned invert/divide logic with strong CMOS drive, bus-contention resolution per target pin, and one-clock-delayed feedback of the resolved wire into the filter/flip-flop (this feedback is why divide only sings when source and target pins overlap, matching the builder's description).
3. **Forward ISP**: black level, WB gains, bilinear demosaic, linear → sRGB. WB × gamma amplification of raw bit damage is what produces the posterized rainbow contours, color negatives, and scanline chaos of the reference photos.

Validated against the four reference looks via CLI contact sheets (identity round-trip is clean; HPF/INV/DIV families reproduced). ~0.25s at 960px, ~fine for video (26 fps at 640px). Built-in presets: `Rainbow Bus Tap`, `Logic Negative`, `Divide By Two`, `Cloud Solarizer`. UI: DIP-switch bit-bank control type (`bits`) in the advanced panel. Excluded from global randomize (character module, like osdOverlay) but has a full per-module dice.

Future extensions (not scheduled): more logic "chips" (XOR, counters, shift registers), horizontal-blanking/interlaced-field readout if scanline statistics need to match harder, CCD clock-signal bending, oversampled analog sim via WASM if sub-clock effects are ever wanted.

### Phase 7: The Physics Rail (ideated 2026-07-07)

Phase 6 proved that simulating the actual circuit beats approximating its look. A real camera offers a chain of physically distinct bend domains, each failing with a different visual grammar because the image lives in a different representation at each stage. Phase 7 builds them as siblings sharing one infrastructure, chained in true signal order at the head of the pipeline:

```text
charge domain -> analog domain -> ADC data bus -> memory -> codec
(ccdClock)      (afeBend, done)  (busBend, done)  (addressBus) (jpegStream)
```

**0. Shared raw-domain harness (`src/raw-domain.js`) — done 2026-07-07**
Factor the inverse/forward ISP out of `bus-bend.js`: sRGB→linear LUTs, RGGB mosaic, 12-bit raw with black pedestal, bilinear demosaic, gamma. The engine runs a single *physics rail* pass: one `imageToRaw`, then every enabled physics module in signal order on the same raw buffer, then one `rawToImage` — so corruptions compound physically (a charge smear that then gets bus-bent inherits both) and the image isn't demosaic-softened once per module. One camera has one white balance: the rail takes WB gains from the first enabled physics module.

**1. `afeBend` — analog front end bend (analog domain) — done 2026-07-07**
Shipped as `src/afe-bend.js` with built-in presets `Ground Loop` (rolling hum bands), `Carrier Clash` (square-wave moire + gain pump), `Reset Ghost` (CDS derivative emboss). Validated: flat-field band period matches the cycles-per-row math; red-before-green black clipping gives the hard colored band edges; idle module round-trips the image intact; compounds correctly with busBend on the shared rail. ~0.37s at 960px including camera-JPEG finish.
The pixel stream before the ADC is a continuous voltage; this is where the classic "audio into the video path" bend lives. Three couplings, all in one per-clock pass over the raw stream:
- *Oscillator injection* (additive): sine/square/saw/sample-hold-noise waveform at a frequency parameterized in cycles-per-row (log sweep ~0.02–320) — sub-row frequencies give rolling horizontal hum bands, tens of cycles per row give tilted moiré against the pixel clock; a `skew` param adds per-row phase creep (band tilt), `wobble` adds seeded phase drift so bands breathe.
- *PGA/VREF wobble* (multiplicative): the same oscillator modulating gain-above-black — exposure that pumps instead of bands that add.
- *CDS timing skew*: correlated double sampling with the reset sample landing on a stale pixel (`signal[n] − amount·(signal[n−lag] − black)`) — the frame becomes a horizontal-derivative emboss with negative trails; lag on a log sweep of 1–48 clocks.
Where busBend is harsh and posterized, this module is liquid: smooth interference physics, colored by WB like everything raw-domain.

**2. `ccdClock` — charge-transfer clock bend (charge domain)**
Before there are bits or even voltages, the image is charge packets marched by multi-phase transfer clocks (V1/V2 row clocks, H1/H2 register clocks). Model the sensor as charge buckets and corrupt the transfer schedule: skipped V pulses (row repeats + charge accumulation), doubled pulses (row skips), partial transfer efficiency (a fraction of charge left behind each shift → *physical* vertical smear, unlike the painterly `verticalSmear`), inter-well mixing, and H-register glitches for per-row horizontal shear. Geometric melt — the NOYSTOISE look, and the A520 author's own "future work" item.

**3. `addressBus` — frame-buffer address-line bend (memory domain)**
`memoryFault` is painterly; the physical version shorts or sticks *address lines* on the SDRAM, so memory aliases in exact powers of two: mirrored tiles, interleaved row pairs, ping-ponged halves — crystalline deterministic repetition, nothing like random block damage. Cheap to simulate (remap read addresses through a corrupted-bit function) and very distinct.

**4. `jpegStream` — entropy-domain bitstream corruption (codec domain)**
`dctCrunch` damages coefficients; real corrupt JPEGs damage the *Huffman bitstream*, so everything after a flipped bit decodes wrong until the next restart marker — the classic cascading shear-and-hue-slide. Needs a small sequential JPEG encoder/decoder pair (encode with restart markers, flip seeded bits, tolerant decode). Largest effort of the four; consider last.

Build order by distinctness-per-effort: **afeBend → ccdClock → addressBus → jpegStream**. Each lands with the standard integration checklist (Phase 5) plus a physics-validation gate: CLI contact sheets, identity round-trip clean when idle, and at least one seeded config matching a documented real-bend behavior.

## Decisions Made (formerly Open Decisions)

- web framework: none — plain ES modules, no build step
- shader layer: Canvas/CPU-first, running in a Web Worker; WebGL deferred until needed
- export formats: PNG, WebP, JPEG
- preset thumbnails: embedded as WebP data URLs in the preset JSON
- reference imagery in the app UI: still open — currently none is shown
