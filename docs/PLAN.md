# Virtual Camera Bender: Rust rewrite, implementation plan

## Status (2026-11-05)

Milestone 1, branch `rust-rewrite`: stages 1 to 6 are complete and merged (final
commit 92cc811 plus this status update). The plan below stands as approved on 2026-11-04; `docs/ARCHITECTURE.md`
describes what was built. The JavaScript version is tagged `v0.1.0-js` and lives on
branch `legacy-js`.

- Stage 0: sourced A520 constants, `cameras/a520.toml` graded by confidence, the
  post-stage policy.
- Stage 1: workspace and lints, core types, noise, configuration, the time and
  resolution model, the `Simulator` shell, image I/O with EXIF orientation, `render`
  and `inspect`.
- Stage 2: scene preparation with highlight reconstruction, the CCD sensor, the image
  processor (Malvar demosaic) and the JPEG model with the Canon tables; auto-exposure
  meters each frame, and the video AE servo was not built.
- Stage 3: still and movie readout with CTE trails and clock faults, the oversampled
  AFE with injection and gain modulation, the 12-bit ADC.
- Stage 4: the bus bend as a nodal circuit (drivers, DIP bank, capacitor, pot on F or
  S, clamp diodes, 74HC00, 74HC74, target selector, DSP latch).
- Stage 5: video through ffmpeg pipes, `render-video` and `sheet`.
- Stage 6: tuning of the bend presets and provisional constants against the A520
  references (docs/research/a520-tuning-log.md), architecture document, README,
  changelog, divan benchmarks, golden-image tests, doc/code consistency pass.
  Open: the HPF reference's magenta sky needs a bus-side mechanism the passive
  model lacks; the default resolution mode is a user decision (native renders
  downscaled match the references far better than small match-input renders).

## 1. Decisions already made

| Topic | Decision |
|---|---|
| First virtual camera | Canon PowerShot A520 with the documented ADC-bus bend (DIP source bank, 1 µF cap, pot, 74HC00 inverter, 74HC74 divider, target selector). Validated against the reference sky composite (HPF, DIV, INV). |
| Medium | Video-first. The core is a stateful circuit simulation; a still is one frame. Analog state (oscillator phase, RC node voltages, flip-flop, PLL, rail) persists across frames. |
| Readout modes | Both, as a switch: full-sensor still readout and line-skipped movie/live-view readout with MJPEG. |
| Resolution | The virtual sensor matches the input resolution by default ("match input"); a "native" option uses the camera's real sensor size. Physics is expressed in seconds, hertz, volts and per-pixel constants so the look does not change with resolution. |
| Units | Physical units everywhere (kΩ, µF, Ω, Hz, V, MHz, ms, electrons). Presets read like a schematic. |
| Old stylised modules | Not ported wholesale. A curated post stage may exist later; every post module must document why the look cannot come from the physics. Colour comes from the signal chain, never from palette LUTs. |
| "Analog" target | Smooth continuous colour, soft coloured grain, and bandwidth/softness/JPEG character, weighted equally. |
| Compute | CPU (rayon) first; wgpu after the look is validated. |
| Platform | macOS first; code stays portable. |
| UI | Decided after this milestone (egui + wgpu is the default candidate). |
| Workflow | Autonomous spike, user reviews a comparison sheet at the end. Includes a research stage. |
| Presets | Old presets do not carry over; their spirit does. |
| Engineering | Microsoft Pragmatic Rust Guidelines, digested in `docs/RUST-GUIDELINES.md`. |

## 2. Milestone 1: the validated core

Goal: a Rust library and CLI that turn an input photo or video into the output of a
simulated, circuit-bent A520, with the A520 bus bend reproducing the reference HPF,
DIV and INV looks closely enough that the user judges them "analog" by eye.

In scope: `vcb_core` (simulation), `vcb_io` (image and video I/O), `vcb_cli`
(render, render-video, inspect, sheet), camera and preset TOML files, golden tests,
architecture docs, a comparison sheet against the references.

Out of scope for this milestone: GPU path, desktop UI, other cameras and bends
(IR cut, rail sag, master clock reclock, SDRAM address lines), post-stage modules,
WASM, webcam input, MIDI. These are listed in section 7 so the design leaves room.

## 3. Architecture

### 3.1 Crates

```
Cargo.toml                 workspace (edition 2024, shared lints and deps)
crates/vcb_core/           simulation: camera model, stages, circuit solver, ISP, codec
crates/vcb_io/             image decode/encode (EXIF orientation), ffmpeg frame pipes
crates/vcb_cli/            command line (clap, anyhow, mimalloc)
cameras/a520.toml          camera model in physical units
presets/*.toml             camera + readout mode + bends + ISP overrides
docs/ARCHITECTURE.md       end-state design (written in stage 6)
docs/research/             reference material and research notes
```

`vcb_core` performs no I/O, reads no clocks and no entropy: it takes buffers and a
seed. That keeps it testable, deterministic, and ready for WASM and GPU backends.

### 3.2 Signal chain

The chain mirrors the hardware. Each stage is a module with a `Params` struct in
physical units, an optional persistent `State`, and a function that advances one frame.

