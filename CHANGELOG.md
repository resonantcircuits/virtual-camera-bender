# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Milestone 4: the rest of the bender's reach

The bends a bender makes on the sensor itself and on its reset gate, on the
frame-buffer SDRAM's data pins and its heat, on the AFE's serial programming link and
on the body's seal against light, each modelled in the stage whose hardware it
changes, with presets tuned by eye.

#### Added

- The sensor bend, `[bend.sensor]`, in the sensor stage: the CCD's `temperature_c`
  and its `substrate_v`. Heat raises the dark current by the Arrhenius rule, doubling
  every `dark_doubling_c`, with its spread over the photosites and the hot photosites;
  raises the read noise; and lights the output amplifier's glow at the corner the
  readout samples first. The optical-black photosites share the temperature, so the
  black clamp removes the mean dark and the heat shows as grain, hot crosses and the
  glow. A raised substrate bias shrinks the well, so highlights clip; a lowered one
  deepens it and takes the overflow drain's capacity away, so highlights bloom along
  their columns and the movie shutter's smear grows.
- The reset bend, `[bend.reset]`, in the AFE's correlated double sampling: missed
  runs of reset pulses (`drop_hz`, `hold_px`) and weak pulses that clear only
  `reset_frac` of the floating diffusion's charge. The diffusion carries its charge
  across pixels, lines and frames; the CDS cancels what is left until it uses up the
  output stage's swing, after which packets read black, so bright runs end in black
  and a weak reset solarises a bright sky.
- The SDRAM's data pins and heat, in the memory stage's `[bend.memory]`:
  `[[bend.memory.data_faults]]` on the data lines `d0` upward, stuck `low` or `high`,
  `bridge`d, or marginal like the address contacts. A word holds the processor's
  integer sample with its black level, so a stuck line acts on one bit of every
  sample after the black clamp: stuck high it lifts black by the bit's weight and
  posterises, stuck low it folds the highlights down; a marginal contact leaves its
  bit in the word until the next write, so a stale word read back clean still shows
  it. The A520's pins `d10` to `d15` carry no sample bit and change nothing. The
  SDRAM's `temperature_c` shortens its cells' retention, log-normal around a median
  that halves every `retention_halving_c`; a cell that cannot outlast the refresh
  period reads its leakage state, true cells falling to 0 and anti cells rising to 1
  by the SDRAM row, the same cells in every frame: a hot board shows single bright and
  dark sites from about 105 °C and loses a fifth of its samples at 120 °C.
- The serial bend, `[bend.serial]`, in the AFE: a noisy contact on the SDATA line over
  which the processor rewrites the AFE's registers in every vertical blanking (the
  control word, the VGA gain, the four `PxGA` pixel gains, the clamp level), each bit
  flipping with probability `bit_error`. A flipped data bit sets a wrong gain, tint or
  pedestal for the frame, or tristates the outputs; a flipped address bit lands the
  write on another register or loses it, and the intended register keeps the last
  frame's content until a clean write. The registers are AFE state, carried across
  frames; a clean write restores the camera's exact values.
- The light leak, `[bend.leak]`, in the sensor stage: light entering behind the lens
  from one `edge`, `radiance` times reference white at the opening and falling by
  `1/e` over `spread_frac` of the frame, with its `colour` and its near infrared
  `nir`, which reaches the dyes through `[sensor] nir_response` since the IR-cut filter
  is bypassed. Its charge joins the scene's before the well, so it clips, blooms and
  smears like scene light, and in still mode it smears alone, the mechanical shutter
  blocking the scene but not the leak. Scene auto-exposure meters it and stops the
  scene down for it within its two stops.
- Camera values in `cameras/a520.toml`: `[sensor]` `reference_temp_c` 25 °C,
  `dark_doubling_c` 6, `read_noise_per_c` 0.003, `amp_glow_e_per_s` 1.0,
  `amp_glow_radius_frac` 0.15, `substrate_v` 12 V, `full_well_frac_per_v` 0.25,
  `drain_loss_v` 2 V and `dark_nonuniformity` 0.3, and `[afe] fd_swing_v` 0.6 V,
  graded by confidence, most of them PROVISIONAL.
