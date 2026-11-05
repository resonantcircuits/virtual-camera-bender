# Virtual Camera Bender

Virtual Camera Bender simulates a circuit-bent early-2000s CCD compact camera, starting
with the Canon PowerShot A520 and the documented bend soldered across its ADC data bus.
A photo or video goes back to being light and travels through a physical model of the
camera: charge in the sensor, volts in the analog front end, bits on the bus, the bend
circuit's capacitor and logic gates, the image processor and the JPEG encoder. The
glitches are not filters; they come out of the circuit, configured in ohms,
microfarads and hertz.

The reference for the A520 bend is a set of photos from a real modified camera. The
validation sheet puts our HPF, DIV and INV renders beside them; it is built by
`scripts/validate-a520.sh` from local reference images into the gitignored
`validation/` folder, so no sheet is committed.

## Build and run

You need Rust 1.93 or later. Video also needs `ffmpeg` and `ffprobe` on `PATH` (or
named by `VCB_FFMPEG` and `VCB_FFPROBE`).

```sh
cargo build --release
```

The binary is `target/release/vcb`. Run it from the repository root, where it finds
camera files in `cameras/` (or pass `--camera-dir`, or set `VCB_CAMERAS`).

Render a photo through a preset; the output extension picks PNG or JPEG:

```sh
vcb render samples/sample-01.jpg hpf.png --preset presets/a520-hpf.toml
```

Render a video through one stateful camera, so oscillator phase and circuit state
carry from frame to frame; `--mode movie` switches to the line-skipped movie readout:

```sh
vcb render-video clip.mp4 bent.mp4 --preset presets/a520-div.toml --mode movie --start 2 --duration 5
```

Render one image through several presets into a labelled contact sheet, with optional
reference images appended as extra tiles:

```sh
vcb sheet sheet.png --input samples/sample-01.jpg \
  --preset presets/a520-clean.toml presets/a520-hpf.toml presets/a520-div.toml presets/a520-inv.toml
```

Print the clocks a preset derives for a given input, such as the pixel period, line
period and frame rate of the virtual sensor:

```sh
vcb inspect --preset presets/a520-hpf.toml --input samples/sample-01.jpg
```

`vcb presets` lists the preset files and their bends. Every rendering command accepts
`--mode still|movie`, `--resolution match-input|native` and `--seed` to override the
preset; `render-video` also takes `--max-dimension`, `--quality`, `--codec h264|prores`
and `--fps`. `vcb help <command>` lists the rest.

## Desktop app

`cargo run --release -p vcb_app -- samples/sample-01.jpg` opens the app (binary
`vcb-app`, Rust 1.95 or later) with the sample photo in front of the camera; without a
file, open an image or a video with "Open…" or drop it on the window. Run it from the
repository root so it finds `cameras/` (or set `VCB_CAMERAS`) and the `presets/` list,
or build `target/Virtual Camera Bender.app` with `scripts/bundle-macos.sh`, which
carries both. The inspector holds every stage in signal order, in physical units:
scene, sensor, readout, the clock and analog-front-end bends, the bus bend as a front
panel and the processor overrides, under a signal-path strip. Dragging a control shows
a 720-pixel preview and releasing it renders the full frame; the camera keeps running
between renders until "Reset circuit". Videos get a scrubber, frame steps and play
through the preview camera, and "Export video…" renders a clip to H.264 or ProRes in
the background. `docs/USER-GUIDE.md` explains every control and the shortcuts.

## Cameras and presets

A camera file (`cameras/a520.toml`) describes the hardware in physical units: sensor
size, full well and conversion gain, pixel clock and blanking, AFE gain and bandwidth,
ADC bits and black level, the 74HC logic, the image processor's colour matrix and the
measured JPEG tables. Every value carries a confidence letter and a pointer to its
source in `docs/research/a520-technical.md`. A preset (`presets/*.toml`) names a
camera, picks the readout mode and resolution, and declares the bends patched into it:
which data lines feed the DIP bank, the capacitor and pot values, the inverter or
divider, the target pin. It reads like a schematic. The four presets are the clean
A520 and the three reference bends: `hpf` (high-pass contours), `div` (divider
streaks) and `inv` (inverted, posterised colour).

## Repository layout

```
crates/vcb_core/   the simulation library: configuration, timing, stages, bus circuit,
                   image processor, JPEG model; no I/O
crates/vcb_io/     image decode and encode with EXIF orientation, ffmpeg video pipes
crates/vcb_cli/    the vcb command line
crates/vcb_app/    the vcb-app desktop app
cameras/           camera files
presets/           preset files
scripts/           validate-a520.sh, the A520 validation sheet
samples/           a sample photo
diagrams/          the A520 bend schematic
docs/              architecture, plan, engineering standards, post-stage policy,
                   research notes, the legacy JavaScript documentation
```

How the simulation is built, stage by stage, is in
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md); the plan and the roadmap are in
[docs/PLAN.md](docs/PLAN.md).

## Status

This is the first milestone of the Rust rewrite: the validated core and the command
line. It simulates the A520 in still and movie modes with the bus, clock and analog
front-end bends. The look of the bends is being tuned against the reference photos.
A GPU path, a desktop app with a patch-bay interface, more cameras and more bends are
later milestones.

## Legacy version

The earlier JavaScript version, a browser editor with stylised effect modules, is
tagged `v0.1.0-js` and lives on the `legacy-js` branch. Its README is kept in
`docs/legacy/README-js.md`. Its presets do not carry over.

## Licence

MIT; see [LICENSE](LICENSE).
