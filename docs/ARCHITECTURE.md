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
power, capture and memory stages resolve to nothing without their preset section and
then pass their signal through untouched. The module docs hold the formulas; this
section states the contracts.

| Stage | Function | In → out | Parameters | State |
|---|---|---|---|---|
| Scene | `scene::prepare` | `Scene` → `Scene` (sensor size) | camera `[isp]` `[lens]` `[sensor] nir_response`; preset `[scene]` `[bend.optics]` | – (working planes only) |
| Sensor | `sensor::expose` | `Scene` → `Charge` (electrons) | camera `[sensor]` `[still]` `[movie]` `[isp]`; preset `[sensor]` `[isp]` `[bend.sensor]` | – |
| Readout | `readout::read` | `Charge` → `Stream` (readout order) | camera clocking; preset `[bend.clocks]` | running faults |
| Power | `power::track` | the bend's current of the last frame → `Rail` (volts per line) | camera `[power]`; preset `[bend.rail]` | decoupling voltage, ripple phase, running spike |
| AFE | `afe::process` | `Stream`, `Rail` → `Analog` (volts) | camera `[sensor]` `[afe]` `[adc]` `[power]`; preset `[bend.afe]` `[bend.reset]` | node, flicker, phases, lockout, floating diffusion |
| ADC | `adc::convert` | `Analog` → `Codes` (words) | camera `[adc]` | – |
| Bus | `bus::bend` | `Codes` in place, reading `Rail` | preset `[bend.bus]`; camera `[adc]` `[logic]` `[power]` | capacitor and line voltages, diodes, gates, current per line |
| Capture | `capture::reframe` | `Codes` in place, reading `Rail` | preset `[bend.clock]`; camera `[still]` `[movie]` `[adc]` `[power]` | drift phases |
| ISP | `isp::clamp_black` | `Codes`, `Rail` → `Raw` | camera `[isp]` `[adc]` `[power]` | – |
| Memory | `memory::bend` | `Raw` in place | camera `[memory]`; preset `[bend.memory]` | SDRAM content, contacts |
| ISP | `isp::develop`, `isp::meter` | `Raw` → `Frame` | camera `[isp]` `[adc]` `[movie]`; preset `[isp]` | exposure and white-balance servos |
| Codec | `codec::encode` | `Frame` in place | camera `[jpeg]` `[still]` `[movie]` | – |

### Scene

The input is turned onto the landscape sensor if held in portrait, centre-cropped to
the sensor's aspect and resampled (area average down, linear up). Highlight
reconstruction inverts an assumed shoulder of the source camera, so clipped white
reaches `headroom` times reference white, with channel ratios kept and clipped channels
estimated from nearby unclipped highlights; it is a documented guess about lost data.
A cos⁴ falloff of `vignette_stops` dims the corners; the inverse colour matrix and
daylight gains move the scene into sensor primaries, clipping negative responses.