```
Scene      linear-light RGB at sensor resolution, with highlight headroom
  |  scene.rs       resample input, inverse tone curve, highlight reconstruction,
  |                 vignetting, scene RGB -> sensor RGB (inverse of the ISP colour matrix)
Charge     electrons per photosite, Bayer mosaic
  |  sensor.rs      CFA sampling, exposure, shot noise, dark current, PRNU/DSNU, hot pixels,
  |                 full-well clip, anti-blooming overflow, interline smear (electronic shutter)
Stream     charge samples in readout order with optical-black columns and blanking timing
  |  readout.rs     still vs movie (line skip), readout direction, vertical transfer with
  |                 charge-transfer-efficiency trails, V-pulse stalls/skips, H-register phase glitches
Analog     volts per sample
  |  afe.rs         oversampled per pixel clock: CDS (with clamp-timing skew), conversion gain,
  |                 read noise (white + 1/f), PGA gain, VREF, injected oscillator, first-order
  |                 bandwidth, slew limit, supply-rail hook
Codes      ADC output words per sample
  |  adc.rs         quantisation to the camera's bit depth, pedestal
  |  bus.rs         A520 bend circuit on the data bus (section 3.4), DSP input latch
Raw        active-area Bayer plane after the DSP's black clamp from the optical-black columns
  |  isp.rs         black clamp, white balance (preset or grey-world auto), demosaic
  |                 (bilinear, later Malvar), colour correction matrix, auto-exposure,
  |                 tone curve, chroma noise reduction, luma NR, sharpening, 8-bit
  |  codec.rs       JPEG simulation: 4:2:2 subsampling, 8x8 DCT, quantisation with Canon-style
  |                 tables at the mode's quality, dequantise, chroma upsample
Frame      8-bit sRGB at the readout mode's output size
  |  post/          empty in milestone 1; policy in section 3.6
```

### 3.3 Time and resolution model

- A readout mode defines `frame_period_s`, `vblank_lines`, `hblank_px`, `ob_cols`
  (optical-black columns), and for movie mode a `line_skip` and an output size.
- `line_period_s = frame_period_s / (rows_read + vblank_lines)`.
- `pixel_period_s = line_period_s / (cols + ob_cols + hblank_px)`.
- With "match input" resolution the pixel clock is derived from these so that an
  oscillator at a given Hz always spans the same fraction of a frame, an RC time
  constant always spans the same fraction of a line, and per-pixel noise is constant.
  With "native" resolution the pixel clock equals the camera's documented clock.
- The AFE runs `oversample` sub-steps per pixel period (default 4) in `f64` time;
  blanking intervals advance the analog state with a constant dark-level input.
- Spatial ISP parameters (blur radii, sharpening radius) are fractions of the frame
  width, converted to pixels per frame. JPEG blocks are always 8 by 8 pixels.

### 3.4 The A520 bend circuit

Modelled as a small resistive network solved per sub-step with backward Euler for the
coupling capacitor, not as bit flips:

- ADC output drivers: ideal 0 V / 3.3 V sources behind an output resistance
  (per-pin value with seeded ±10 % variation so that ties resolve deterministically).
- Source DIP: selected pins D11..D2 join node S, optionally through a series patch
  resistor. Node S couples through the capacitor to node F; the pot loads F to ground.
- Function: `bypass` connects F to T directly; `invert` drives T from a 74HC00 inverter
  whose input is F; `divide` drives T from a 74HC74 flip-flop clocked by F.
  Gate inputs have a threshold near Vdd/2 with small hysteresis; outputs have their
  own output resistance; propagation delay is one sub-step.
- Target selector: T joins one pin D11..D4, or ground. `common_bus` merges S and T.
  A pin present in both banks also merges them (the self-clocking divider case).
- The DSP latches each bus line at a fixed fraction of the pixel period by comparing
  the pin voltage to its input threshold plus a few millivolts of thermal noise.
  Lines not touched by the bend carry the ADC code bit unchanged.

The expected emergent behaviour, which is the validation target: contours that follow
the local mean through the cap's high-pass action (HPF reference), horizontal stretch
and streaking from the divider holding state between edges (DIV reference), and
posterised negatives from the inverter driving a high bit (INV reference).

### 3.5 Determinism and noise

- A preset carries a `seed`; every stage derives its own sub-seed from it.
- Spatial noise is a hash of `(x, y, salt, seed)`; temporal noise comes from PCG
  streams advanced deterministically with the sample index and stored in `SimState`.
- Same input, preset, seed and frame index produce byte-identical output.
- Noise is physically placed: shot noise in charge, read noise in volts before the
  bandwidth limit, comparator noise at the DSP input, none of it as per-pixel speckle
  in code space.

### 3.6 Post-stage policy

A post module may be added only with a short written justification in
`docs/POST-STAGE.md` answering: which look it produces, why no stage of the signal
chain can produce it physically, and what it operates on (camera JPEG output only).
Candidates for later: generational recompression (a real workflow artefact), OSD
datestamp burn-in (a DSP overlay, arguably physics), pixel sort (no physical
equivalent, justification required).

### 3.7 Configuration

TOML, parsed with `serde`. A camera file describes the hardware; a preset references
a camera, picks a readout mode, declares bends and overrides. Field names carry units.

```toml
# presets/a520-hpf.toml
camera = "a520"
seed = 1
[readout]
mode = "still"            # or "movie"
resolution = "match-input" # or "native"
[bend.bus]
source_pins = [9, 8, 7]
target = "pin:10"          # or "gnd"
function = "bypass"        # "invert" | "divide"
cap_uf = 1.0
pot_kohm = 10.0
series_ohm = 0.0
common_bus = false
[bend.clocks]
cte = 0.9995
[bend.afe.inject]
wave = "square"
freq_hz = 1200.0
amplitude_v = 0.15
```

### 3.8 Public API sketch (fixed in stage 1, implemented in stages 2 to 5)

```rust
pub struct Camera { /* sensor, timing, afe, adc, isp, jpeg sections */ }
pub struct Preset { pub camera: String, pub seed: u64, pub readout: Readout, pub bend: Bend, /* overrides */ }
pub struct Scene { width: u32, height: u32, r: Box<[f32]>, g: Box<[f32]>, b: Box<[f32]> }
pub struct Frame { width: u32, height: u32, rgb: Box<[u8]> }

pub struct Simulator { /* camera, preset, derived timing, per-stage state, scratch buffers */ }
impl Simulator {
    pub fn builder(deps: impl Into<SimDeps>) -> SimulatorBuilder;   // deps = (Camera, Preset)
    pub fn frame(&mut self, scene: &Scene, index: u64) -> Frame;     // stateful, deterministic
    pub fn timing(&self) -> Timing;                                  // derived clocks for `inspect`
    pub fn reset(&mut self);
}
```

