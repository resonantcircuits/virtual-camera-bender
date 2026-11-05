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
   reclock, SDRAM address lines, CDS skew, AWB/AE hunting as servo behaviour.
4. More cameras (Sony DSC-V1, Fuji S9000, Praktica DC42) as camera files.
5. Post stage per the policy; preset library rebuilt from the old presets' spirit.
6. Capture input, virtual camera output, MIDI/OSC, audio-reactive injection.
7. WASM/WebGPU demo build of `vcb_core`.
