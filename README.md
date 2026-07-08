# Virtual Camera Bender

![Virtual Camera Bender](docs/assets/banner.png)

Virtual Camera Bender is a web-based still-image editor for creating circuit-bent compact digital camera aesthetics from input images, with a CLI companion that applies the same cameras to video.

The project is inspired by early 2000s consumer digital cameras pushed into failure states: false color, clipped highlights, vertical readout smear, dense sensor noise, posterized contours, and occasional memory or scanline corruption. The goal is artistic plausibility, not physically exact simulation — with one growing exception: the physics rail modules (CCD clock, AFE bend, rail sag, bus bend, master clock) physically simulate real circuit-level bends on the camera's charge-transfer clocks, analog front end, supply rail, ADC data bus, and master oscillator.

## Current State

The still-image editor is working: image upload (click or drag-and-drop), live preview, 49 built-in camera presets with live thumbnails, macro and per-module controls, family/global/per-module randomizers, undo/redo, A/B comparison, JSON preset save/load, original-resolution export (PNG/WebP/JPEG), and a headless CLI renderer that shares the same engine.

Video is supported image-first: load a clip in the app to design the look on a contact sheet of its frames (no in-app playback or encoding), tune temporal behavior (locked/hold/flicker seeds, parameter drift, frame ghosting), then render with the CLI (`render-video`, ffmpeg-backed, parallel workers).

Repository layout:

- `index.html`, `styles.css`: static web app shell (no build step).
- `src/engine-core.js`: the pure image-processing pipeline, shared by the web app, the render worker, and the CLI.
- `src/render-worker.js`: Web Worker that runs the pipeline off the main thread.
- `src/presets.js`: preset schema defaults, macro-to-pipeline mapping, control definitions.
- `src/built-in-presets.js`: the built-in camera preset collection.
- `src/randomize.js`: random modes, family randomizers, and per-module randomizers.
- `src/app.js`: UI wiring.
- `src/temporal.js`: video temporal scheduling — per-frame seed modes and parameter drift, shared by app and CLI.
- `src/cli.js`, `src/dev-server.js`: headless still/video renderer (ffmpeg-backed) and static dev server.
- `src/video-worker.js`: Node worker thread used by `render-video` for parallel frame rendering.
- `docs/PROJECT_SPEC.md`: product, aesthetic, technical, and roadmap spec.
- `docs/PRESET_FORMAT.md`: the preset JSON structure and module reference.

Local source/reference image folders are intentionally ignored by Git. Keep personal test photos in `test-images/` and unlicensed visual references in `reference-images/`.

## Run Locally

Start a local static server from the repository root:

```sh
npm run serve
```

Then open `http://localhost:8787`.

## Engine Modules

The still-image pipeline currently chains these modules (all preset-controlled):

physics rail (one shared raw round trip: inverse ISP → 12-bit Bayer raw → enabled circuit sims in signal order → forward ISP), currently CCD clock (charge-transfer faults: melt drips, row stalls, shear bands, bloom spikes) → AFE bend (analog oscillator injection, gain wobble, CDS ghosting) → rail sag (supply brownout: breathing exposure bands, latch-up color bands, static and dropout rows) → bus bend (per-clock ADC data-bus bend circuit, with soft series-resistance shorts and a common effects bus) → master clock (reclocked capture: stretch, skew, vertical roll, sync tears, shredded scanlines) → then cheap camera (downscale, lens blur, bit crush + dither, sharpen) → sync fault (frame-wrap tears, rolling-shutter wobble) → bayer fault (wrong-phase demosaic checkerboards) → buffer ghost (stale-frame blocks, self or loaded second image) → chroma shift → exposure fault → color bend (hue rotate, channel swap/invert, solarize) → contour rings → false color (9 palettes, posterized or smooth gradient map) → gradient wash (positional rainbow fields) → edge burn → pixel sort → vertical smear → sensor noise → memory fault (interlace, block shift, row repeat, scanline dropout) → DCT crunch (JPEG quantization, DC hue drift, AC scramble, block stutter, chroma subsampling) → OSD overlay (datestamp and HUD burn-in).

