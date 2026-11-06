# Architecture

## 1. Purpose

Virtual Camera Bender simulates a circuit-bent early-2000s CCD compact camera, first
the Canon PowerShot A520 with the bend documented on its ADC data bus: a DIP bank of
data lines, a coupling capacitor, a potentiometer, a 74HC00 inverter or 74HC74 divider,
and a selector driving the result back onto one line. An input photo or video frame is
the light in front of the lens; it leaves as the JPEG the camera would have written.

"Physically based" means every stage models its hardware in bench quantities:
electrons, volts, bus words, ohms, microfarads, hertz. Glitches emerge from those
models; none is painted on. Three principles hold. The core is sans-I/O and
deterministic: `vcb_core` reads no files, clocks or entropy, and the same input,
preset, seed and frame index give the same bytes. Physical quantities carry their unit
in the field name (`pot_kohm`, `cap_uf`); ratios are named for what they are (`cte`).
Colour comes from the signal chain (white balance, colour matrix and clipping acting on
whatever the bends did to the raw data), never from a lookup table or palette.

## 2. Crates and boundaries

```
  vcb_cli   binary `vcb`: clap, anyhow, mimalloc, tracing-subscriber
  vcb_app   binary `vcb-app`: eframe (egui, wgpu), rfd, anyhow, mimalloc
    |   \
    |    v
    |   vcb_io    image, kamadak-exif, serde_json; runs ffmpeg and ffprobe
    |    |
    v    v
  vcb_core  rayon, tracing; serde behind the `serde` feature
```

`vcb_core` holds configuration, timing, the stages, the circuit solver, the image
processor and the codec model; it takes a linear-light `Scene`, returns an 8-bit sRGB
`Frame`, and emits only `tracing` events. `vcb_io` owns every file and process: JPEG
and PNG with EXIF orientation, raw frames piped through `ffmpeg`. `vcb_cli` loads
camera and preset files, builds a `Simulator` and connects readers to writers. Both
binaries depend on `vcb_core` and `vcb_io`, and neither on the other.

`vcb_app` is the desktop instrument: the camera's front panels in physical units
around a viewport. The UI thread never renders; a render worker thread owns two
running simulators, one for reduced previews while a control is dragged and one for
full renders on release, and drains its queue to the newest job, so the latest
change wins and stale replies are dropped. A new preset or scene size reaches a
running simulator through `Simulator::set_preset` and `set_scene_size`, so the
camera's analog memory carries from one render to the next.

## 3. The signal chain

`Simulator::frame` runs the stages in order on buffers allocated once. Each stage
module has a `Params` struct resolved once from camera, preset and `Timing`, a `State`
where the hardware remembers across frames, and one public function per frame. The
module docs hold the formulas; this section states the contracts.

| Stage | Function | In → out | Parameters | State |
|---|---|---|---|---|
| Scene | `scene::prepare` | `Scene` → `Scene` (sensor size) | camera `[isp]` `[lens]`; preset `[scene]` | – |
| Sensor | `sensor::expose` | `Scene` → `Charge` (electrons) | camera `[sensor]` `[still]` `[movie]` `[isp]`; preset `[sensor]` `[isp]` | – |
| Readout | `readout::read` | `Charge` → `Stream` (readout order) | camera clocking; preset `[bend.clocks]` | running faults |
| AFE | `afe::process` | `Stream` → `Analog` (volts) | camera `[sensor]` `[afe]` `[adc]`; preset `[bend.afe]` | node, flicker, phases |
| ADC | `adc::convert` | `Analog` → `Codes` (words) | camera `[adc]` | – |
| Bus | `bus::bend` | `Codes` in place | preset `[bend.bus]`; camera `[adc]` `[logic]` | capacitor and line voltages, diodes, gates |
| ISP | `isp::clamp_black`, `isp::develop`, `isp::meter` | `Codes` → `Raw` → `Frame` | camera `[isp]` `[adc]` `[movie]`; preset `[isp]` | exposure and white-balance servos |
| Codec | `codec::encode` | `Frame` in place | camera `[jpeg]` `[still]` `[movie]` | – |

### Scene

The input is turned onto the landscape sensor if held in portrait, centre-cropped to
the sensor's aspect and resampled (area average down, linear up). Highlight
reconstruction inverts an assumed shoulder of the source camera, so clipped white
reaches `headroom` times reference white, with channel ratios kept and clipped channels
estimated from nearby unclipped highlights; it is a documented guess about lost data.
A cos⁴ falloff of `vignette_stops` dims the corners; the inverse colour matrix and
daylight gains move the scene into sensor primaries, clipping negative responses.