`vcb_io` provides `read_image(path) -> Result<Scene>` (sRGB decode, EXIF
orientation), `write_frame(path, &Frame, Format)`, `VideoReader` and `VideoWriter`
over ffmpeg rawvideo pipes, and `probe(path)`.

## 4. Stages, deliverables, acceptance

Each stage is implemented by a subagent (Opus 5.5, effort xhigh) against a brief that
contains this plan, `docs/RUST-GUIDELINES.md`, the stage's interfaces and acceptance
criteria. The orchestrator verifies and commits. Sizes are rough.

### Stage 0: research consolidation (in progress)

Two research agents are collecting (a) more reference images of real bent CCD
compacts into `reference-images/web/` with a manifest, and (b) A520 technical
constants with sources into `docs/research/a520-technical.md`.

Deliverable: `cameras/a520.toml` filled with sourced numbers (bit depth, black level,
raw dimensions, pixel clock, line period, full well, smear spec, CTE, 74HC thresholds,
colour matrix, JPEG tables). Unknowns get a documented substitute and a `confidence`
note. Owner: orchestrator, after the agents report.

### Stage 1: workspace and foundations (small)

- Workspace with shared lints from the guidelines, `vcb_core`, `vcb_io`, `vcb_cli`.
- `vcb_core`: `Plane<T>`, `Scene`, `Frame`, hash-based noise and PCG streams,
  `Camera`/`Preset`/`Readout`/`Bend` serde types, `Timing` derivation, the `Simulator`
  shell with stage trait and `SimState`, canonical `Error`.
- `vcb_io`: `read_image` with EXIF orientation, `write_frame` (PNG, JPEG), `probe`.
- `vcb_cli`: `render` that passes the image through an identity chain; `inspect` printing
  the derived timing for a preset.
- Tests: determinism of the noise sources, timing derivation (line period and pixel
  period match the formulas; match-input keeps the oscillator lines-per-cycle constant
  across two resolutions), TOML round trip, EXIF orientation on a rotated fixture.
- Acceptance: CI gates pass; `vcb render in.jpg out.png --preset presets/a520-clean.toml`
  reproduces the input.

### Stage 2: the clean camera (medium) — parallel with stage 3

Modules `scene.rs`, `sensor.rs`, `isp.rs`, `codec.rs`.

- Scene: area resampling, inverse tone curve with `headroom`, highlight reconstruction,
  vignetting, inverse-CCM to sensor primaries.
- Sensor: everything in section 3.2 except readout; electronic-shutter smear is a flag
  set by the readout mode.
- ISP: black clamp from optical black, WB (daylight gains or grey-world auto), bilinear
  demosaic, CCM, AE (scene-metered for stills, lagging servo for video), tone curve LUT,
  chroma NR in YCbCr, luma NR, unsharp mask, 8-bit.
- Codec: JPEG simulation with Canon-style tables at Superfine/Fine/Normal and MJPEG
  quality for movie mode; chroma upsampling selectable (nearest for the chunky look).
- Tests: flat field stays flat through demosaic; CCM and inverse round trip; noise-off
  no-bend chain reproduces the input within a small colour error; JPEG at quality 100 is
  near-identity; golden renders at 256 by 192.
- Acceptance: the sky input rendered through the clean A520 looks like a 2005 compact
  JPEG (slightly oversharpened, chroma-blurred, blocky) and not like the source.

### Stage 3: readout, AFE, ADC (medium) — parallel with stage 2

Modules `readout.rs`, `afe.rs`, `adc.rs`.

- Readout: still and movie modes, optical-black columns, readout direction, CTE trail
  model whose trail length scales with transfers travelled, V-pulse stall/skip events as
  rates per second with durations in lines, H-register glitches with wraparound and CFA
  parity consequences.
- AFE: the oversampled analog model of section 3.2, with injection waveforms sine,
  square, saw, sample-and-hold noise; blanking handled as constant-input advance.
- ADC: quantisation, pedestal, code buffer in stream layout.
- Tests: an injected 1 kHz square wave at a 130 µs line period yields about 7.7 lines
  per cycle; a bandwidth of one tenth of the pixel rate softens a one-pixel edge to the
  expected rise; CTE trail length grows with row distance; determinism across runs;
  oscillator phase continues across two consecutive frames (no reset).
- Acceptance: movie mode shows vertical smear through highlights; a lowered CTE melts
  bright objects along the readout direction; injection produces tilted hum bands.

### Stage 4: bus bend circuit (medium) — after stage 3

Module `bus.rs` plus a small `circuit.rs` nodal solver.

- Solver: nodes, voltage sources with series resistance, resistors, one capacitor with
  backward-Euler companion model, ideal switch to ground; dense solve for up to four nodes.
- Gates: inverter and D flip-flop with thresholds, hysteresis, output resistance.
- DSP latch with per-line threshold and thermal noise; pin voltage with patch resistor.
- Tests: two equal drivers meet at half rail; a stronger driver wins; capacitor AC
  coupling decays with the pot time constant; the divider toggles once per rising edge;
  a pin in both banks produces the self-clocking topology; no bend means codes unchanged.
- Acceptance: HPF, DIV and INV presets on the cropped reference sky produce looks that
  match the three reference quadrants in character (contour placement, streak
  direction, colour family), judged on a side-by-side sheet.

### Stage 5: video and CLI (small) — parallel with stage 4

- `vcb_io`: `VideoReader`/`VideoWriter` over ffmpeg pipes, `probe`.
- `vcb_cli`: `render-video` with a stateful `Simulator`, movie/still mode, `--seed`,
  `--start/--duration`, progress on stderr; `sheet` rendering several presets into one
  labelled contact sheet (used for validation now, by the UI later).
- Tests: a synthetic 2-second clip (ffmpeg `testsrc` or a zoom on a still) renders with
  continuous oscillator phase; frame count and dimensions match the probe.