## Workflow Features

- **Web Worker rendering** — processing runs off the main thread, so the UI stays responsive during heavy renders and full-resolution exports.
- **Undo / Redo** — every preset switch, randomize, reroll, and control tweak is tracked (up to 60 steps).
- **A/B split compare** — toggle Split A/B and drag the divider across the image; hold `C` for a quick full-frame flash of the original.
- **Per-module randomize, solo & bypass** — each module group in the circuit panels has an `R` (dice: re-roll only that module's parameters, keeping the seed), an `S` (solo: render only that module), and a lamp button (quick enable/disable). The dice respects the Randomize mode selected in the left panel.
- **Live preset thumbnails** — the preset list previews every built-in camera on the currently loaded image.
- **Ghost image** — the Buffer Ghost module blends a stale frame into blocks of the image: a shifted copy of the photo by default, or any second image via the `LOAD` button in its Stylized Circuit panel (`--ghost <image>` in the CLI).

- **Video frames sheet** — loading a video opens a 25-frame contact sheet rendered through the current camera (`V` or the Frames button); clicking a frame makes it the working image, carrying its temporal seed so the preview matches the final render of that exact frame.
- **Temporal panel** — visible when a video is loaded: seed mode (locked / hold / flicker), hold length, parameter drift amount/speed, and ghost lag (feeds Buffer Ghost the frame N back for real stale-buffer trails). Saved in the preset; the Copy Render Command button emits the matching CLI line.

## Shortcuts

- `R` — global randomize
- `G` — camera gallery
- `V` — video frames sheet (when a video is loaded)
- `C` (hold) — view original image
- `S` — toggle A/B split compare
- `Cmd/Ctrl+Z` / `Shift+Cmd/Ctrl+Z` — undo / redo
- `?` — in-app help (signal path, randomize modes, module buttons)
- Reroll button — new seed, same settings

## Command Line

Stills and video render headless (requires `ffmpeg` on PATH):

```sh
node src/cli.js render input.jpg output.png --builtin "IR Bloom"
node src/cli.js render input.jpg output.png --preset my-camera.vcb-preset.json --set pipeline.pixelSort.strength=0.9
node src/cli.js render input.jpg output.png --builtin "Double Buffer" --ghost other-frame.jpg
node src/cli.js render-video clip.mp4 bent.mp4 --preset my-camera.vcb-preset.json
node src/cli.js render-video clip.mp4 test.mp4 --builtin "Codec Rot" --start 4 --duration 2 --max-dimension 960
node src/cli.js list-presets
```

`render-video` processes every frame through the engine in parallel worker threads (`--jobs`, default cores−1), preserves the source frame rate, stream-copies audio, and encodes H.264 (`--crf`, default 18). Temporal behavior comes from the preset's `temporal` block and can be overridden inline, e.g. `--set temporal.mode=hold --set temporal.driftAmount=0.4`. Frame indices are deterministic: the same input, preset, and options produce byte-identical output regardless of `--jobs`.

## Roadmap

The first milestone (still-image web app with upload, preview, presets, randomizers, macro + advanced controls, JSON preset save/load, and original-resolution export) is complete, the CLI renderer arrived early, and video mode (contact-sheet design in the app, `render-video` in the CLI with temporal seed modes, drift, and frame ghosting) shipped with Phase 4. Next up:

- batch image processing through the CLI
- more effect families targeting the camera's digital brain (JPEG/DCT corruption, OSD/datestamp burn-in, sync tear + rolling-shutter wobble, Bayer/demosaic faults, and stale-buffer ghosting are done): amp glow, dead columns, purple fringing, AWB/AE hunting bands, and generational recompression — full implementation notes in `docs/PROJECT_SPEC.md` under "Phase 5: New Effect Families"

## Design Direction

The interface prioritizes usability while borrowing from old digital camera menus: compact controls, mode labels, preset slots, simple macro settings, and deeper configuration panels when needed.

The engine keeps the image-processing pipeline (`src/engine-core.js`) separate from the UI, so the same preset format drives the web app, the CLI renderer, and a future video renderer.