### Sensor

An interline CCD with an RGGB mosaic. Exposure is physical: at `nominal_exposure_s`
reference white fills `white_fraction` of `full_well_e`, and the collected charge
scales with `exposure_s / nominal_exposure_s`, so a longer exposure brightens the
frame until the well, and then the ADC, clips. The exposure time is the preset's
`[sensor] exposure_s`, else the readout mode's camera value; the movie readout's
electronic shutter integrates at most one frame period. Dark charge is
`dark_e_per_s` times the exposure time. Non-uniformity and hot photosites hash the
camera's `unit_seed`: they belong to the unit, not the preset. Shot noise is Gaussian
with the charge's variance. An overflow drain absorbs `antibloom_ratio` full wells;
the rest spills along the column. Movie mode's electronic shutter adds smear from
`smear_db` (V/10 definition), one constant streak per column; it scales with the
light and the readout time, not the exposure, so relative to the signal it grows as
the exposure shortens. The readout time is at most the processor's frame period, since
the sensor reads until VD.

Auto-exposure scales the light on top of the set exposure. With `ae_mode = "scene"`
each frame is scaled, within two stops and without lag, so the mean luminance of the
scene integrated for the set exposure lands on `ae_target`; with `"servo"` the image
processor's exposure servo sets the scale from the previous frame (see ISP), and the
first frame after a reset is metered on the scene. Either compensates a changed
exposure time within its two stops, as the aperture would; with `"fixed"` the time
alone sets the brightness. The image processor normalises by the code of reference
white at the nominal exposure.

### Readout

Each line carries `ob_cols` optical-black samples, then the active ones; blanking is
not stored, its length rides in the `Layout`. Movie mode reads one row pair in every
`line_skip` pairs. Transfer inefficiency uses one reservoir per column:

> a packet that crosses `N` native transfers keeps `cte^N` and deposits the rest into
> its column's reservoir, which releases `r = 1 / (1 + N (1 - cte) / cte)` of its
> content into every later line.

The geometric tail matches the binomial trail to first order in `N (1 - cte)`.
`[bend.clocks]` adds vertical stalls, double pulses and horizontal glitch bands at rates
per second; an odd slip swaps the Bayer phase, which develops as a colour swap.

### AFE

An AD9949-class front end, run in `f64` at `oversample` sub-steps per pixel period.
Correlated double sampling gives `conv_gain_uv_per_e`; a late reset sample (`cds_lag_px`,
`cds_mix`) echoes edges. Read noise is white plus 1/f. An injected oscillator
(`[bend.afe] inject`: sine, square, saw, sample-and-hold) adds at the input, the PGA
applies `pga_gain_db`, optionally modulated by `gain_mod`, and one pole of
`bandwidth_hz` with a slew limit drives the ADC. Blanking advances in closed form, so
oscillators roll on across lines and frames. The supply is ideal (`rail_v` is the hook
for sag); bandwidth and slew rate are provisional.

### ADC

`0 V` to `full_scale_v` maps to codes `0` to `2^bits - 1`, rounded and clipped, after a
third of a code of dither keyed to image position. Bit `k` drives line `Dk`.

### Bus bend

Each line is an ideal 0 V or `v_oh` driver behind `driver_r_ohm` (log-normal spread
hashed from `unit_seed`, so it belongs to the unit) and `bus_series_ohm`. The source
pins (`D2` to `D11`) join node S, directly or through `series_ohm`; `cap_uf` couples S
to node F; `pot_kohm` loads F to ground (`pot_node = "f"`, the high-pass reading and
default) or S, as the schematic draws it (`"s"`). The gate inputs on F leak
`input_leak_mohm` to half the supply and clamp F through protection diodes beyond
`clamp_diode_v` via `clamp_r_ohm`; with the pot on S, these diodes restore F's level.
The function is a wire, a 74HC00 inverter or a 74HC74 toggle with thresholds,
hysteresis, delays, setup time and `ff_fmax_hz`; at 15 MHz the divider skips
pixel-rate edges, and those skips, not chance, make its runs irregular. Node T joins
one pin (`D4` to `D11`) or ground; `common_bus`, or a pin in both banks, merges S and
T. Every tapped line, and the bend's wires S and T, carry `line_pf` to ground, and a
driver that changes its bit slews at `driver_slew_v_per_us` (a full swing must fit in
the camera's own pixel period; a reclocked sensor clock too fast for it gets a swing of
one period), so a line is still moving for part of each pixel period. The
processor latches each touched line's actual voltage at `latch_phase` against
`dsp_threshold_v` plus `dsp_noise_mv` of noise; untouched lines keep the ADC's bit. At
27 MHz a 15 pF line through 250 Ω has settled to within a millivolt at a latch phase of
0.75. Its charge flows partly through the coupling capacitor, which shifts the
capacitor's voltage by about `line_pf / cap_uf` of each step on S. An RC line only
moves between settled levels, so neither the capacitance nor an early latch raises a
bit whose settled level lies below the threshold.