- Acceptance: `vcb render-video` on a short clip with an injection bend shows bands that
  roll continuously, not re-seeded per frame.

### Stage 6: validation, tuning, docs (medium) — orchestrator plus one tuning agent

- Comparison sheet: reference quadrants beside our HPF/DIV/INV renders, plus the clean
  camera, movie mode, melt and injection examples on the sample photos and the new
  web references.
- Tuning pass on component values and ISP parameters to close the gap, with every change
  recorded in the preset files, not in code.
- `docs/ARCHITECTURE.md` (end state), `README.md` (new), `docs/POST-STAGE.md` (policy),
  `cameras/a520.toml` documented.
- Benchmarks with `divan` for AFE, bus, ISP, codec; a timing table in the README.
- Acceptance: the user reviews the sheet and either accepts the look or opens the next
  tuning round.

## 5. Orchestration protocol

- The orchestrator (this session) writes briefs, fixes interfaces, verifies, integrates
  and commits. It does not implement stages itself except glue and the camera TOML.
- Each subagent runs with `model: opus`, `effort: xhigh`, in the repo, on branch
  `rust-rewrite`, restricted to the files of its stage. It must run `cargo fmt`,
  `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`,
  `cargo doc --no-deps`, and report what it built, what it tested, what it could not do.
- Verification per stage: the orchestrator reruns the gates, renders test images, looks
  at them, spot-reads the key files against the guidelines digest, and either accepts or
  returns findings to the same agent via SendMessage.
- One commit per accepted stage, plain message, no attribution trailer (user rule).
  No pushes unless asked.
- Parallelism: stage 1 first; stages 2 and 3 together; stages 4 and 5 together; stage 6
  last. Two agents never edit the same file.

## 6. Risks and open questions

- ADC bit depth: CHDK suggests 10 bits for A-series; the bend write-up labels D11..D2.
  The bus model is width-agnostic; the camera file carries whichever the research supports.
- The CTE trail model is an approximation of a shift register; if it fails to look
  right, an explicit register simulation is affordable in movie mode and parallelisable.
- Highlight reconstruction from an already tone-mapped input is a guess; it is
  parameterised (`headroom`) and documented as such.
- Performance: a 24 MP input with 4x oversampling is roughly 100 M analog steps, about
  one to two seconds on CPU. Acceptable for the CLI; the GPU milestone addresses preview.
- Validation is by eye. A simple metric (colour histogram and gradient-orientation
  statistics against the reference quadrant) will be computed for the sheet but is
  advisory only.

## 7. Later milestones (not for approval now)

1. GPU: wgpu compute for ISP, codec, sensor noise; CPU rows for the circuit stages.
2. Desktop app (egui + wgpu likely): real-time preview, signal-path UI with physical
   knobs, pin-matrix patch bay for the bus bend, camera gallery.
3. More bends from the legacy spirit: IR-cut removal, supply-rail sag, master-clock
   reclock, SDRAM address lines, CDS skew, AWB/AE hunting as servo behaviour
   (milestone 3, done).
4. Milestone 4 candidates, the remaining places a bender can physically reach on a
   CCD compact, ranked by distinctness per effort (direction approved 2026-11-06;
   each ships with its own presets):
   1. **Sensor temperature** (sensor stage): a heat gun on the sensor. Dark current
      doubles every six to eight degrees (Arrhenius on `dark_e_per_s`), hot
      photosites multiply, amp glow appears on long exposures. `[bend.sensor]
      temperature_c`. Trivial and composes with everything.
   2. **Reset gate and output biases** (sensor ribbon, AFE input): a missed or weak
      reset pulse leaves charge on the floating diffusion, so values accumulate along
      the line until they saturate (long drags with hard saturation); a pot on the
      substrate bias changes full well and anti-blooming (blooming spikes, exposure).
      `[bend.ccd] reset_drop_hz`, `reset_frac`, `substrate_v`. Cheap: the constants exist.
   3. **SDRAM data pins and refresh** (memory stage): stuck data bits in the stored
      raw (a posterised cousin of the bus bend after capture) and refresh or clock
      faults, where rows fade as the DRAM charge leaks (stale frames that decay).
      The stage already holds the SDRAM as state.
   4. **AFE serial configuration** (AFE): the processor programs gain, offset and
      clamp over a serial bus; corrupting it makes those change at random between
      writes, the real cause of bands of wrong exposure and colour. Stateful.
   5. **Light leaks** (scene stage): a drilled or opened body fogs the sensor with a
      soft gradient from one edge. Simple.
   6. **JPEG buffer** (codec): the compressed image lives in SDRAM too, so the same
      address and data faults corrupt the bitstream: shifted macroblocks and DC
      prediction errors that tint everything after the fault. Needs the entropy coder
      modelled. The one large item.
   The second lever is exposing camera constants a bender can reach as pots (the
   substrate bias, the AFE gain) rather than adding stages.
5. More cameras (Sony DSC-V1, Fuji S9000, Praktica DC42) as camera files.
6. Post stage per the policy; preset library rebuilt from the old presets' spirit.
7. Capture input, virtual camera output, MIDI/OSC, audio-reactive injection.
8. WASM/WebGPU demo build of `vcb_core`.

## Milestone 2: the desktop app (direction approved 2026-11-05)

Goal: an installable macOS app in which the user loads a photo or a clip, picks a
camera and a preset, turns physical controls and sees the result live, and exports
stills and video. The user tests it directly; sheets are no longer the main surface.

Framework: `eframe`/`egui` 0.37 with the wgpu backend (the frame is uploaded as a
texture; a future GPU path renders straight into it), `rfd` for file dialogs. Native
look is the default resolution; interactive drags render a reduced-size preview and
the release renders at full size.

### Stage 7: core support for interactivity and two model gaps (parallel with 8)

