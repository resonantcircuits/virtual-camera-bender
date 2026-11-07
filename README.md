# Virtual Camera Bender

Virtual Camera Bender simulates a circuit-bent early-2000s CCD compact camera, starting
with the Canon PowerShot A520 and the documented bend soldered across its ADC data bus.
A photo or video goes back to being light and travels through a physical model of the
camera: charge in the sensor, volts in the analog front end, bits on the bus, the bend
circuit's capacitor and logic gates, the image processor and the JPEG encoder. The
glitches are not filters; they come out of the circuit, configured in ohms,
microfarads and hertz.

Beside the bus bend, the CCD's clock faults and an oscillator injected into the analog
front end, the camera can be bent many more ways, each modelled in the hardware it
changes: the IR-cut filter slid off the sensor, so the silicon sees near
infrared; a light leak in the body, a glow from one edge that bypasses the lens, the
shutter and the filter; a heat gun on the CCD, whose dark current doubles every few
degrees into grain, hot crosses and the output amplifier's glow, and a turned
substrate bias that clips highlights or lets them bloom along the columns; a weak,
flaky reset gate on the CCD's output, which turns bright runs black and solarises the
sky; a noisy contact on the analog front end's serial programming link, which flashes
wrong gains, tints and green mazes into single frames; a sagging, rippling supply rail
that blooms rows, locks the analog front end out and browns the processor out; a
replaced master oscillator that the image processor no longer keeps in step with; and
the frame-buffer memory, whose faulted address pins fold the frame in powers of two
and let earlier frames through, whose faulted data pins posterise it and lift its
black, and whose cells, under a heat gun, lose their bits before the refresh.

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
scene and the IR-cut filter, the sensor with its temperature, substrate bias and
light leak, readout, the transfer-clock and analog-front-end bends with the reset
gate and the serial link, the bus bend as a front panel, the master clock, the
processor overrides, the SDRAM's temperature, address pins and data pins and the
supply rail, under a signal-path strip that lights every bent stage. Dragging a control shows
a 720-pixel preview and releasing it renders the full frame; the camera keeps running
between renders until "Reset circuit". Videos get a scrubber, frame steps and play
through the preview camera, and "Export video…" renders a clip to H.264 or ProRes in
the background. `docs/USER-GUIDE.md` explains every control and the shortcuts.

## Cameras and presets

A camera file (`cameras/a520.toml`) describes the hardware in physical units: sensor
size, full well and conversion gain, the dyes' near-infrared response, the dark
current's temperature law and the substrate bias, pixel clock and blanking, AFE gain,
bandwidth, the output stage's swing and the serial registers' widths, ADC bits and
black level, the 74HC logic, the supply rail and its failure thresholds, the image
processor's colour matrix and black clamp, the frame-buffer SDRAM's organisation and
its cells' retention, and the measured JPEG tables. Every value carries
a confidence letter and a pointer to its source in `docs/research/a520-technical.md`.
A preset (`presets/*.toml`) names a camera, picks the readout mode and resolution, and
declares the bends patched into it: which data lines feed the DIP bank, the capacitor
and pot values, the inverter or divider, the target pin; how far the IR-cut filter has
slid; the light leaking into the body; the sensor's temperature and substrate bias;
the reset gate's missed and weak pulses; the serial link's bit errors; the loads on the
rail; the replacement oscillator; the faulted address and data pins and the SDRAM's
temperature. It reads like a schematic, and its header says what is wired and why.

The presets, all of the A520 (`presets/a520-<name>.toml`):

| Preset | Bend | Look |
|---|---|---|
| `clean` | none | the unmodified camera |
| `hpf` | bus, bypass | high-pass contours, the reference bend |
| `div` | bus, divider | divider streaks |
| `inv` | bus, inverter | inverted, posterised colour |
| `full-spectrum` | IR-cut filter removed | the washed full-spectrum photograph: cream clouds, teal water, pale vegetation |
| `chlorophyll` | IR-cut filter removed, low red gain | aerochrome-ish: deep sky and water, foliage glowing pink-white |
| `full-spectrum-hpf` | IR-cut filter removed and the HPF bus bend | the reference bend on a full-spectrum camera |
| `brownout` | rail, steady load and ripple | breathing bright bands, no failures |
| `sieve` | rail, load spikes | rows that die into solid colour, clustered |
| `dying-battery` | rail and frame buffer | lockout stripes, then the frame buffer's earlier content below the brownout |
| `loose-sync` | master clock, small detune, line lock | a held picture that shears and snaps back |
| `reclock-roll` | master clock, wide drift, narrow lock | tears and colour-phase bands that move between frames |
| `s9000-tear` | frame buffer, marginal contacts | torn bands and stale frames showing through |
| `mirror-bank` | frame buffer, one solid row pin | a crystalline repeated band (stills only) |
| `heat-gun` | sensor at 120 °C, scene auto-exposure | mottled shadows and hot crosses in daylight; on a dark, long exposure a glow in the readout corner |
| `substrate` | substrate bias 2 V below nominal, overexposed | highlights that bloom along their columns |
| `reset-drag` | reset gate, weak pulses and runs of missed ones | bright runs ending in black streaks, a solarised sky |
| `hot-reclock` | master clock as `loose-sync`, sensor as `heat-gun` | the loose-sync look on a hot, grainy sensor |
| `stuck-bit` | frame buffer, data pin d6 stuck high | posterised colour, black lifted |
| `hot-board` | sensor and frame buffer at 120 °C, scene auto-exposure | the heat gun's grain with a fifth of the samples' bits lost, in fine stripes |
| `serial-noise` | AFE serial link, one bit in a hundred flipped | a movie that flashes wrong gains, tints, green mazes and dropped frames; the still shows a green maze |
| `light-leak` | daylight leak from one edge, scene auto-exposure | a pink glow over a stopped-down scene |
| `open-body` | red plastic leak with the IR-cut filter removed, grey-world balance | a peach glow under a teal full-spectrum sky |

`hpf`, `div` and `inv` are tuned against photos of a real bent A520; the others are
tuned by eye, since no reference images exist for those bends
(`docs/research/milestone3-tuning-notes.md`, `docs/research/milestone4-tuning-notes.md`).

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

The Rust rewrite has its validated core, the command line and the desktop app
(milestones 1 and 2). Milestone 3 adds the optics, supply-rail, master-clock and
frame-buffer bends beside the bus, CCD-clock and analog-front-end bends, with ten
presets, tested by the user and tagged `v0.3.0`. Milestone 4 adds the sensor,
reset-gate, frame-buffer data-pin and retention, serial-link and light-leak bends with
nine presets; the JPEG buffer follows. A GPU path and more cameras are later
milestones.

## Legacy version

The earlier JavaScript version, a browser editor with stylised effect modules, is
tagged `v0.1.0-js` and lives on the `legacy-js` branch. Its README is kept in
`docs/legacy/README-js.md`. Its presets do not carry over.

## Licence

MIT; see [LICENSE](LICENSE).
