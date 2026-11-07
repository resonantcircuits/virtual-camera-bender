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
for CCD silicon; `dark_e_per_s`, the hot photosites' multiples and the dark-current
non-uniformity, `dark_nonuniformity` of the site's dark charge, scale together, while
`dsnu_e` stays a fixed readout offset), the
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
`amp_glow_radius_frac`, `substrate_v`, `full_well_frac_per_v`, `drain_loss_v`,
`dark_nonuniformity`. State: none. The
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

### 3.11 The models of stage 19

**Memory data pins and retention, `[bend.memory]`** (memory stage, stage 19b). Two
more things a bender reaches on the frame-buffer SDRAM. The data pins:
`[[bend.memory.data_faults]]` with `pin` (`d0` upward, the SDRAM's `[memory]
data_pins` DQ lines, 16 on the x16 part), `kind` (`low`, `high`, `bridge` with
`with`, the OR of the two as the address faults) and the optional marginal contact
(`on_frac`, `switch_hz`, the same telegraph process, sampled per line at the burst). A
word holds the clamped sample as the integer the DMA wrote, the processor's
`dsp_bits`-bit code with the row's black level added back and rounded, in the word's
low bits (the packed layout stays unmodelled, one sample per word, so on the A520 the
pins `d10` to `d15` carry no sample bit: a fault on them is accepted and inert, and the
panel says so). A fault acts at the write, so the stored word takes the faulted bit,
and at the read, through the contact as it then is: a solid fault reads stuck whatever
was stored; a marginal one leaves a corrupted bit in the word until the next write, so
a clean read of a stale word shows it. The black clamp has run before the store, so a
stuck bit shifts the black with the signal: `d9` high puts every sample in the top
half, `d9` low folds the highlights down, `d5` high lifts black by 32 codes. Data
faults are per sample and need no scaling with the geometry; they apply to the movie
raw as to the still. The retention: `temperature_c` (default the camera's `[memory]
reference_temp_c`), a heat gun on the board. A DRAM cell's charge leaks, and the
controller refreshes every row every `refresh_ms` (C, JEDEC, 64 ms); the cell's
retention `τ = retention_s * 2^(-(T - T_ref) / retention_halving_c) * e^(retention_spread * N)`,
`N` a unit normal hashed from the word address, the bit and the camera's `unit_seed`
(the chip's own map of weak cells), halves every `retention_halving_c` degrees (C,
about 10 °C for DRAM) around a median `retention_s` at the reference (PROVISIONAL
128 s at 25 °C: a median of 2 s at the JEDEC limit of 85 °C) with a log-normal spread
`retention_spread` (PROVISIONAL 0.5). A cell with `τ` below the refresh period loses
its bit before the refresh comes, and the refresh then latches what it finds, so at
the read a frame period later the cell reads its leakage state whatever and whenever
the write was; a cell with `τ` above the refresh period is refreshed in time, every
time, and never decays (the sliver between the frame period and the refresh period is
ignored). True cells fall to 0 and anti cells rise to 1, by the parity of the SDRAM
row (PROVISIONAL: the folded bit-line layout alternates them). The stage keeps, per
word of the span, the masks of the bits that fall and rise at the current
temperature, recomputed when the temperature or the span changes (a kept
reconfiguration), and applies them to every word read, stale words included. At the
reference temperature no cell of the chip reaches the refresh period (the hashed
tail does not reach seven sigma), so the default is byte-identical; heating the
board shows isolated bit errors first, bright and dark single sites whose weight is
the bit's, then a frame eaten by them: with the numbers above about a thousandth of
the sites at 110 °C and a fifth at 120 °C. A section with `temperature_c` alone runs
the store and the load without faults and renders the camera's bytes.

**AFE serial corruption, `[bend.serial]`** (AFE stage, stage 19c). The processor
programs the AFE over its three-wire serial interface (SL, SCK, SDATA; AD9949 class,
research section 5): each write an 8-bit register address (C) followed by a data
field of `[afe] serial_data_bits` (PROVISIONAL 24, shorter registers padded) that the
chip latches into the register and applies at the next VD (C for the family, whose
registers are VD-updated unless held; PROVISIONAL for the A520's use of it). Modelled
registers, with PROVISIONAL addresses: control at 0x00 (bit 0 power-down, bit 1
output disable), the VGA gain at 0x01 (C; `vga_bits` 10, `vga_min_db + vga_db_per_code
* code`, C: 6 dB and 0.0351 dB per code), the four PxGA gains at 0x02 to 0x05 (R, Gr,
Gb, B in the mosaic's order; `pxga_bits` 6 over 0 to `pxga_max_db` 18 dB, C for the
range, PROVISIONAL for the width), the clamp level at 0x06 (`clamp_bits` 8, codes of
the converter's full width, C). Their nominal contents are the camera's: `pga_gain_db`,
`pxga_gain_db` (new, PROVISIONAL `[0, 0, 0, 0]`: Canon balances white in the
processor) and `pedestal_v` through `code_v`. The processor rewrites the whole set in
every vertical blanking, before the frame's first line (PROVISIONAL: a firmware that
writes only changes would make a corrupted register stick until the next change).
The bend is a noisy contact on SDATA: `bit_error`, the probability that a bit of a
write, address or data, reads wrong at the chip, drawn per bit from a [`Pcg32`] seeded
with the bend's stage seed (salt `"serial"`) and the frame index. A write whose
address survives lands its data, flips and all, truncated to the register's width;
a write whose address flips lands on the register at the flipped address if it is
one of the seven and is lost otherwise, and the intended register keeps its content,
the last frame's, corrupted or not, until a clean write: the registers are [`State`],
carried across frames and kept on reconfiguration, nominal at reset. A clean write
restores the camera's exact value, not its quantised code, so `bit_error = 0` renders
the camera's bytes. At VD the registers take effect for the frame: the VGA gain
replaces `pga_gain_db`; each PxGA gain multiplies the samples of its mosaic phase
(the AFE learns each sample's colour from the readout line's image row and the
sample's column, the optical-black columns taking the phase of their position) in
the PGA step; the clamp level sets the pedestal; a power-down or output-disable bit
tristates the outputs for the frame, so the bus holds its last word (the lockout
path). The timing-core registers are not modelled: a write that lands on them is
lost here, where the chip would lose the frame. The look: with `bit_error` around
a thousandth most frames are clean and now and then one is a stop or three bright or
dark (a VGA bit), tinted or checkered (a PxGA bit, the two greens apart give the
demosaic a fine maze), or one code (a control bit); higher error rates corrupt every
frame, and a lost write makes the previous frame's fault persist.

**Light leak, `[bend.leak]`** (sensor stage, stage 19d). A hole drilled in the body,
an opened seam, a loosened lens mount: light enters behind the lens, bypassing the
aperture, the shutter and the IR-cut filter, and falls on the sensor from one edge.
Unfocused and scattered by the cavity, it is a soft glow brightest at the opening:
irradiance `E = radiance * e^(-d / spread_frac)`, `d` the distance from `edge`
(`"left"`, `"right"`, `"top"`, `"bottom"` in the sensor's landscape orientation, like
the glow's corner; the simulator turns a portrait frame back afterwards) as a
fraction of the frame's extent along that axis (PROVISIONAL shape: a diffuse opening;
a point hole's `h x / (h² + x²)²` is not modelled), `radiance` in multiples of
reference white's visible radiance, `colour = [r, g, b]` the light's linear sRGB
relative to white (default `[1, 1, 1]`, daylight; a plastic wall passes red), `nir`
the light's near infrared relative to its visible (default 1, daylight), added
through `[sensor] nir_response` since the hot mirror is bypassed, so a daylight leak
develops pink under the daylight balance like the full-spectrum look. The charge
`E[channel] * white_fraction * full_well_e * exposure_s / nominal_exposure_s`, in
sensor primaries through the camera's matrix and the inverse daylight gains as the
scene is, is not scaled by `exposure_scale`, which stands for the aperture; it joins
the photo-charge before PRNU, dark, shot noise, the well and the smear, so it blooms,
clips and smears like scene light. In movie mode it smears through the electronic
shutter with the scene; in still mode the mechanical shutter blocks the scene during
readout but not the leak, so the leak alone smears, the smear formula on its column
means (PROVISIONAL: the leak's light reaches the vertical register as the scene's
does). Scene auto-exposure meters through the sensor, so it sees the leak: the
exposure scale becomes `(target - leak) / scene` within its two stops and at its
minimum where the leak alone exceeds the target; the servo meters the bent raw and
needs nothing. No state, no randomness; every value is a kept knob. Camera: nothing
new.

### Stage 19a: schema and plumbing (small, one agent)

`preset/serial.rs` (`SerialBend`, `[bend.serial]`), `preset/leak.rs` (`LeakBend`,
`[bend.leak]`, with an `Edge` enum), `data_faults` and `temperature_c` in
`preset/memory.rs` (a `DataFault` sharing the contact validation, pins unique
among the data faults), the `Bend` fields, validation, serde round trips; the camera
keys `[memory] data_pins`, `reference_temp_c`, `retention_s`, `retention_halving_c`,
`retention_spread`, `refresh_ms` and `[afe] serial_data_bits`, `vga_bits`,
`vga_min_db`, `vga_db_per_code`, `pxga_bits`, `pxga_max_db`, `clamp_bits`,
`pxga_gain_db` with letters and citations in `cameras/a520.toml` and the camera
model; `vcb inspect` naming the sections; config tests. The stages' `Params` are not
touched: each physics agent reads its section. Acceptance: every gate green,
goldens unchanged.

### Stage 19b, 19c, 19d: the three bends (medium, small, small; three agents in parallel)

19b owns `memory.rs`, `tests/memory.rs` and the memory bench; 19c owns `afe.rs`,
`simulator.rs` where the AFE's parameters are resolved, and a new `tests/serial.rs`;
19d owns `sensor.rs` and a new `tests/leak.rs`. Each delivers the physics in the
module header with formulas and units, tests of physical properties (a solid `d9`
high puts every sample at or above 512; a marginal data contact leaves its bit in a
stale word; the chip at the reference temperature is byte-identical and at 120 °C
loses about a fifth of its sites, the dead cells the same in every frame and preset;
`bit_error` 0 is byte-identical; a flipped VGA MSB brightens a frame by 18 dB; a lost
write keeps the last frame's register; a leak from the left is brightest in column 0
and falls to `1/e` at `spread_frac`; a leak smears in still mode and the scene does
not; scene auto-exposure takes the leak back), replay from reset, `set_preset`
keeping state, a contact sheet on `samples/sample-01.jpg`, and a note on cost.

### Stage 19e: panels and guide (medium, one agent)

A data-pin grid and a temperature knob on the Memory panel, a "Serial link" block
on the AFE panel, a "Light leak" block on the Sensor panel, the strip lighting the
stages for the new sections, headless tests, `docs/USER-GUIDE.md` sections.

### Stage 19f: presets, goldens, benches, docs (medium, one agent plus the orchestrator)

Presets with schematic-style headers and goldens: `a520-stuck-bit` (a solid data pin:
posterised, black lifted), `a520-hot-board` (the SDRAM and the sensor under one heat
gun), `a520-serial-noise` (the AFE's link corrupted: flicker, tints, mazes),
`a520-light-leak` (a daylight leak from the left on a stopped-down scene) and
`a520-open-body` (a red plastic leak with the full-spectrum optics); benches for the
new steps; `docs/ARCHITECTURE.md`, `CHANGELOG.md`, README, the milestone 4 tuning
note. The orchestrator judges each preset by eye on the samples and a clip.

### 3.12 The model of stage 20: the JPEG buffer

**The entropy coder** (codec stage). The camera's encoder is baseline sequential
JPEG. After quantisation each block's coefficients are coded: the DC as the
difference to the previous block of the same component, in a size category with the
category's extra bits; the ACs in zigzag order as (run, size) symbols with extra
bits, with the end-of-block and zero-run-of-sixteen symbols; through the Huffman
tables of the standard's Annex K.3 (PROVISIONAL: the A520's own tables were not read
from its files; the sample on file and most compacts of the time use them); in MCU
order (for 4:2:2 two luma blocks, then Cb and Cr; 4:2:0 four; 4:4:4 one of each);
with byte stuffing (0xFF then 0x00); and, with `[jpeg] restart_interval` MCUs
(PROVISIONAL 0, no restart markers: Canon's files are believed to carry none), an
RSTn marker and a DC-predictor reset after each interval. The coded scan, headers
aside, is the buffer the processor holds in the SDRAM until the card writer takes
it. The coder is exact: without faults the decoder returns the quantised
coefficients, the frame's bytes are those of the present round trip and the goldens
stand. It runs only while the preset has `[bend.memory]`, since only then can the
buffer be corrupted; a few tens of milliseconds at native size, measured by its bench.

**The buffer in the SDRAM** (memory stage). The scan is written at `[memory]
jpeg_base` (PROVISIONAL 4,194,304 words, 2^22, beyond the still raw buffer), two bytes
per 16-bit word, low byte first (PROVISIONAL), in stream order, through the address
faults and data faults of `[bend.memory]` exactly as the raw is. The encoder's DMA
writes it after the frame's development, which follows the raw's read-back, and the
card writer reads it a frame period later; the marginal contacts are sampled at the
write and at the read of each burst of a readout line's worth of words, as for the
raw's lines (PROVISIONAL timing). An address fault folds the scan onto itself, so the
reader finds one chunk where two were written, and sends chunks into words the frame
did not write, where the power-on content or a stale frame's scan lies; a data fault
flips the same bit of every word, a bit of every other byte of the scan; the
retention masks act on the buffer's words too. The pins are the same as the raw's, so
a fault acts on both buffers, as it would in the camera; which addresses it reaches
is the memory map's doing: on the A520 at native still size the raw occupies
addresses below 2^22, so a fault on `a11` (row bit 22) stuck low folds the JPEG
buffer down onto the raw's words after the raw has been read and leaves the raw
alone, stuck high sends the raw into the JPEG buffer's words, where the scan
overwrites it before the read-back; the movie raw ends below 2^19, so `a9` to `a11`
leave it alone and reach only the scan. The fault's linear weights scale with the
geometry as the raw's, and `jpeg_base` moves with the row bits as `raw_base` does.

**The decoder** (codec stage). The file is read by a decoder of libjpeg's kind, and
the model decodes the corrupted scan as libjpeg does, because that is what a bender
sees. A corrupted bit mis-decodes a Huffman code, and from there the scan is read out
of step: wrong sizes, wrong runs, blocks ending early or late, so the macroblocks after
the fault shift sideways, and the DC differences accumulate wrongly, tinting and
shading everything after it. The rules, libjpeg's: a code that matches no symbol
within sixteen bits is a bad code, the symbol reads 0 (a zero DC difference, an
end-of-block for an AC) and decoding goes on at the next bit; a run that overruns the
block ends it; a 0xFF followed by anything but 0x00 or an RSTn is a marker: the rest
of the scan decodes as zero coefficients, flat grey; a scan that ends early leaves
the remaining blocks grey; an RSTn marker resynchronises, the decoder resets its DC
predictors and continues with the interval the marker names, skipping or repeating
intervals as libjpeg's `jpeg_resync_to_restart` does. The decoded blocks are
dequantised, inverse-transformed and merged into the frame through the present
decoder path; the frame's size stays the camera's. The look: a solid data pin
destroys the scan from its first bytes, grey after a few blocks; the bend wants
marginal contacts, a flip or two per frame, so the picture holds to the fault, then
shifts, tints and runs grey, and heals at the next restart marker if the camera
writes them; a marginal address pin repeats strips of the picture shifted sideways.
In movie mode every frame is a Motion JPEG frame through the same coder at the
mode's quality, so the faults flicker from frame to frame. Camera `[memory]` gains
`jpeg_base` and `[jpeg]` gains `restart_interval`.

### Stage 20a: the entropy coder and decoder (medium, one agent)

`codec/huffman.rs`: the Annex K.3 tables, the encoder from quantised integer blocks
to the scan's bytes, the decoder with the error rules above; `codec/mod.rs` split so
that quantisation yields integer coefficients, `encode` keeps its present exact
path, and `encode_scan` and `decode_scan` expose the scan as `Vec<u8>` in a reused
buffer; tests (the round trip is bit-exact on every subsampling and quality; the scan
of a flat frame is short and of a noisy one long; one flipped bit shifts every
macroblock after it and leaves those before it; a marker emulated by a flip greys the
rest; with a restart interval the damage ends at the next marker; a stream cut short
greys the rest), a bench of the coder and the decoder. Owns `codec/`, a new
`tests/huffman.rs`, the bench file's codec entries.

### Stage 20b: the buffer in the SDRAM (medium, one agent, parallel with 20a)

`memory::bend_scan(params, state, index, scan: &mut Vec<u8>)`: the scan written and
read through the same faults, contacts and retention masks as the raw at
`jpeg_base`, with the span widened to cover it, the write and read instants as
specified, and the power-on content as bytes; `[memory] jpeg_base` in the camera;
tests (a solid `a11` low on a still leaves the raw byte-identical and folds the scan;
a data pin flips one bit of every other byte; a marginal contact leaves a stale
frame's bytes; the span covers the scan; the state keeps the scan's words across
frames and `set_preset`). Owns `memory.rs`, `tests/memory.rs`, the camera model's
`[memory]` and `cameras/a520.toml`.

### Stage 20c: wiring, presets, panels, docs (medium, one agent plus the orchestrator)

`Simulator::frame` running `encode_scan`, `bend_scan`, `decode_scan` in place of
`encode` while the preset has `[bend.memory]`; the Memory panel's note on the
JPEG buffer and a readout of the scan's size; presets with goldens `a520-card-rot`
(a marginal data pin at a few hertz: shifted macroblocks, tints, grey tails) and
`a520-strip-repeat` (a marginal `a11`: repeated strips on a still, the raw
untouched); `docs/ARCHITECTURE.md`, `CHANGELOG.md`, README, the user guide, the
milestone 4 tuning note; the CLI's `inspect` printing the scan's size.

### Status (2026-11-07)

Stages 16, 17 and 18 are complete and merged on `rust-rewrite`: the sensor bend
(temperature, substrate bias; dark-current non-uniformity proportional to the dark
charge beside the fixed DSNU offset; the drain gone two volts below nominal; the glow
∝ k − 1), the reset bend (a 0.6 V source-follower swing, PROVISIONAL; runs keep their
length in time), their panel blocks and guide sections, the presets `a520-heat-gun`
(120 °C at the daylight exposure: a 0.5 s exposure blows the sample out),
`a520-substrate` (10 V at a fixed 1/13 s: the deepened well spills only from clipped
sites), `a520-reset-drag` and `a520-hot-reclock`, with goldens, benches,
`docs/research/milestone4-tuning-notes.md` and the docs. Found and fixed on the way:
the movie readout's optical black integrated a whole frame period where the image
integrated the shutter time, which crushed hot movie frames green. Eleven milestone 3
goldens were regenerated for a rounding-level change from the dark spread. Open for
the fix round: a weak reset needs about 20 to 25 % on daylight scenes, blooming needs
clipped highlights, the glow needs a dark long exposure, and movie frames of the
heat-gun presets are now worth re-checking. Stages 19 and 20 wait for the user's
stage 15 findings.

### Status (2026-11-07, later)

Stage 15 is closed: the user tested the app across both milestones and found
nothing to fix; `v0.3.0` is tagged. Stage 19 is specified above (section 3.11) and
started; stage 20 follows it; after stage 20, a preset gallery in the app in the
spirit of the legacy version's and an audit of what the legacy presets' looks still
need (the user's direction, 2026-11-07).