- `Simulator::set_preset(preset)` re-resolves stage parameters without reallocating
  when the readout geometry is unchanged, keeps analog state unless a stage's topology
  changed (bus wiring, oscillator wave), and reallocates otherwise.
  `Simulator::set_scene_size(width, height)` for match-input previews.
- Bus line capacitance and driver slew: `[adc] line_pf` and `driver_slew_v_per_us`,
  so a line mid-transition at the latch instant can read either way; the candidate
  mechanism for the HPF reference's bright sky (tuning log).
- Lagging auto-exposure and auto-white-balance servos for video: `[isp] ae_mode =
  "servo"` and `wb = "servo"` with time constants in seconds, metered on the bent raw
  of the previous frame, as DIGIC does.

### Stage 8: the app shell and the bus-bend front panel (parallel with 7)

Crate `vcb_app`, binary `vcb-app`. Window: viewport in the centre, inspector on the
right, file and transport bar on top, status line at the bottom. Open by drag and drop
or dialog (images and video). A render worker thread owns the `Simulator`; the UI
sends the latest preset and gets frames back (latest wins, stale results dropped).
Preview scale during drags, full render on release, both with timing in the status
line. Zoom, pan, fit; hold a key to see the original. Camera picker from `cameras/`,
preset open and save as TOML, export still as PNG or JPEG. First panel: readout
(mode, resolution, seed) and the bus bend drawn as a front panel: DIP source switches
D11..D2, target selector D11..D4 and GND, bypass/function and invert/divide
switches, pot in kΩ, capacitor in µF, pot node, patch resistor in Ω, common bus.

Visual rules: neutral dark greys, one accent colour, egui's sans font with a clear
size hierarchy, 8-pixel spacing grid, units on every control, physical values shown
as numbers, controls grouped by stage in signal order, no uppercase shouting, no
monospace except numeric readouts, no neon.

### Stage 9: the full instrument and video

Panels for clocks (CTE, stalls, glitches), AFE (injection, gain modulation, CDS skew),
scene (headroom), sensor exposure, ISP overrides (white balance, tone, NR, sharpening),
each in physical units. Video: frame scrubber, play-through preview at reduced size,
movie or still readout, export with progress through `vcb_io`'s writer on a thread.
A signal-path strip showing stage order. Keyboard shortcuts. A macOS app bundle.

### Stage 10: user testing round

The user plays; findings become fixes; `docs/USER-GUIDE.md`; tag `v0.2.0`.

### Status (2026-11-06)

Stages 7, 8 and 9 are complete and merged: the interactive core API with physical
exposure, bus line capacitance and slew (the HPF sky experiment was negative; see the
tuning log), AE/AWB servos; the `vcb_app` desktop app with every panel in signal
order, video scrubbing, play and export, shortcuts, a macOS bundle script and
`docs/USER-GUIDE.md`. Stage 10 is the user's testing round.

## Milestone 3: the bends from the old presets' spirit (started 2026-11-06)

Goal: four more bends, each a physical model in the stage whose hardware it changes,
with their panels in the app and a preset library rebuilt from the legacy presets'
spirit (`docs/legacy/PROJECT_SPEC.md`, phases 7 and 8; the legacy sources are
`src/ir-cut.js`, `src/rail-sag.js`, `src/master-clock.js`, `src/address-bus.js` on
`main`). The legacy modules are the brief for the look, never for the method: their
normalised knobs become bench quantities, their coin flips become circuits with memory.

| Bend | Section | Where it lives | What it is |
|---|---|---|---|
| IR-cut filter removal | `[bend.optics]` | the scene stage (optics, before charge) | The DSC-V1 magnetic bypass: the hot mirror slides off the sensor; silicon sees near infrared through all three dyes. |
| Supply-rail sag | `[bend.rail]` | a new `power` stage before the AFE; the AFE, bus and processor read its rail | A loaded or dying 3.3 V rail: ratiometric droop, lifted pedestal, and brownout of the AFE, the bend's logic and the processor. |
| Master-clock reclock | `[bend.clock]` | a new `capture` stage between the bus and the processor; `Timing` carries two clocks | The LTC1799 mod: the timing generator and AFE run from a detuned oscillator while the processor captures on its own. |
| SDRAM address lines | `[bend.memory]` | a new `memory` stage between `clamp_black` and `develop`, on the raw | Stuck, bridged or marginal address pins on the frame-buffer SDRAM: aliasing in powers of two, stale frames showing through. |

### Decisions

| Topic | Decision |
|---|---|
| The near infrared of the scene | Unknowable from an sRGB photo; synthesised by a documented estimator like highlight reconstruction, with its parameters in the bend section. The scene's auto-exposure then compensates the extra light and white balance fights the cast, so the full-spectrum look emerges from the chain. |
| The sagging rail | One rail, the 3.3 V digital and AFE supply, as a source behind an output impedance with decoupling, loaded by a quiescent current, the bend's own current (measured by the bus bend one frame earlier), seeded load spikes and ripple. Sampled once per line in readout order, linear within the line. The AFE's `rail_v` hook consumes it; failures are thresholds, not dice. |
| Which clock the reclock moves | The timing generator's and the AFE's: the processor keeps its own crystal, generates VD and captures the bus at the nominal pixel period. Every sensor-side constant scales with the bent period as it already does for match-input. A modelling choice, documented. |
| Reclock capture | Sample and hold: the processor latches the word on the bus at its own latch instants; the bus stage's own mid-transition latching keeps its source-period phase (documented approximation). Bayer parity falls where it falls: colour-phase static is the physics, not a separate knob. VD from the processor, so frames stay synchronous and rows beyond VD are lost; optional HD re-synchronisation with a capture window. |
| Address lines | Physical SDRAM pins with multiplexed row and column addresses, so one stuck pin corrupts two bits of the linear address. Writes happen in readout order, reads one frame period later; the SDRAM's content is state, so a marginal contact reads stale frames. Memory outside the raw buffer reads as a fixed seeded pattern. |
| Resolution | Memory faults scale their address weight with the virtual pitch and rows so tiles keep their size as a fraction of the frame; the reclock ratio and the rail's time constants are physical and used as given, line rates being native. |
| Knobs stay live | A changed oscillator frequency, rail load, filter position or address fault is `Reconfigured::Kept`: `same_geometry` ignores them, each stage re-resolves, state carries. New memory faults keep the SDRAM content. |
| App | Four panels: Optics (after Scene), Clock (after Bus), Memory (after Processor), Power at the end of the inspector. The strip gains Clock and Memory stages; Power lights the stages it feeds. |
| Validation | By eye on `samples/` and a clip; no reference images exist for these bends. Each bend ships at least one preset whose look matches the legacy description. |