- Camera values for the stage 19 bends: `[memory] data_pins` 16, `reference_temp_c`
  25 °C, `retention_s` 128 s, `retention_halving_c` 10, `retention_spread` 0.5 and
  `refresh_ms` 64, and `[afe] serial_data_bits` 24, `vga_bits` 10, `vga_min_db` 6,
  `vga_db_per_code` 0.0351, `pxga_bits` 6, `pxga_max_db` 18, `clamp_bits` 8 and
  `pxga_gain_db` [0, 0, 0, 0], with their confidence letters; the camera's gains and
  pedestal must fit their registers.
- In the app, a "Sensor bend" block on the Sensor panel (temperature in °C and
  substrate bias in V, dimmed at the camera's values, with readouts of the dark
  current's multiple and the full well) and a "Reset gate" block on the AFE panel
  (missed pulses per second, run length in pixels, reset strength in per cent, with
  the swing and the packet size a weak reset shows on); the strip lights Sensor and
  AFE for them; every knob keeps the circuit running. Guide sections for both.
- In the app, a data-pin grid (d0 to d15, the pins without a sample bit dimmed) and a
  retention temperature on the Memory panel, with readouts of the median retention
  and the share of cells and samples that leak; a "Serial link" block on the AFE
  panel (bit error per thousand bits, with the bits a frame, the corrupted writes a
  frame and the share of frames that take one); a "Light leak" block on the Sensor
  panel (edge, radiance, spread, colour and near infrared, with the glow's reach in
  photosites and what is left at the far edge). The strip lights the stages they
  bend; every knob keeps the circuit running. Guide sections for all three.
- Four presets: `a520-heat-gun` (mottled shadows and hot crosses in daylight, the
  readout corner's glow on a dark, long exposure), `a520-substrate` (highlights that
  bloom along their columns), `a520-reset-drag` (bright runs ending in black streaks,
  a solarised sky) and `a520-hot-reclock` (the loose-sync oscillator on a hot, grainy
  sensor).
- Five presets for the stage 19 bends: `a520-stuck-bit` (data line d6 stuck high:
  posterised, black lifted), `a520-hot-board` (the sensor and the SDRAM under one heat
  gun at 120 °C: grain, hot crosses and a fifth of the samples' bits lost),
  `a520-serial-noise` (one bit in a hundred of the AFE's serial link flipped: a frame
  in five flashes a gain, a tint, a green maze or a dropped output; seed 17's still
  shows a green maze), `a520-light-leak` (a daylight leak glowing pink from one edge
  over a scene auto-exposure stops down) and `a520-open-body` (the full-spectrum
  optics with daylight through red plastic under grey-world balance: a peach glow
  under a teal sky).
- Tests of both bends' physics, replay and reconfiguration (`sensor_bend`, `reset`),
  golden frames of the four presets, and benchmarks of the sensor with a hot and a
  drainless CCD and of the AFE with a weak, flaky reset gate.
- Tests of the stage 19 bends' physics, replay and reconfiguration (`memory`, `serial`,
  `leak`, `stage19_plumbing`), golden frames of their five presets, and benchmarks of
  the frame buffer with stuck and marginal data pins and a hot SDRAM, of the AFE with
  bit errors on its serial link at 0.003 and 0.03, and of the sensor with a light leak.
- `docs/research/milestone4-tuning-notes.md`, how the presets were chosen and what
  the sample could not show, with a stage 19 section on the five new presets and the
  SDRAM's retention spread.

#### Changed

- The dark current gains its own spread over the photosites, `dark_nonuniformity`,
  beside `dsnu_e`, which stays the readout chain's fixed offset pattern; at the
  reference temperature it moves eleven goldens by at most 4 codes. The simulator
  applies the sensor's read-noise rise to the AFE's read noise.
- `vcb presets` names the sensor, reset, serial and leak bends.
- `docs/ARCHITECTURE.md` describes the five bends, the reset and serial streams, the
  diffusion's charge, the AFE's registers and the SDRAM's cell masks, the new
  configuration, their cost and their limits; the README lists the presets.
- The existing golden frames are unchanged by the new presets: fourteen by stage 18's
  four, eighteen by stage 19's five.

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
