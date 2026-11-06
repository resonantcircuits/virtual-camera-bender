# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Milestone 3: the bends from the old presets' spirit

The legacy JavaScript presets' looks return as physical models in the stage whose
hardware each bend changes; their normalised knobs become bench quantities and their
coin flips circuits with memory.

#### Added

- The IR-cut filter removal, `[bend.optics]`, in the scene stage: the hot mirror slides
  off the top `ir_cut_pulled_frac` of the sensor behind an out-of-focus shadow, and the
  scene's near infrared, estimated from its linear colours (`nir_weights`, raised on
  foliage by `foliage_nir`, off by default, and scaled by `nir_scale`), defocused and
  added to every dye. Scene auto-exposure and white balance react unchanged, so the
  full-spectrum look comes out of the chain.
- The supply rail, a new `power` stage and `[bend.rail]`: one 3.3 V rail behind the
  source impedance and decoupling, loaded by a steady current, Poisson load spikes and
  the bus bend's own current, with ripple on the source, computed per readout line in
  closed form and carried across frames. The AFE's reference and pedestal follow it,
  and below its lockout the AFE holds its last word and then restores its pedestal;
  the bus drivers, 74HC gates and latch threshold scale with it, and the gates float
  below their minimum supply; the processor browns out for the rest of the frame.
- The master-clock reclock, a new `capture` stage and `[bend.clock]`: the timing
  generator and AFE run from an oscillator at `osc_mhz`, wandering by `drift_ppm` at
  `drift_hz` and following the rail by `ppm_per_v`, while the processor keeps its
  crystal, generates VD and latches the bus on its own clock, with optional HD
  re-synchronisation within `hd_lock_px`. `Timing` carries both clocks:
  `pixel_period_s` and `clock_ratio` for the sensor side, `dsp_pixel_period_s` and
  `frame_period_s` for the processor.
- The SDRAM address-line bend, a new `memory` stage and `[bend.memory]`: the clamped raw
  is written to the frame buffer in readout order and read back a frame period later
  through physical address pins, each carrying a column and a row bit, stuck `low` or
  `high`, `bridge`d, or marginal (`on_frac`, `switch_hz`); the SDRAM keeps its content
  across frames and reconfigurations.
- The connections between the bends: the rail drives the reclocked oscillator; a
  processor brownout stores no more rows, so the frame buffer shows what it held there;
  the processor's black clamp corrects only within `[isp] clamp_range`, so a
  locked-out AFE's held word keeps its colour.
- Camera values in `cameras/a520.toml`: `[power]` (`rail_v` 3.3 V, `source_ohm` 0.5,
  `decoupling_uf` 47, `quiescent_ma` 60, `afe_uvlo_v` 2.7 V, `logic_min_v` 2.0 V,
  `dsp_brownout_v` 2.4 V, `afe_restart_lines` 3), `[memory]` (`column_bits` 9,
  `row_bits` 12, `bank_bits` 2, `raw_base` 0, a 128 Mbit x16 SDRAM),
  `[sensor] nir_response` [1.0, 0.55, 0.5], `[lens] nir_defocus_frac` 0.003 and
  `filter_edge_blur_frac` 0.06, and `[isp] clamp_range` 128, graded by confidence,
  most of them PROVISIONAL.
- Ten presets: `a520-full-spectrum` (the washed full-spectrum photograph),
  `a520-chlorophyll` (aerochrome-ish, foliage glowing), `a520-full-spectrum-hpf` (the
  HPF bend on a full-spectrum camera), `a520-brownout` (breathing bright bands),
  `a520-sieve` (clustered rows dying into solid colour), `a520-dying-battery` (lockout
  stripes, then the frame buffer's earlier content below the brownout),
  `a520-loose-sync` (a held picture that shears and snaps back), `a520-reclock-roll`
  (travelling tears and colour-phase bands), `a520-s9000-tear` (torn bands and stale
  frames) and `a520-mirror-bank` (one solid row pin, a crystalline repeated band).
- Tests of the four bends' physics, replay and reconfiguration (`optics`, `power`,
  `capture`, `memory`, `milestone3_plumbing`), golden frames of the ten presets, and
  benchmarks of `power::track`, `capture::reframe` and `memory::bend` with a bent case
  for each new stage, the scene with the filter removed and the AFE on a sagging rail.
- `docs/research/milestone3-tuning-notes.md`, how the presets and camera values were
  chosen.

#### Changed

- The AFE runs from the rail instead of an ideal supply; on a nominal rail it gives the
  same bytes.
- Sub-line analog constants scale with the processor's period, so a reclocked sensor
  clock leaves them fixed in seconds and a slower oscillator settles more per pixel;
  the AFE's oscillators advance VD to VD over the processor's frame period, and the
  smear's readout time stops at VD.
- `vcb presets` names the new bends and `vcb inspect` the timing generator's clock
  beside the native one; `Simulator::set_preset` keeps the rail, the oscillator's
  drift and the SDRAM's content through any change of the new sections.
- `docs/ARCHITECTURE.md` describes the power, capture and memory stages and the optics
  step, the two clocks, the new seeds and state, and the new configuration; the README
  lists the presets.
- The four existing golden frames are unchanged.