### 3.9 The models

**Optics.** `[bend.optics]`: `ir_cut_pulled_frac` in `[0, 1]`, the fraction of the frame
height the filter has slid off from the top (1 is removed; the filter's shadow edge is
the geometric penumbra of a lens pupil partly blocked, `[lens] filter_edge_blur_frac`
of the height wide); `nir_weights = [r, g, b]`, the scene's NIR radiance as a weighted
sum of its linear sRGB (default `[0.58, 0.38, 0.04]`, summing to one: red-weighted,
Rayleigh-blue skies dark); `foliage_nir`, the NIR reflectance of chlorophyll relative
to its visible green, keyed on greenness (the Wood effect; default 0, a deliberate
assumption about the scene the user turns on); `nir_scale`, overall NIR radiance
relative to visible for a scene of equal energy (default 1). The scene's NIR is not
recoverable from an RGB photo: the estimator is a documented guess like highlight
reconstruction, and a fourth scene plane of real NIR data would replace it at the
same point with nothing downstream changing (user decision, 2026-11-06). Camera:
`[sensor] nir_response = [r, g, b]`, each channel's charge from unit estimated NIR
relative to the charge reference white gives the green channel (C, dye transmission
beyond 750 nm: PROVISIONAL `[1.0, 0.55, 0.5]`); `[lens] nir_defocus_frac`, the blur
radius of the NIR image as a fraction of the width (IR focuses behind the sensor,
PROVISIONAL 0.003); `[lens] filter_edge_blur_frac` (a 1 mm gap at f/4 over the sensor
height, PROVISIONAL 0.06). In `scene::prepare`, after the colour conversion: the NIR
plane is built from the linear sRGB scene, blurred, multiplied by `nir_response` and
the uncovered fraction, and added to each channel. The scene auto-exposure meters the
sum.