Every capacitance becomes its backward-Euler companion and each diode a switch,
leaving a piecewise-linear network with three states (no diode, ground or supply
diodes conducting). The state is the voltage across every capacitance: the coupling
capacitor, then each merged node that carries line capacitance. `Params::resolve`
inverts each state's conductance matrix once. Every state variable is the voltage
across one capacitance, so the backward-Euler step's matrix is similar to a symmetric
one; replacing each of its eigenvalues `1 / (1 + σ dt)` by `e^(-σ dt)` makes a sub-step
the exact response to sources held over it, which line time constants of a few
nanoseconds against a 9 ns sub-step need. Every voltage is tabulated as linear in the
driver pattern, the gate level and the previous state. A pixel period whose gate output
and diode state hold, the rule, is evaluated in closed form from its start state:
node F at every sub-step (for the gates and the diodes), the latched pins and the end
state are each one product with the start state. A pixel in which the gate switches or
a diode changes runs sub-step by sub-step. Blanking runs in closed form on the slowest
mode once the lines have settled, split where a gate transition falls due or a diode
turns off; gates see no input crossings there.

### ISP

`clamp_black` reads `dsp_bits` lines from `dsp_lsb` (on the A520 `D11` to `D2`, a
10-bit raw) and follows each line's optical-black mean with the DSP's clamp loop, in
readout order from the first line's measurement:

> `b[n] = b[n-1] + (1 - e^(-1/4)) (ob[n] - b[n-1])`

