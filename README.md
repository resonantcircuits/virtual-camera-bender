# Virtual Camera Bender

![Virtual Camera Bender](docs/assets/banner.png)

Virtual Camera Bender is a web-based still-image editor for creating circuit-bent compact digital camera aesthetics from input images.

The project is inspired by early 2000s consumer digital cameras pushed into failure states: false color, clipped highlights, vertical readout smear, dense sensor noise, posterized contours, and occasional memory or scanline corruption. The goal is artistic plausibility, not physically exact simulation.

## Current State

The still-image editor is working: image upload (click or drag-and-drop), live preview, 18 built-in camera presets with live thumbnails, macro and per-module controls, family/global/per-module randomizers, undo/redo, A/B comparison, JSON preset save/load, original-resolution export (PNG/WebP/JPEG), and a headless CLI renderer that shares the same engine.

Repository layout:

- `index.html`, `styles.css`: static web app shell (no build step).
- `src/engine-core.js`: the pure image-processing pipeline, shared by the web app, the render worker, and the CLI.
- `src/render-worker.js`: Web Worker that runs the pipeline off the main thread.
- `src/presets.js`: preset schema defaults, macro-to-pipeline mapping, built-in cameras, control definitions.
- `src/randomize.js`: random modes, family randomizers, and per-module randomizers.
- `src/app.js`: UI wiring.
- `src/cli.js`, `src/dev-server.js`: headless renderer (ffmpeg-backed) and static dev server.
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

cheap camera (downscale, lens blur, bit crush + dither, sharpen) → sync fault (frame-wrap tears, rolling-shutter wobble) → bayer fault (wrong-phase demosaic checkerboards) → chroma shift → exposure fault → color bend (hue rotate, channel swap/invert, solarize) → contour rings → false color (9 palettes, posterized or smooth gradient map) → gradient wash (positional rainbow fields) → edge burn → pixel sort → vertical smear → sensor noise → memory fault (interlace, block shift, row repeat, scanline dropout) → DCT crunch (JPEG quantization, DC hue drift, AC scramble, block stutter, chroma subsampling) → OSD overlay (datestamp and HUD burn-in).

## Workflow Features

- **Web Worker rendering** — processing runs off the main thread, so the UI stays responsive during heavy renders and full-resolution exports.
- **Undo / Redo** — every preset switch, randomize, reroll, and control tweak is tracked (up to 60 steps).
- **A/B split compare** — toggle Split A/B and drag the divider across the image; hold `C` for a quick full-frame flash of the original.
- **Per-module randomize, solo & bypass** — each module group in Advanced Circuit has an `R` (dice: re-roll only that module's parameters, keeping the seed), an `S` (solo: render only that module), and a lamp button (quick enable/disable). The dice respects the Randomize mode selected in the left panel.
- **Live preset thumbnails** — the preset list previews every built-in camera on the currently loaded image.

## Shortcuts

- `R` — global randomize
- `C` (hold) — view original image
- `S` — toggle A/B split compare
- `Cmd/Ctrl+Z` / `Shift+Cmd/Ctrl+Z` — undo / redo
- `?` — in-app help (signal path, randomize modes, module buttons)
- Reroll button — new seed, same settings

## Command Line

Stills can also be rendered headless (requires `ffmpeg` on PATH):

```sh
node src/cli.js render input.jpg output.png --builtin "IR Bloom"
node src/cli.js render input.jpg output.png --preset my-camera.vcb-preset.json --set pipeline.pixelSort.strength=0.9
node src/cli.js list-presets
```

## Roadmap

The first milestone (still-image web app with upload, preview, presets, randomizers, macro + advanced controls, JSON preset save/load, and original-resolution export) is complete, and the CLI renderer arrived early. Next up:

- video mode: per-frame processing with stable seeds and temporal smoothing
- preset gallery view with thumbnails and notes
- batch image processing through the CLI
- more effect families targeting the camera's digital brain (JPEG/DCT corruption, OSD/datestamp burn-in, sync tear + rolling-shutter wobble, and Bayer/demosaic faults are done): stale-buffer ghosting, amp glow, dead columns, purple fringing, AWB/AE hunting bands, and generational recompression — full implementation notes in `docs/PROJECT_SPEC.md` under "Phase 5: New Effect Families"

## Design Direction

The interface prioritizes usability while borrowing from old digital camera menus: compact controls, mode labels, preset slots, simple macro settings, and deeper configuration panels when needed.

The engine keeps the image-processing pipeline (`src/engine-core.js`) separate from the UI, so the same preset format drives the web app, the CLI renderer, and a future video renderer.