**Power.** Camera `[power]`: `rail_v` 3.3, `source_ohm` (regulator and battery
impedance, PROVISIONAL 0.5), `decoupling_uf` (PROVISIONAL 47), `quiescent_ma`
(PROVISIONAL 60), `afe_uvlo_v` (AD9949 class minimum supply, 2.7, C),
`logic_min_v` (74HC, 2.0, C), `dsp_brownout_v` (PROVISIONAL 2.9), `afe_restart_lines`
(PROVISIONAL 3). Preset `[bend.rail]`: `load_ma` (a steady added load: LEDs, the
LTC1799, a pot to ground), `spike_ma` and `spike_hz` (transients: the flash charger,
a motor; duration `spike_ms`), `ripple_mv` and `ripple_hz` (the converter's or a weak
battery's), `source_ohm` to replace the camera's (a dying battery). The `power` stage
computes the rail per line in readout order from the load, the previous frame's bus
current per line (recorded by `bus::bend` in its state: the current the drivers push
into the pot, the gates and the clamps), the spikes and the ripple, through the RC of
the source impedance and decoupling, in closed form across blanking. Consumers: the
AFE scales its reference and pedestal by `rail_v / nominal` (the existing hook, now per
line); below `afe_uvlo_v` it tristates and the bus holds its last word until
`afe_restart_lines` after recovery; the bus drivers' `v_oh`, the 74HC thresholds and
output levels follow the rail, and below `logic_min_v` the gates go high-impedance and
the flip-flop forgets; below `dsp_brownout_v` the processor captures nothing for the
rest of the frame (zero codes; with the memory bend, stale buffer content). The
capacitor voltage and the spike process persist across frames.

**Clock.** Preset `[bend.clock]`: `osc_mhz` (the replacement oscillator; the camera's
`[still]`/`[movie] pixel_clock_hz` is the nominal), `drift_ppm` and `drift_hz` (slow
seeded wander of an RC oscillator), `ppm_per_v` (supply sensitivity, acts on the rail
if the rail bend exists; LTC1799 class 0.1 %/V, C), `hd_lock_px` (the processor's
line-start capture window in pixels; 0 is free-running). `Timing` gains the sensor-side
`pixel_period_s` (nominal over the ratio) beside the processor's nominal one; sensor,
readout, AFE, ADC and bus stages run on the sensor-side period as they do today; the
frame period is the processor's, and the sensor reads until VD. `capture::reframe`
maps each processor latch instant to the bus word valid then, or to the blanking word
when the timing generator is in blanking, with the optional HD re-synchronisation per
line. The drift oscillator's phase persists; VD restarts the generator each frame. At
match-input the capture runs on the camera's native line and is sampled onto the
virtual grid, so a preview shows the native look downscaled (a virtual line is mostly
blanking). Because VD restarts the generator, a steady oscillator draws the same tears
in every frame; drift moves them. A generator that free-runs across VD (vertical roll)
is a later option.

**Memory.** Camera `[memory]`: `column_bits` (PROVISIONAL 9), `row_bits` (12),
`bank_bits` (2), `raw_base` (word address of the raw buffer, PROVISIONAL 0), one
sample per word (the packed 10-bit layout is not modelled). Preset `[bend.memory]`:
`[[bend.memory.faults]]` with `pin` (the address pin, `a0` upward), `kind` (`"low"`,
`"high"`, `"bridge"` with `with = <pin>`, both pins reading the OR of the two),
and optionally `on_frac` and `switch_hz` for a marginal contact as a random telegraph
process in camera time (absent: solid). `memory::store` writes the clamped raw to the
SDRAM state in readout order through the faulted address, `memory::load` reads it
back one frame period later through the fault as it then is; the SDRAM keeps its
content between frames; addresses outside the raw buffer read a fixed seeded pattern.
At a virtual geometry each fault's two linear weights are scaled with the virtual
pitch and rows and rounded to powers of two.

### Stage 11: schema and plumbing (small, one agent)

The four preset sections in their own files under `preset/` (`optics.rs`, `rail.rs`,
`clock.rs`, `memory.rs`) with defaults, validation and serde round trips; the camera
sections `[power]`, `[memory]`, the new `[sensor]` and `[lens]` keys with PROVISIONAL
values and citations in `cameras/a520.toml`; the `Timing` field for the sensor-side
period (equal to the nominal until stage 12c); modules `power`, `capture`, `memory`
with `Params::resolve` returning `None` without their section and pass-through stage
functions; `Stages`, `States`, `Buffers` and `Simulator::frame` wired in signal order;
`same_geometry` and `set_preset` treating the new sections as kept state; `PanelId`
and strip `Stage` variants with empty panels; config tests. Acceptance: every gate
green, goldens unchanged, `vcb inspect` lists the new sections.

### Stage 12: the four bends (medium each, four agents in parallel)

12a optics owns `scene/`, 12b power owns `power.rs`, `afe.rs`, `bus/`, 12c clock owns
`capture.rs`, `timing.rs`, 12d memory owns `memory.rs`; each owns its `preset/<bend>.rs`
and one test file. Camera value changes are reported to the orchestrator, who applies
them. Each delivers: the physics documented in the module header with formulas and
units, the stage seed with a new salt, time scaling per section 4 of the architecture,
tests of physical properties (the rail of an unloaded camera is the nominal to a
microvolt; a 27 MHz oscillator captures byte-identically to no bend; a solid stuck pin
aliases exactly in powers of two; the NIR of a pure blue sky is below a twentieth of a
white's), replay from reset, `set_preset` keeping state, a contact sheet on
`samples/sample-01.jpg`, and a note on cost per frame.

### Stage 13: the panels (medium, one agent)

Optics, Clock, Memory and Power panels in physical units after the existing panels'
pattern (`Knob`, `override_row`, power switch, camera default dimmed), the strip's new
stages lit when bent, the address-fault editor as a row of pins with a kind per pin,
shortcuts, `docs/USER-GUIDE.md` sections, headless panel tests.

### Stage 14: presets, goldens, docs, tuning (medium, one agent plus the orchestrator)

Presets in the legacy presets' spirit, one file each with a schematic-style header,
these ten names (the guide of stage 13 refers to them):

| Preset | Bend | Look |
|---|---|---|
| `a520-full-spectrum` | optics, grey-world balance | the washed full-spectrum photograph: cream clouds, teal water, pale vegetation |
| `a520-chlorophyll` | optics with `foliage_nir`, low red gain | aerochrome-ish: deep sky and water, foliage glowing pink-white |
| `a520-brownout` | rail, steady load and ripple | breathing bright bands, no failures |
| `a520-sieve` | rail, spikes, no bus bend | rows that die into solid colour, clustered |
| `a520-loose-sync` | clock, small detune, HD lock | a held picture with diagonal shear and a few snaps |
| `a520-reclock-roll` | clock, drift, narrow lock | tears and colour-phase bands that move between frames |
| `a520-s9000-tear` | memory, marginal contacts | torn bands and stale frames showing through |
| `a520-mirror-bank` | memory, one solid row pin | a mirrored band, crystalline |
| `a520-dying-battery` | rail with memory | lockout stripes, then the previous frame below the brownout |
| `a520-full-spectrum-hpf` | optics with the HPF bus bend | the reference bend on a full-spectrum camera | Golden hashes and PNGs, `docs/ARCHITECTURE.md` (sections 3,
4, 6, 8, 9), `CHANGELOG.md`, README preset list, benchmarks for the three new stages,
a tuning note per bend in `docs/research/`. The orchestrator judges each preset by eye
on the samples and a clip.

### Stage 15: user testing round

The user plays; findings become fixes; tag `v0.3.0`.

### Status (2026-11-06, evening)

Stage 11 and stages 12a to 12e are complete and merged on `rust-rewrite` (f489dbb plus
this status): the four bends with their physics, tests and contact sheets, and the
cross-stage connections (the rail drives the reclock oscillator through `ppm_per_v`, a
processor brownout leaves stale SDRAM rows, sub-line constants scale on the processor
clock so a slow sensor clock settles more per pixel, the processor's black clamp
corrects within `[isp] clamp_range`, smear is capped at VD). Values settled during the
stage: `foliage_nir` defaults to 0 and the NIR estimator is labelled an assumption
(user decision), `nir_response` `[1.0, 0.55, 0.5]`, `filter_edge_blur_frac` 0.06,
`dsp_brownout_v` 2.4 (at 2.9 the processor dropped the frame before the AFE could
lock out), `clamp_range` 128 (64 would change the DIV golden), memory pins limited to
`a0` to `a11`. Known limits for stage 14 to work with: large reclock detunes are
violent, readable loose-sync presets need detunes near 0.05 % or drift; every reclock
feels the rail through the default `ppm_per_v`; the HPF bend darkens held words, so a
chromatic-sieve preset wants the rail bend without it; the bend's own pot current only
loads the rail from the next frame on. Next: stage 13 (panels) and stage 14 (presets,
goldens, docs, benches for the new stages).

### Status (2026-11-06, night)

Stages 13 and 14 are complete and merged (3f033ef plus this status): the Optics,
Master clock, Memory and Power panels with the strip lighting every bent stage and
headless tests of load, apply, power switches, the pin grid and reachability; the ten
presets of the table above with goldens, `vcb presets` and CLI tests; benches for the
power, capture and memory stages; `docs/ARCHITECTURE.md`, `CHANGELOG.md`, `README.md`,
`docs/USER-GUIDE.md` and `docs/research/milestone3-tuning-notes.md`. Looks stage 14
could not fully reach without core changes, for milestone 4 or a fix round: brownout
bands are soft (the lockout caps the ratiometric brightening at 22 %); a locked-out
AFE holds one word, so sieve stripes have one hue (a latch-up to a rail code would
vary them); the mirror bank is a still look (movie rows lie below the pin's bit); a
vertical roll needs a timing generator that free-runs across VD. Stage 15 is the
user's testing round; nobody has clicked the four new panels by hand (drop-downs,
typed values and tooltips were checked only headless and in screenshots).

## Milestone 4: the rest of the bender's reach (started 2026-11-06)

Goal: the bends of section 7 item 4, in the order of their ranking, each with its own
presets, panels and docs, built while milestone 3 is in the user's hands. The cheap
sensor-side pair first (stages 16 and 17, in parallel, both in `vcb_core`), their
panels, presets and docs next (stage 18), then the memory and AFE items (stage 19)
and the JPEG buffer (stage 20) once the user's stage 15 findings are in.

### 3.10 The models

**Sensor bend, `[bend.sensor]`** (sensor stage, stage 16). Two things a bender does to
the sensor itself. `temperature_c` (default the camera's `reference_temp_c`): the dark
current follows the Arrhenius rule, doubling every `dark_doubling_c` degrees (C, 6 to 8
for CCD silicon; `dark_e_per_s` and the hot photosites' multiples scale together), the
read noise rises `read_noise_per_c` per degree (PROVISIONAL), and the output amplifier's
glow, `amp_glow_e_per_s` of extra dark current at its corner falling off over
`amp_glow_radius_frac` of the width (PROVISIONAL; the corner is the readout's first
sample), scales the same way, so long exposures at temperature glow in that corner.
`substrate_v` (default the camera's `substrate_v`, C-class 12 V for Sony interline
parts): a pot on the substrate bias moves the vertical overflow drain's barrier; the full
well falls by `full_well_frac_per_v` per volt above nominal (PROVISIONAL, half the well
at +2 V), so a raised bias clips while the drain takes the excess; a lowered bias
deepens the well and raises the barrier until the drain no longer takes the excess
(its capacity is gone `drain_loss_v` below nominal, PROVISIONAL 2 V), so highlights
bloom along the columns, and the electronic shutter, which pulses the substrate,
stops clearing in movie mode (smear grows). The glow scales with `k − 1`, since the
characterised dark at the reference temperature already includes the amplifier's
share (goldens unchanged). Camera `[sensor]`
gains `reference_temp_c`, `dark_doubling_c`, `read_noise_per_c`, `amp_glow_e_per_s`,
`amp_glow_radius_frac`, `substrate_v`, `full_well_frac_per_v`, `drain_loss_v`. State: none. The
optical-black clamp removes the mean dark, so what shows is grain, hot crosses, the
glow's gradient and clipping, as on a real hot sensor.

**Reset bend, `[bend.reset]`** (AFE stage, stage 17). The reset gate pulse that empties
the floating diffusion before each packet is a wire on the sensor ribbon. With
correlated double sampling, a missed or weak reset cancels in the difference, so what a
bender sees is the output stage running out of range: with `drop_hz` missed pulses per
second, each lasting `hold_px` pixel periods (geometric, mean), the diffusion keeps
accumulating packets until the source follower's swing `fd_swing_v` (C, the AD9949's
1.0 V p-p CDS input is the matching limit; the CCD's own swing PROVISIONAL 1.0 V) is
used up, after which packets are lost: the CDS reads zero (black) until the next reset.
A weak reset, `reset_frac` in `[0, 1]` (1 is a full reset), leaves that fraction of the
diffusion's charge behind each pixel, so the diffusion walks towards saturation in
bright runs at a rate set by the scene, and bright streaks end in black. The model
lives in the AFE's CDS step: a state for the diffusion's charge carried across pixels,
lines and frames (it empties in blanking only if a reset arrives), seeded drops from the
stage seed with a new salt. Camera `[afe]` gains `fd_swing_v`.

### Stage 16: the sensor bend (small, one agent)

`preset/sensor_bend.rs` (`SensorBend`, section `[bend.sensor]`), the camera keys with
letters and citations, the model in `sensor.rs`, tests (dark doubling per
`dark_doubling_c`; hot photosites scale with it; the glow is strongest at the readout's
first corner and grows with exposure; a raised substrate bias clips a grey card that was
unclipped and spills along columns; inert values byte-identical; replay; `Kept`), a sheet
(heat gun at 45 and 70 °C with a long exposure; substrate +1.5 V and −1.5 V).

### Stage 17: the reset bend (small, one agent)

`preset/reset.rs` (`ResetBend`, section `[bend.reset]`), `[afe] fd_swing_v`, the model
in `afe.rs`, tests (a dropped reset over a bright run goes black after the swing is used
up, a dark run survives; `reset_frac` 1 is byte-identical to no bend; the diffusion's
state carries across lines and frames; replay; `Kept`), a sheet (drops at a few per
second; `reset_frac` 0.7).

### Stage 18: panels, presets, docs for both (medium, two agents)

A Sensor panel section for temperature and substrate bias and a Reset section in the AFE
panel (app agent, with the guide); presets `a520-heat-gun`, `a520-substrate`,
`a520-reset-drag` and one combination with goldens, benches, architecture, changelog,
README and tuning note (docs agent). The strip lights Sensor and AFE for them.

### Stage 19: memory data pins and refresh, AFE serial, light leaks

Specified after stage 15's findings; models per section 7 item 4.

### Stage 20: the JPEG buffer

The entropy coder in the codec stage, then address and data faults on the compressed
buffer. Specified when stage 19 lands.