so a bend that disturbs the optical-black columns shifts black for a few lines. The
loop corrects only within `[isp] clamp_range` codes of the nominal black, so a line
whose optical black holds a picture word, as from a locked-out AFE, keeps its colour.
`develop` normalises by `white_code` (derived from the analog chain), white-balances the
mosaic, clipping where the least-amplified channel saturates, demosaics (Malvar or
bilinear), applies the colour matrix, bins movie frames, applies tone curve and sRGB
through one table, then noise reduction and an unsharp mask on `Y'CbCr`, radii as
fractions of the frame width. The matrix is a stand-in (dcraw's S60).

`meter` runs the servos after each frame on the bent raw, as DIGIC meters the data it
receives. With `ae_mode = "servo"` the next frame's exposure scale moves towards
`scale * ae_target / luminance`, the luminance being the raw's channel means through
this frame's white balance and colour matrix; with `wb = "servo"` the red and blue
gains move towards the frame's grey-world gains. Each approaches its goal in stops with
the time constant `ae_tau_s` or `wb_tau_s` of camera time: a frame corrects
`1 - e^(-frame_period_s / tau_s)` of the remaining error. After a reset the first frame
uses the scene-metered exposure and the daylight gains, so a still with servos equals
one with `ae_mode = "scene"` and daylight white balance. A servo that is turned off
forgets its value at once.

### Codec

Encode and decode in place: BT.601 `Y'CbCr`, 4:2:2 subsampling, 8 by 8 DCT,
`round(F / Q)` with the measured Canon tables at the mode's quality, inverse DCT and
chroma upsampling (repetition or libjpeg's triangle filter). Entropy coding is lossless
and not modelled.

### Post

None exists. A post module needs an entry in `docs/POST-STAGE.md` arguing why no stage
can produce its look; it takes only the camera's 8-bit frame and never makes colour.

## 4. Time and resolution

`Timing::derive` fixes the clocks. A line is `ob_cols + cols + hblank_px` pixel periods,
a frame `rows_read + vblank_lines` lines; `rows_read` keeps one row pair in every
`line_skip` pairs. Orientation `auto` holds the camera in portrait when the input is
taller than wide: the scene is swapped onto the landscape sensor, the output back.

At `native` resolution the sensor size, pixel clock, optical black and blanking are the
camera's. At `match-input` the sensor takes the oriented input size rounded down to
even, and the native active-line time and frame period stay fixed: the pixel period is
the native active-line time over `cols`. Optical black scales with the width (at least
eight columns, or the native count if smaller), vertical blanking with the rows;
horizontal blanking absorbs the rest of the native line, the pixel period trimmed by at
most half a period per line to hold whole periods. Where fewer than eight blanking
periods would remain, as for inputs with more rows than the native readout, blanking
stays at eight and the frame period grows. Still output is `cols` by `rows_read`; movie
output is `cols / h_bin` by `rows_read` at match-input, the camera's movie size at native.

Constants shorter than a line (AFE bandwidth and slew, gate delays, minimum pulse,
setup time, `1 / ff_fmax_hz`, the bus drivers' slew) describe the hardware in seconds at
the native clock and are multiplied by `Timing::resolution_scale`, `dsp_pixel_period_s /
native_pixel_period_s`, keeping their length in pixels at every resolution while a
reclocked timing generator leaves them fixed in seconds, so a slower oscillator settles
more per pixel; so is the bus line capacitance, which keeps the lines' time constants in
pixels; the latch phase is a fraction of the pixel period. Oscillators, the flicker
corner and fault rates are used as given, since line time and frame period are native;
from frame to frame they advance over the processor's frame period, VD to VD. Fault
durations are native lines scaled to the virtual rows, CTE counts native transfers, and
ISP radii are fractions of the width.

## 5. Determinism and randomness

All randomness lives in `noise`. Each stage seeds itself with
`stage_seed(preset.seed, salt("<stage>"))`, so stages never share a stream. Spatial
values are pure hashes of `(x, y, salt, seed)`. Temporal noise hashes with the seed
mixed with the frame index (shot noise, ADC dither, clock faults) or draws from a
`Pcg32` seeded with stage seed and frame index, in readout order (read and latch
noise). The fixed pattern (non-uniformity, hot photosites, the spread of the bus
drivers) uses the camera's `unit_seed`, so `--seed` leaves it alone. Parallel passes
reduce per-row results in a fixed order, so thread count never changes a byte.

The contract: the same sequence of scenes and indices, from a fresh simulator or after
`Simulator::reset`, with the same camera, preset and seed, is identical byte for byte.
An index rendered twice without a reset need not repeat: the state has moved on.

`Simulator::set_preset` and `set_scene_size` reconfigure a running simulator, as a knob
turned on the bent camera. If the readout geometry stays (sensor size, rows read,
columns, optical black, blanking, clocks, mode, orientation, output size and
oversampling), every stage's parameters are resolved anew and all buffers and state
are kept: the AFE's node, flicker level and oscillator phases (a new waveform continues
the old phase), running clock faults, the servos, and the bus bend's capacitor
voltages, diodes and gates while its wiring stays. The wiring is the set of source
pins, the target, the function, the pot node, whether the pot is at zero, the common
bus and whether patch resistors are fitted; new component values keep the charge. A
new wiring, or adding or removing the bend, returns the bend to power-on, and a servo
that the preset turns off forgets its value; a new geometry rebuilds everything as
`build` does. Values that leave the geometry alone, such as the exposure time, take
effect with the next frame. Both calls return a `Reconfigured`: `Kept`, with flags
for a reset bus bend and a forgotten servo, or `Rebuilt`, so a caller such as the
app knows whether the next frame continues the sequence or starts it afresh. A
rejected preset changes nothing.

## 6. Configuration

Camera files (`cameras/<name>.toml`) hold `[camera]`, `[sensor]`, `[lens]`, `[still]`,
`[movie]`, `[afe]`, `[adc]`, `[logic]`, `[isp]` and `[jpeg]`, reject unknown keys and
are range-checked by `Camera::validate`. `[sensor] nominal_exposure_s` calibrates the
sensitivity; `[still]` and `[movie]` each give their `exposure_s`, the movie's at most
`1 / fps`. `[logic]` describes the bend's 74HC chips,
including input protection (`clamp_diode_v`, `clamp_r_ohm`, `input_leak_mohm`). Each
physical value carries a confidence letter from the key-constants table of
`docs/research/a520-technical.md`: **A** measured or stated for the A520, **B** stated
for the near-certain sibling part, **C** a documented substitute of the same class,
**D** derived; or **PROVISIONAL**, a placeholder or choice awaiting validation.

A preset needs only `camera`. `[readout]` sets mode, resolution and orientation;
`[scene]` the highlight headroom; `[sensor]` the exposure time `exposure_s`, which
replaces the camera's in either mode (positive and finite; a sensor `iso` would join
it there); `[bend.bus]` source pins, target, function, `cap_uf`,
`pot_kohm`, `pot_node`, `series_ohm` and `common_bus`; `[bend.clocks]` CTE and fault
rates; `[bend.afe]` injection, gain modulation and CDS skew; `[isp]` overrides, which
change only what the processor assumes (the sensor's response stays the camera's), so
they show as colour errors.

To add a camera, write a camera file with a confidence letter and citation per value
and name it in a preset; the RGGB mosaic and the selector banks are still code. To add
a bend, model it in the stage whose circuit it changes: a `[bend.<name>]` struct with
unit-suffixed fields and validation, resolved into that stage's `Params`, cross-frame
memory in its `State`, randomness from the stage seed with a new salt, sub-line
constants scaled as in section 4, tests of its physics and of its replay from reset.
A bend elsewhere, such as the SDRAM address lines, becomes a new stage before the codec.

## 7. Testing and validation

Tests of the public API live in `crates/*/tests/`, one file per area (`noise`,
`timing`, `config`, `stages`, `clean_camera`, `readout_afe`, `bus`, `codec`,
`simulator`, `interactive`, `servo`, `golden`, `io`, `video`, `cli`, and the app's
panels, render worker, video, export and viewport); the `test-util` feature exposes
internal state to them. They assert physical properties and invariants, never restated
constants: a 1 kHz square injection crosses zero twice per cycle, equal drivers meet
at half rail, the divider skips pixel-rate edges, flat fields stay flat, every bend
replays identically from power-on. There is no random-input framework. Video tests
generate clips with ffmpeg and skip without it.

The golden tests guard against unintended change: a synthetic 160 by 120 scene (a
gradient, a bright disc, a dark rectangle) goes through each preset in still mode at
match-input resolution with seed 1, and the frame's FNV-1a hash must equal
`crates/vcb_core/tests/golden/<preset>.hash`, beside which lies the golden PNG.
`VCB_UPDATE_GOLDEN=1` rewrites hashes and PNGs after an intended change.

Validation is by eye. `scripts/validate-a520.sh` renders the reference sky through the
clean, HPF, DIV and INV presets above the matching quadrants of a composite shot with a
real bent A520, cropped from local, unlicensed images into the gitignored `validation/`.

## 8. Performance

`cargo bench -p vcb_core` (divan) times each stage and the whole frame on a synthetic
2272 by 1704 scene, native still readout. Median of 20 samples; Apple M2 (4
performance, 4 efficiency cores), macOS 15.7, Rust 1.96:

| Function (ms) | Clean | HPF |
|---|---:|---:|
| `scene::prepare` | 9.8 | 8.6 |
| `sensor::expose` | 38.9 | 38.9 |
| `readout::read` | 1.1 | 1.2 |
| `afe::process` | 59.7 | 59.7 |
| `adc::convert` | 5.3 | 5.3 |
| `bus::bend` | 0 (no bend) | 74.8 |
| `isp::clamp_black` | 0.7 | 0.7 |
| `isp::develop` | 46.4 | 49.2 |
| `codec::encode` | 10.6 | 10.8 |
| `Simulator::frame` (whole chain) | 188.2 | 278.2 |

Scene, sensor, ISP and codec run over rows on the rayon pool. The AFE and the bus bend
run sequentially in readout order, since their state carries from sample to sample.
The AFE steps through about 16 million sub-steps per frame; the bus bend, with three
capacitances on the HPF wiring (the coupling capacitor and the lines on S and T),
evaluates about 4 million pixel periods in closed form and steps through the rest.

## 9. Extension points and known limitations

Extension points are the `[bend.*]` sections, camera files, the `rail_v` hook for
supply sag, and new stages in `Simulator::frame`.

Highlight reconstruction guesses at lost data. Shot noise is Gaussian, smear one
streak per column, the CTE trail geometric. The AFE has one pole and an ideal supply.
Scene auto-exposure meters the clean scene without lag; the servos meter the bent raw
with a lag, on the plain mean luminance and grey world, without DIGIC's weighting.
Auto-exposure stands in for the aperture: there is no separate aperture or ISO
control, and the movie shutter's exposure does not follow the light. Bus lines carry
capacitance but no inductance, so they never ring past their settled level. Bend
gates see no crossings during blanking, and gate timing resolves to one sub-step.
The colour matrix, tone curve, noise reduction, sharpening, mosaic phase, AFE
bandwidth and many sensor values (the nominal exposure among them) are stand-ins or
provisional; defect concealment is absent. The movie line skip and the AFE part are
inferred. The A520 bend layout is hard-coded. No post stage or GPU path exists: the
desktop app renders on the CPU and shows each frame as a texture. Validation is by
eye against one composite.