### Milestone 2: the desktop app

#### Added

- `vcb_app`, the desktop app (`vcb-app`): viewport with fit, zoom, pan and
  hold-to-compare; a render worker with latest-wins coalescing, reduced previews
  while dragging and full native renders on release; a readout panel; the bus bend
  as a front panel after the schematic; camera discovery, preset load and save,
  image and first-video-frame input, image export.
- `Simulator::set_preset` and `Simulator::set_scene_size`, which reconfigure a running
  simulator and keep buffers and analog state where the readout geometry and the bus
  wiring allow, with the accessors `preset`, `camera`, `scene_size` and
  `exposure_scale`. Both return a `Reconfigured`: `Kept`, flagging a bus bend returned
  to power-on and a servo that forgot its value, or `Rebuilt` after a new geometry.
- Physical exposure: `[sensor] nominal_exposure_s` (1/125 s on the A520,
  PROVISIONAL) is the exposure at which reference white fills `white_fraction` of the
  well, and the collected charge scales with `exposure_s / nominal_exposure_s`, so a
  longer exposure brightens and clips, raises the dark charge and lowers the smear
  relative to the signal. Auto-exposure scales on top of it and compensates a changed
  time within its two stops; `sensor::Params::white_e` gives reference white's charge.
- `[movie] exposure_s`, the electronic shutter's integration time, at most `1 / fps`
  (1/125 s on the A520, PROVISIONAL).
- The preset's `[sensor]` section with an optional `exposure_s`, which replaces the
  camera's exposure in either readout mode (limited to one frame period in movie mode)
  and takes effect through `set_preset` without a rebuild.
- Bus line capacitance and driver slew, `[adc] line_pf` and `driver_slew_v_per_us`
  (15 pF and 600 V/µs on the A520, PROVISIONAL): the image processor latches each
  line's actual voltage, which can be mid-transition.
- Lagging auto-exposure and auto-white-balance servos, `ae_mode = "servo"` and
  `[isp] wb = "servo"` with `ae_tau_s` and `wb_tau_s`, metered on the bent raw of
  the previous frame; the preset's `[isp]` can set `ae_mode`, `ae_tau_s` and
  `wb_tau_s`.
- Tests of reconfiguration (`interactive`) and of the servos (`servo`), and the line
  capacitance experiment in `docs/research/a520-tuning-log.md`.

#### Changed

- Native is the default sensor resolution; match-input stays as the explicit option
  for large exports.
- The bus circuit's sub-step is the exact response of its network to the sources held
  over it, and a pixel period in which the gate and the protection diodes hold is
  evaluated in closed form.
- The HPF and INV golden frames, which the line capacitance and slew move in a few
  percent of their pixels.
- The movie readout integrates for `[movie] exposure_s` instead of a whole frame
  period: the same brightness, about eight times the smear relative to the signal
  at 15 fps and an eighth of the dark charge. The still goldens are unchanged.
- A servo that the preset turns off forgets its value at once, so the next frame is
  exposed and balanced as the new setting says, not by the stopped servo.
- `docs/ARCHITECTURE.md` describes `vcb_app` among the crates, physical exposure and
  what reconfiguration reports.

### Milestone 1: the validated core

The project is rewritten in Rust as a physical simulation of a circuit-bent CCD
compact camera. The JavaScript editor it replaces is tagged `v0.1.0-js` and kept on
the `legacy-js` branch; its presets and stylised effect modules do not carry over.

#### Added

- `vcb_core`, a sans-I/O, deterministic simulation library: scene preparation with
  highlight reconstruction and vignetting, an interline CCD sensor (shot noise,
  fixed-pattern noise, hot photosites, anti-blooming, smear), still and line-skipped
  movie readout with charge-transfer trails and clock faults, an oversampled analog
  front end (CDS, 1/f read noise, injected oscillators, gain modulation, bandwidth and
  slew), a 12-bit ADC, the A520 data-bus bend as a nodal circuit simulation (DIP
  source bank, coupling capacitor, pot on node F or S, 74HC00 inverter, 74HC74
  divider, input clamp diodes, target selector, DSP latch), the image processor
  (black-clamp loop, white balance, Malvar demosaic, colour matrix, tone curve, noise
  reduction, sharpening) and a JPEG model with the measured Canon tables.
- A time and resolution model that keeps the look independent of the input size.
- `vcb_io`: image decode and encode with EXIF orientation, and video through
  `ffmpeg` pipes.
- `vcb_cli`: the `vcb` command with `render`, `render-video`, `sheet`, `inspect` and
  `presets`.
- `cameras/a520.toml`, the Canon PowerShot A520 in physical units, its values graded
  by confidence and traced to their sources, and the presets `a520-clean`, `a520-hpf`,
  `a520-div` and `a520-inv`.
- `scripts/validate-a520.sh`, which builds the comparison sheet against the reference
  photos of a real bent A520.
- Integration tests of physical behaviour and determinism, golden-image hashes of the
  four presets, and divan benchmarks of every stage.
- `docs/ARCHITECTURE.md`, `docs/PLAN.md`, `docs/RUST-GUIDELINES.md` and
  `docs/POST-STAGE.md`.