With `[bend.optics]` the IR-cut filter has slid off the top `ir_cut_pulled_frac` of
the sensor (the DSC-V1's magnet bypass), and the photosites there collect near
infrared through all three dyes. An RGB photo records none, so the scene's NIR is
estimated, a documented guess like highlight reconstruction: a weighted sum of the
linear scene (`nir_weights`, red-heavy, so a clear sky stays dark), raised on green
foliage by `foliage_nir` (the Wood effect, an assumption about the scene and off by
default) and scaled by `nir_scale`. The NIR image is defocused over `[lens]
nir_defocus_frac` of the width, since the lens focuses the visible, gated by the
filter's out-of-focus shadow (`filter_edge_blur_frac` of the height wide) and added
to each channel through `[sensor] nir_response`. Nothing downstream knows about it:
scene auto-exposure meters the sum and takes the extra light back, daylight white
balance turns it pink and grey world fights the cast, so the full-spectrum look
comes out of the chain. Real NIR data, a fourth scene plane, would replace the
estimate at the same point. The step has neither memory nor randomness.

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

With `[bend.sensor]` the CCD runs at `temperature_c` on a substrate bias of
`substrate_v`, by default the camera's `reference_temp_c` and `substrate_v`, where
every factor is exactly 1 and the charge is the unbent camera's. Heat raises the dark
current by the Arrhenius rule, doubling every `dark_doubling_c`; its spread over the
photosites (`dark_nonuniformity` of each site's dark charge) and the hot photosites
rise with it, while `dsnu_e` stays the readout chain's fixed offset. The read noise,
which the AFE adds, rises by `read_noise_per_c` per degree. The output amplifier glows
from the corner the readout samples first, `amp_glow_e_per_s` times the dark current's
rise above the reference, falling off over `amp_glow_radius_frac` of the width. The
optical-black photosites share the temperature, so the processor's clamp removes the
mean dark and the heat shows as grain, hot crosses after the demosaic, the glow and
clipping. The bias moves the overflow drain's barrier: above nominal the well shrinks
by `full_well_frac_per_v` per volt while the drain takes the excess, so highlights clip
flat; below it the well deepens and the drain's capacity falls to nothing
`drain_loss_v` below nominal, so highlights bloom along their columns, and more than a
volt below nominal the movie shutter, which pulses the substrate, clears less and the
smear grows. Reference white's charge stays on the nominal well, so metering is
unchanged. The section keeps no state.

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

### Power

One 3.3 V rail feeds the AFE, the bus drivers, the bend's 74HC chips and the
processor's inputs: an EMF behind the regulator's and battery's `source_ohm` with
`decoupling_uf` across it, holding `rail_v` at the camera's own `quiescent_ma`, so only
added current sags it. `[bend.rail]` adds a steady `load_ma`, load spikes of `spike_ma`
lasting `spike_ms` that arrive as a Poisson process at `spike_hz` (one transient load,
so an arrival during a spike extends it), ripple of `ripple_mv` at `ripple_hz` on the
EMF, and optionally a weaker `source_ohm`; the bus bend's own current, measured on
each line of the previous frame, loads the same line of this one. `power::track`
advances the RC in closed form between events and stores the voltage at the start
and end of every readout line, which the consumers take as linear within the line;
ripple faster than about half the line rate is therefore sampled, not resolved.
Without the section the rail holds `rail_v` to the bit and every consumer takes its
ideal path.

The consumers are thresholds and ratios, not dice. The AFE's reference and pedestal
follow the rail, so codes above black inflate by `rail_v / v` as it sags; below
`afe_uvlo_v` the AFE locks out and the bus holds its last word, optical black
included; when the rail returns, the AFE's black clamp restores the pedestal with a
time constant of `afe_restart_lines` native lines. The bus drivers, the gates' thresholds and the processor's latch
threshold scale with the rail, and below `logic_min_v` the 74HC chips float and
forget their state. A line that starts below `dsp_brownout_v` browns the processor
out for the rest of the frame (ISP). A reclocked oscillator follows the rail through
`ppm_per_v` (Capture).

### AFE

An AD9949-class front end, run in `f64` at `oversample` sub-steps per pixel period.
Correlated double sampling gives `conv_gain_uv_per_e`; a late reset sample (`cds_lag_px`,
`cds_mix`) echoes edges. Read noise is white plus 1/f. An injected oscillator
(`[bend.afe] inject`: sine, square, saw, sample-and-hold) adds at the input, the PGA
applies `pga_gain_db`, optionally modulated by `gain_mod`, and one pole of
`bandwidth_hz` with a slew limit drives the ADC. Blanking advances in closed form, so
oscillators roll on across lines and frames. The supply is the rail of the power
stage, read per sample; a line at the nominal rail takes the ideal path, which gives
the ideal supply's bytes. Bandwidth and slew rate are provisional.

With `[bend.reset]` the reset gate, which empties the CCD's floating diffusion before
each packet, misses runs of pulses (runs starting at `drop_hz` per second of readout,
lasting `hold_px` pixel periods on average) or clears only `reset_frac` of the
diffusion's charge. That charge is state, carried across pixels, blanking, lines and
frames. The CDS step reads the difference of the diffusion's level after the reset and
after the packet, each clipped at the output stage's swing `[afe] fd_swing_v`, so a
missed or weak reset cancels until the charge left behind uses up the swing; then
packets are lost and read black. A bright run therefore ends in black; under a weak
reset a run brighter than `reset_frac` of the swing per packet dims, and from
`reset_frac / (1 - reset_frac)` of it reads black, brighter light reading darker.
Optical black passes through the same diffusion, and the read noise is added after it.
With no drops and full resets the step is skipped.

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
bit whose settled level lies below the threshold. The drivers, the gates and the
latch threshold run from the rail; every level scales with it, and the network,
linear in its sources, keeps its tables by carrying its state over the rail ratio.
The bend records the mean current its high sources draw on each line, which loads
the rail on that line of the next frame.

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

### Capture

With `[bend.clock]` the timing generator and the AFE run from an oscillator at
`osc_mhz` (the LTC1799 mod) while the processor keeps the camera's crystal, generates
VD and latches the bus once per period of its own clock. Everything upstream has run
on the timing generator's clock, so `capture::reframe` replaces each word by the one
the bus carried at the processor's latch instant: the timing generator's position is
the integral of the frequency ratio since VD, which restarts it every frame; where it
is in blanking, or past its last line, the bus carries the black code, and latches
after the next VD are lost, so a slow generator never delivers its last lines. The
frequency wanders by up to `drift_ppm` (two incommensurate sines at `drift_hz`, their
phases seeded and running on across frames) and follows the rail by `ppm_per_v`. At
each of its line starts the processor resets its line counter to a timing-generator
line start that lies within `hd_lock_px` periods; this is a counter reset, not a loop
filter, so a phase that slips faster than the window is caught only as it wraps past.
Nothing corrects the Bayer phase: a word latched an odd number of periods or lines
away develops in another channel, as colour-phase static. A free-running detune
shears the picture by `(ratio - 1)` of a line per line and tears it with blanking
where it wraps; a steady oscillator draws the same tears in every frame, and the
drift moves them. At match-input the capture runs on the camera's native line and
clock and is sampled onto the virtual grid, so a preview shows the native look
downscaled. A steady oscillator at the nominal frequency leaves every word in place.

### ISP

`clamp_black` reads `dsp_bits` lines from `dsp_lsb` (on the A520 `D11` to `D2`, a
10-bit raw) and follows each line's optical-black mean with the DSP's clamp loop, in
readout order from the first line's measurement:

> `b[n] = b[n-1] + (1 - e^(-1/4)) (ob[n] - b[n-1])`

so a bend that disturbs the optical-black columns shifts black for a few lines. The
loop corrects only within `[isp] clamp_range` codes of the nominal black, so a line
whose optical black holds a picture word, as from a locked-out AFE, keeps its colour.
The processor's capture runs from the rail: a line that starts below `[power]
dsp_brownout_v` browns it out, and it captures nothing more of the frame, which leaves
that line and every later one as zero rows (`Raw::dropped_from`); the next frame is
captured afresh.

After the frame buffer (Memory, below), `develop` normalises by `white_code` (derived from the analog chain), white-balances the
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

### Memory

Between `clamp_black` and `develop`, with `[bend.memory]`, the clamped raw passes
through the processor's frame-buffer SDRAM: written as the lines arrive, in readout
order, one sample per word at `raw_base + y * cols + x`, and read back a frame period
later. The controller splits the linear address row-bank-column (`column_bits`,
`bank_bits`, `row_bits`; a modelling choice, DIGIC's is undocumented), and the SDRAM
takes row and column on the same pins, so pin `a_p` carries column bit `p` in one
phase and a row bit in the other, and one faulted pin corrupts two bits. A pin stuck
`low` or `high`, or two pins `bridge`d to their OR, fold the frame onto itself in
exact powers of two: where sites share a word the later write wins, and an odd row
offset moves the copy onto the other Bayer row phase. A marginal contact (`on_frac`,
`switch_hz`) is a random telegraph process in camera time, sampled exactly at each
line's write and again at its read; where it changed in between, the read finds what
the write did not put there: an earlier frame, another row, or the SDRAM's power-on
content (random codes hashed from the unit seed). Rows the processor did not capture
after a brownout are not written, so they read what the SDRAM held. Without the
section the raw passes untouched and a brownout leaves black rows.

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

The camera has two clocks, which are one until a preset reclocks it. The processor's
crystal clocks its latch on the bus, `dsp_pixel_period_s`, and generates VD, so the
frame period `frame_period_s` is always the processor's. The timing generator's clock
drives the CCD's transfer, the AFE, the ADC and the bus drivers: `pixel_period_s`,
which is `dsp_pixel_period_s / clock_ratio`, and `line_period_s`; `clock_ratio` is 1,
or with `[bend.clock]` the oscillator's `osc_mhz` over the readout mode's nominal
pixel clock. VD restarts the timing generator, which reads until the next VD and
loses what it has not read by then. On the sensor's clock run the readout and its
fault rates, the AFE's sub-steps and blanking, the ADC, the bus bend's circuit, the
supply rail's samples per readout line and the frame buffer's writes; on the
processor's run the capture's latches, the black clamp and brownout, the frame
buffer's read a frame period after the write, the servos and the movie shutter's
longest exposure; the smear's readout time, on the sensor's clock, stops at VD.
Oscillators (the AFE's injection and gain modulation, the flicker corner, the rail's
ripple and spikes, the reclock's drift, the marginal memory contacts) run in real
time within the readout and advance over the processor's frame period, VD to VD,
from one frame to the next. A new oscillator therefore keeps the frame period and the
geometry of a running simulator. The table in `Timing`'s documentation assigns every
consumer its clock.

At `native` resolution the sensor size, the processor's pixel clock, optical black and
blanking are the camera's. At `match-input` the sensor takes the oriented input size rounded down to
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

The milestone 3 bends follow the same rule. The rail's source impedance, decoupling,
spikes and ripple, the oscillator's detune, drift and supply sensitivity, and the
memory contacts' switching are physical and used as given, so a dip or a wander
covers the same fraction of the frame at every resolution. The NIR's defocus and the
filter's shadow are fractions of the frame. The capture runs on the native line and
clock and samples the result onto the virtual grid, since a match-input line is
mostly blanking and would tear differently at every size; `hd_lock_px` counts native
periods. The memory faults' address weights scale with the virtual pitch and rows and
round to powers of two, so a fold keeps its fraction of the frame, exactly where the
size ratios are powers of two. Of the milestone 4 bends, the reset gate's `drop_hz` is
a rate in camera time and its `hold_px` counts periods of the camera's own pixel clock
at every resolution, so a missed run covers the same share of a line in a match-input
preview as at native; the amplifier glow's radius is a fraction of the width.

## 5. Determinism and randomness

All randomness lives in `noise`. Each stage seeds itself with
`stage_seed(preset.seed, salt("<stage>"))`, so stages never share a stream. Spatial
values are pure hashes of `(x, y, salt, seed)`. Temporal noise hashes with the seed
mixed with the frame index (shot noise, ADC dither, clock faults) or draws from a
`Pcg32` seeded with stage seed and frame index, in readout order (read and latch
noise). The fixed pattern (non-uniformity, hot photosites, the spread of the bus
drivers) uses the camera's `unit_seed`, so `--seed` leaves it alone. Parallel passes
reduce per-row results in a fixed order, so thread count never changes a byte.

The milestone 3 stages follow the pattern. `power` draws the rail's spike arrivals
from a `Pcg32` seeded with its stage seed and the frame index; `capture` takes the
starting phases of its oscillator's drift from its stage seed; `memory` runs each
marginal contact from a `Pcg32` seeded with its stage seed and the pin, the frame
index as the stream, and hashes the SDRAM's power-on content from the camera's
`unit_seed` (salt `memory.power-on`), since a cell's power-up state belongs to the
chip. The optics step has no randomness. Their state: the rail keeps the decoupling
capacitor's voltage, the ripple's phase and what is left of a running spike; the AFE
whether it is locked out, the held word and the pedestal it is restoring; the bus
bend the current it drew on each line, for the next frame's rail; the capture the
drift's two phases; the frame buffer every SDRAM word the frames have reached since
power-on and each contact's state. The scene keeps only the NIR's working planes.

The milestone 4 bends add one stream and one piece of state. The sensor bend draws no
randomness of its own: the dark current's spread hashes the unit seed (salt
`sensor.dark.spread`), like the hot photosites, so its grain belongs to the unit and
stays put from frame to frame, and the glow joins the dark charge before its shot noise
is drawn. The reset bend draws its missed-pulse runs from a `Pcg32` seeded with its own
stage seed (salt `reset`) and the frame index, in readout order. The AFE's state gains
the floating diffusion's charge and whether a run was in progress at the end of the
frame, which the next frame continues afresh from its own stream, exact since the
geometric run length has no memory; with a sound gate the diffusion holds the frame's
last packet.

The contract: the same sequence of scenes and indices, from a fresh simulator or after
`Simulator::reset`, with the same camera, preset and seed, is identical byte for byte.
An index rendered twice without a reset need not repeat: the state has moved on.

`Simulator::set_preset` and `set_scene_size` reconfigure a running simulator, as a knob
turned on the bent camera. If the readout geometry stays (sensor size, rows read,
columns, optical black, blanking, the processor's clock, mode, orientation, output size
and oversampling; the timing generator's clock may move), every stage's parameters are
resolved anew and all buffers and state are kept: the AFE's node, flicker level and
oscillator phases (a new waveform continues the old phase), running clock faults, the
servos, and the bus bend's capacitor voltages, diodes and gates while its wiring
stays. The wiring is the set of source pins, the target, the function, the pot node,
whether the pot is at zero, the common bus and whether patch resistors are fitted;
new component values keep the charge. The rail's state, the drift's phases and the
SDRAM's content and contacts carry on whatever `[bend.optics]`, `[bend.rail]`,
`[bend.clock]` and `[bend.memory]` become, a new oscillator frequency included, and
new memory faults read the old content; the floating diffusion's charge carries on
whatever `[bend.reset]` becomes, and `[bend.sensor]`, which has no state, takes effect
with the next frame. A new wiring, or adding or removing the bus bend, returns that
bend to power-on; removing `[bend.rail]` returns the rail to the
nominal voltage and removing `[bend.memory]` forgets the SDRAM; a servo that the
preset turns off forgets its value; a new geometry rebuilds everything as `build`
does. Values that leave the geometry alone, such as the exposure time, take
effect with the next frame. Both calls return a `Reconfigured`: `Kept`, with flags
for a reset bus bend and a forgotten servo, or `Rebuilt`, so a caller such as the
app knows whether the next frame continues the sequence or starts it afresh. A
rejected preset changes nothing.

## 6. Configuration

Camera files (`cameras/<name>.toml`) hold `[camera]`, `[sensor]`, `[lens]`, `[still]`,
`[movie]`, `[afe]`, `[adc]`, `[logic]`, `[power]`, `[isp]`, `[memory]` and `[jpeg]`,
reject unknown keys and are range-checked by `Camera::validate`. `[sensor]
nominal_exposure_s` calibrates the sensitivity; `[still]` and `[movie]` each give their
`exposure_s`, the movie's at most `1 / fps`. `[sensor] nir_response` is each dye's
charge from unit near infrared relative to the green channel's from reference white;
`[lens] nir_defocus_frac` is the blur of the NIR image as a fraction of the width and
`filter_edge_blur_frac` the width of the IR-cut filter's shadow as a fraction of the
height. `[logic]` describes the bend's 74HC chips, including input protection
(`clamp_diode_v`, `clamp_r_ohm`, `input_leak_mohm`). `[power]` describes the 3.3 V
rail (`rail_v`, `source_ohm`, `decoupling_uf`, `quiescent_ma`) and its failure
thresholds (`afe_uvlo_v`, `logic_min_v`, `dsp_brownout_v`, each below `rail_v`) with
the AFE's `afe_restart_lines`. `[isp] clamp_range` is the black clamp's correction
range either side of black, in processor codes. For the sensor bend, `[sensor]` gives
the temperature its dark current and read noise are stated at (`reference_temp_c`),
the dark current's doubling interval (`dark_doubling_c`) and relative spread over the
photosites (`dark_nonuniformity`), the read noise's rise per degree
(`read_noise_per_c`), the amplifier glow's dark current at its corner one doubling
above the reference and its falloff (`amp_glow_e_per_s`, `amp_glow_radius_frac`), the
nominal substrate bias (`substrate_v`), the well's change per volt above it
(`full_well_frac_per_v`) and how far below it the overflow drain's capacity is gone
(`drain_loss_v`); for the reset bend, `[afe] fd_swing_v` is the output stage's swing in
volts. `[memory]` gives the frame-buffer
SDRAM's `column_bits`, `row_bits` and `bank_bits` and the raw buffer's word address
`raw_base`. Each physical value carries a confidence letter from the key-constants
table of `docs/research/a520-technical.md`: **A** measured or stated for the A520,
**B** stated for the near-certain sibling part, **C** a documented substitute of the
same class, **D** derived; or **PROVISIONAL**, a placeholder or choice awaiting
validation.

A preset needs only `camera`. `[readout]` sets mode, resolution and orientation;
`[scene]` the highlight headroom; `[sensor]` the exposure time `exposure_s`, which
replaces the camera's in either mode (positive and finite; a sensor `iso` would join
it there); `[bend.bus]` source pins, target, function, `cap_uf`,
`pot_kohm`, `pot_node`, `series_ohm` and `common_bus`; `[bend.clocks]` CTE and fault
rates; `[bend.afe]` injection, gain modulation and CDS skew; `[isp]` overrides, which
change only what the processor assumes (the sensor's response stays the camera's), so
they show as colour errors. The milestone 3 bends: `[bend.optics]` the filter's
position `ir_cut_pulled_frac` and the NIR estimate (`nir_weights`, `foliage_nir`,
`nir_scale`); `[bend.rail]` `load_ma`, `spike_ma`, `spike_hz`, `spike_ms`, `ripple_mv`,
`ripple_hz` and a replacement `source_ohm`; `[bend.clock]` the oscillator's `osc_mhz`
(required), `drift_ppm`, `drift_hz`, `ppm_per_v` and the processor's `hd_lock_px`;
`[bend.memory]` a list of `[[bend.memory.faults]]`, each a `pin` with a `kind` (`low`,
`high`, or `bridge` `with` another pin) and, for a marginal contact, `on_frac` and
`switch_hz`, a pin carrying one fault at most. Whether a pin exists on the camera's
SDRAM is checked when the simulator resolves the preset. `[bend.clock]` and
`[bend.clocks]` are different sections: the second is the CCD's transfer clocks. The
milestone 4 bends: `[bend.sensor]` the CCD's `temperature_c` (−40 to 150 °C) and
`substrate_v`, each optional and defaulting to the camera's, so an empty section is
the unbent camera; `[bend.reset]` the reset gate's `drop_hz`, `hold_px` (at least 1,
default 8) and `reset_frac` (0 to 1, default 1), inert at no drops and full resets.
The heat and the exposure trade against each other, the dark charge being the dark
current times the exposure time: at the camera's exposure a daylight frame shows the
heat from about 110 °C, and the glow wants a dark scene and a long exposure.

To add a camera, write a camera file with a confidence letter and citation per value
and name it in a preset; the RGGB mosaic and the selector banks are still code. To add
a bend, model it in the stage whose circuit it changes: a `[bend.<name>]` struct with
unit-suffixed fields and validation, resolved into that stage's `Params`, cross-frame
memory in its `State`, randomness from the stage seed with a new salt, sub-line
constants scaled as in section 4, tests of its physics and of its replay from reset.
A bend in hardware that no stage models becomes a new stage in signal order, as the
supply rail, the processor's capture and the frame buffer did; a stage that other
stages feel publishes a buffer they read, as the rail does.

## 7. Testing and validation

Tests of the public API live in `crates/*/tests/`, one file per area (`noise`,
`timing`, `config`, `stages`, `clean_camera`, `readout_afe`, `bus`, `codec`,
`simulator`, `interactive`, `servo`, `optics`, `power`, `capture`, `memory`,
`milestone3_plumbing`, `sensor_bend`, `reset`, `golden`, `io`, `video`, `cli`, and the
app's panels, render worker, video, export and viewport); the `test-util` feature exposes
internal state to them. They assert physical properties and invariants, never restated
constants: a 1 kHz square injection crosses zero twice per cycle, equal drivers meet
at half rail, the divider skips pixel-rate edges, flat fields stay flat, the rail of an
unloaded camera is the nominal to the bit, an oscillator at the nominal frequency
captures byte-identically to none, a solid stuck pin aliases exactly in powers of two,
the NIR of a pure blue sky stays below a twentieth of a white's, the dark current
doubles every `dark_doubling_c`, a missed reset over a bright run goes black once the
swing is used up while a dark run survives, every bend replays identically from
power-on. There is no random-input framework. Video tests generate clips with ffmpeg
and skip without it.

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

The milestone 3 stages were timed on the project's development VM instead (an AMD
Ryzen 7 5825U guest with 4 virtual cores under KVM, Ubuntu 26.04, Rust 1.99, shared
with other work, so whole-frame medians scattered by up to a fifth between runs),
against a clean frame of 430 ms there. `power::track` on the brownout preset's rail
takes 0.09 ms, two hundredths of a percent of a frame, one closed-form step per line.
`capture::reframe` on the reclock roll takes 4.6 ms, about 1 % of a frame. `memory::bend`
with the S9000 tear's two contacts takes 6.0 ms, under 1.5 % of a frame; a fault on a
higher pin widens the store towards the whole SDRAM and the copy with it. The optics
step raises `scene::prepare` from 42.5 to 81.5 ms, 9 % of a frame, mostly the
defocus of the NIR image. On the brownout's sagging rail `afe::process` takes 104 ms
against 86 ms on the nominal one, 4 % of a frame more, since a line off the nominal
rail leaves the AFE's ideal path. Without their sections the three new stages return
in nanoseconds.

The milestone 4 bends were timed on the same VM, against a clean frame of 450 ms.
`sensor::expose` on the heat gun's 120 °C CCD takes 124.8 ms against 116.3 ms, 2 % of a
frame more, for the glow's falloff at every photosite and the scene auto-exposure the
preset meters with; on the substrate preset's drainless, overexposed CCD it takes
120.7 ms, the spill walking the columns. `afe::process` with the reset drag's weak,
flaky gate takes 100.0 ms against 87.5 ms, 3 % of a frame more, for the floating
diffusion carried through every sample and the missed runs drawn in readout order.
Without their sections both stages take their unbent paths.

## 9. Extension points and known limitations

Extension points are the `[bend.*]` sections, camera files, new stages in
`Simulator::frame`, the `Rail` for any further consumer of the supply, and the scene's
NIR estimate, which a fourth scene plane of measured near infrared would replace at
the same point with nothing downstream changing.

Highlight reconstruction guesses at lost data. Shot noise is Gaussian, smear one
streak per column, the CTE trail geometric. The AFE has one pole. Scene auto-exposure
meters the prepared scene, near infrared included, without lag; the servos meter the
bent raw with a lag, on the plain mean luminance and grey world, without DIGIC's
weighting. Auto-exposure stands in for the aperture: there is no separate aperture or
ISO control, and the movie shutter's exposure does not follow the light. Bus lines
carry capacitance but no inductance, so they never ring past their settled level. Bend
gates see no crossings during blanking, and gate timing resolves to one sub-step.
The colour matrix, tone curve, noise reduction, sharpening, mosaic phase, AFE
bandwidth and many sensor values (the nominal exposure among them) are stand-ins or
provisional; defect concealment is absent. The movie line skip and the AFE part are
inferred. The A520 bend layout is hard-coded. No post stage or GPU path exists: the
desktop app renders on the CPU and shows each frame as a texture. The bus bends are
validated by eye against one composite, the milestone 3 and 4 bends by eye alone.

The milestone 3 bends add their own. The NIR estimate guesses at what an RGB photo
never recorded: the weights are a mild assumption, `foliage_nir` an explicit one (that
green means chlorophyll), and the deep red the hot mirror also trims is left out. The
AFE's reference follows the rail ratiometrically, a PROVISIONAL model of a reference
without headroom where an AD9949 rejects supply variation inside its range. The rail is
one supply: the CCD's clock and substrate supplies and the processor's core do not
sag with it, the 74HC chips keep their timing and the drivers their strength at a low
rail, the bend's current loads it a frame late, and ripple faster than about half the
line rate is sampled per line, not resolved. The processor drives VD and restarts the
timing generator every frame, so a steady detune draws the same tears in every frame
and nothing rolls vertically across frames; a generator that free-runs across VD is a
later option. The bus bend latches at the timing generator's phase, not at the
processor's latch instant. The frame buffer holds one sample per word: DIGIC's packed
10-bit raw layout and its address mapping are unknown and not modelled, nor are the
SDRAM's burst counter, `a10`'s auto-precharge, refresh, or the interleaving of the next
frame's writes with this frame's reads, and the A520's SDRAM part is unidentified. Its
row pins above `a8` lie beyond the movie raw and act on stills only.

The milestone 4 bends add theirs. The amplifier glow's geometry, an exponential falloff
from the first-read corner over `amp_glow_radius_frac` of the width, is a guess, and its
strength, the read noise's rise per degree and the dark current's spread are
PROVISIONAL; the sensor's temperature is uniform and steady, without a gradient from
where the heat gun points or a drift through a video. The substrate bias moves the well
and the drain linearly, and its effect on the movie shutter is a linear growth of the
smear, all PROVISIONAL. A deepened well holds more than the ADC's full scale, so every
highlight that blooms is clipped already, and a daylight scene without clipped
highlights blooms only when overexposed. The output stage's swing `fd_swing_v` is
PROVISIONAL, the source follower's saturation signal plus a margin. The floating
diffusion does not spill: its charge is unbounded, and an over-full diffusion does not
leak over the reset gate's barrier, which matters only for how fast a weak reset
drains it after a run of missed pulses. In movie mode the optical-black photosites
integrate dark current over a whole frame and the image's over the shutter's
exposure; at the reference temperature the two differ by hundredths of an electron,
but on a hot sensor with a movie exposure shorter than the frame the black clamp
removes too much and the shadows crush green.
