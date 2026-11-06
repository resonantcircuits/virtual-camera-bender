# Milestone 3 tuning notes (stages 12 and 14)

Tuning of the ten milestone 3 presets and the PROVISIONAL camera values they rest on.
No reference photos exist for these bends, so every judgement is by eye: on
`samples/sample-01.jpg` at native still resolution, and in movie mode on a 3 s pan
(`validation/clip-pan.mp4`, 1280 x 960 at 15 fps, gitignored like the references).
The sample is a portrait photo of a lake under a clear sky with thin clouds, dry grass
and a few green mullein plants around a dead tree. The camera holds it in portrait, so
the sensor's readout lines run vertically in the upright frame, the first line on the
left; in the landscape clip they run horizontally. Stage 12's camera values were set by
the orchestrator from the bend agents' contact sheets; in stage 14 only presets
changed, and no Rust code. Round sheets lived in the session's scratch space and are
not committed.

The looks to reach are the stage 14 table in `docs/PLAN.md`, from the legacy presets'
descriptions (`docs/legacy/PROJECT_SPEC.md`, phases 7 and 8). The legacy modules were
the brief for the look, never for the method: where the physics will not give a look,
that is recorded below rather than forced.

## Optics: `a520-full-spectrum`, `a520-chlorophyll`, `a520-full-spectrum-hpf`

**Camera values (stage 12a).** `[sensor] nir_response = [1.0, 0.55, 0.5]` (C,
PROVISIONAL): the Bayer dyes turn transparent past 700 nm, red from about 650 nm, green
and blue only past about 800 nm, where silicon has lost half its response, so red
collects most; a first trial with green and blue at 0.75 and 0.7 is in the stage 12a
sheets. `[lens] filter_edge_blur_frac = 0.06` (was 0.02 in the first trial), derived as
the filter's gap over the f-number, an assumed 1 mm at f/4 against the 4.26 mm sensor
height. `nir_defocus_frac = 0.003`, 6.8 pixels at the native width: the soft glow of
full-spectrum photos, by eye. `foliage_nir` defaults to 0 (user decision): reading
green as chlorophyll is an assumption about the scene, which a preset makes explicitly.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| O1 | Stage 12a's set: filter removed with daylight and grey-world balance, `foliage_nir` 0 and 4; foliage 8 with red gain 1.2; filter half pulled | Daylight balance turns everything pink: violet sky, salmon grass. Grey world gives the washed look: cream clouds, slate sky, grey-teal water. Foliage 4 turns the mullein pale pink. Half pulled: a soft vertical boundary, the uncovered left pink | grey world is the full-spectrum look |
| O2 | Grey world with `foliage_nir` 0, 2, 3 and 4, `nir_scale` 1 to 1.5, bluer weights `[0.5, 0.42, 0.08]` | Foliage 2 pales the plants a little; 4 with `nir_scale` 1.5 washes the whole frame further and the plants go pink-white. The bluer weights barely change anything | full-spectrum: foliage 3, `nir_scale` 1.3 |
| O3 | Foliage 8 and 10 with fixed gains: red 1.2, 1.0, 0.9 against blue 1.5 to 1.8, `nir_scale` 1 and 1.3 | Red 1.2: violet sky. Red 1.0: blue sky, teal water. Red 0.9 with blue 1.7: deep blue sky and water, foliage pink-white. Blue 1.8 with red 1.2: purple | chlorophyll: red 0.9, blue 1.7, foliage 8, `nir_scale` 1.3 |
| O4 | Full-spectrum optics under the HPF bus bend, grey world or daylight | Grey world, metered on the bent raw, turns the HPF's red sky green with cream and red bands and a pink tree; daylight keeps magenta and yellow bands | full-spectrum-hpf: grey world, as full-spectrum |

Scene auto-exposure (`ae_mode = "scene"`) is on in all three: the camera's default
fixed exposure would let the added NIR overexpose the frame, and a full-spectrum
shooter meters through the open sensor. The sample has little foliage, so the
chlorophyll preset sets `foliage_nir` at twice a typical leaf for the few plants to
glow; on a green landscape it will glow much more. The optics has neither time nor
randomness, so movie mode shows the still's look at the movie's size.

## Power: `a520-brownout`, `a520-sieve`, `a520-dying-battery`

**Camera values (stages 12b and 12e).** `[power] dsp_brownout_v` 2.9 V became 2.4 V:
at 2.9 V, a supervisor's trip point, the processor dropped the frame before the rail
could reach the AFE's 2.7 V lockout, so the lockout was never seen. `[isp] clamp_range
= 128` codes: with an unlimited clamp the processor subtracted a locked-out AFE's held
word along with its optical black and every stripe developed black; 64 would also
have capped the DIV bend's optical-black disturbance (101 codes on the golden scene)
and changed its golden. `source_ohm` 0.5, `decoupling_uf` 47, `quiescent_ma` 60 and
`afe_restart_lines` 3 stay PROVISIONAL; `afe_uvlo_v` 2.7 V and `logic_min_v` 2.0 V are
C, from the AD9949 class and the 74HC family.

Two bounds shape these presets. The AFE's reference follows the rail, so a sag inflates
codes above black by `rail_v / v`; the lockout at 2.7 V caps that at 3.3 / 2.7, 22 %,
about a third of a stop. And a locked-out line holds one word on every site, optical
black included, so it develops in the ratio of the white-balance gains: a pink of the
held word's brightness, or black where the clamp's range still covers the word.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| B1 | Stage 12b's brownout (200 mA on 1.5 Ω, 280 mV at 12 Hz) and two deeper variants (300 mA at 8 Hz, 250 mA at 20 Hz) | 12b's: faint bands. The deeper ones dip under 2.7 V: pink and black lockout bars | drop anything that locks out |
| B2 | 100 mA, 420 mV at 12 Hz (2.73 V to 3.57 V); 150 mA, 350 mV at 9 Hz | Soft bright bands, clearest in the sky, nothing fails. The top of the swing stays under the AD9949's 3.6 V maximum | brownout: the first |
| S1 | Stage 12b's sieve without the bus bend (230 mA spikes holding 2.61 V, 14 Hz, 5 ms); 30 Hz with 2 ms; 20 Hz with 3 ms | Wide pink bars and a black one spread over the frame, or many thin pink lines spread evenly: dead rows, but not clustered | clustering needs a cause |
| S2 | A spike plateau just over the lockout, lowered under it by the ripple's troughs: 180 mA at 60 Hz for 1 ms with 120 mV at 5 Hz; 185 to 200 mA at 25 to 40 Hz for 2.5 to 4 ms with 100 to 130 mV at 4 to 6 Hz; seeds 1 to 3 | The spikes lock the AFE out only in the troughs, so the dead rows bunch, one cluster per trough, pink or black; elsewhere the spikes only bloom thin bright lines. 1 ms spikes give hairlines | sieve: 185 mA, 40 Hz, 2.5 ms, 130 mV at 4 Hz |
| D1 | Stage 12e's battery (1000 mA on 0.5 Ω, 450 mV at 4 Hz, with `[bend.memory]`), with 300 mA spikes; 12e's movie battery (5 Ω, 150 mA spikes) | One lockout bar, then the brownout's power-on noise; spikes add a black bar; the movie battery shows no stripes in a still | stripes need spikes on a falling rail |
| D2 | 1.5 Ω, 300 mA (2.85 V), 400 mV at 4 Hz, with 133 to 140 mA spikes (0.2 V) at 40 to 120 Hz for 0.6 to 1.5 ms | As the ripple falls past 3.0 V the spikes lock out (stripes), past 2.7 V the rail stays locked out (a bar), and the first spike near the trough takes it under 2.4 V (brownout). 80 Hz for 1 ms gives three stripes before the bar | dying battery: 133 mA, 80 Hz, 1 ms |
| D3 | The dying battery in movie mode on the pan, with and without `[bend.memory]` | Without it each brownout blacks out the frame to its bottom; with it the rows show the frame before, which in a slow pan reads as the picture going on. Lockout bars come about every fourth frame | keep `[bend.memory]` |

The brownout's bands stay soft: the third of a stop the lockout allows is all a
ratiometric reference can give, and only highlights near the clip turn it into bright
bands. In a still from power-on the dying battery's brownout reads the SDRAM's power-up
content, pastel noise; the previous frame shows only in a video or in the app's running
camera.

## Clock: `a520-loose-sync`, `a520-reclock-roll`

**Camera values (stages 12c and 12e).** None: the reclock uses the camera's nominal
pixel clock and the processor's own timing. Two findings from stage 12 bound the
presets: large detunes are violent, and every reclock feels the rail through
`ppm_per_v` (default 1000, the LTC1799 class) but only when `[bend.rail]` is present;
neither clock preset has a rail, so the default is inert there.

The geometry decides everything. A still line is 3564 periods (2320 stored, 1244 of
blanking) and a frame 1704 lines, so a free-running detune of `ratio - 1` slips
`3564 (ratio - 1)` periods a line and wraps a whole line, a diagonal tear through the
blanking, every `1 / (ratio - 1)` lines. The HD window is a counter reset, not a loop:
a slip per line inside the window holds every line, and a larger one is caught only as
the phase wraps past or as a drift brings it back. A movie line is 4036 periods but a
movie frame only 446 lines, so the same oscillator shears a movie frame a quarter as
far as a still.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| C1 | Steady 0.05 % (26.9865 MHz) in a 1-pixel window; stage 12c's 26.98 MHz with 900 ppm at 3 Hz in 4 pixels; 26.9 to 26.97 MHz with drift and 1 to 3-pixel windows | 0.05 % slips 1.8 periods a line, 3000 over the frame, nearly a whole line: the picture shears by most of its height behind a black wedge of blanking, and the wrap that would snap it lies 2000 lines away. 26.9 to 26.97 MHz cut the frame into diagonal bands of blanking | a steady 0.05 % is past reading on a still; work near zero detune with drift |
| C2 | 26.998 MHz free-running; 27.0 MHz with 200 to 1000 ppm at 4 to 6 Hz in windows of 0.3 to 2 pixels; seeds 1 to 3 | Readable: the picture waves with the drift, pink colour-phase static wherever the slip leaves an odd phase, a wedge of blanking at the line ends. A window above the peak slip holds all of it | the window sets hold against let go |
| C3 | 26.999 to 27.0 MHz, 300 to 600 ppm at 3 to 10 Hz, windows 0.6 to 2 pixels | 400 ppm at 4 Hz in 1 pixel holds most of the frame, with a magenta lobe where the slip within a line crosses half a period. 600 ppm at 10 Hz in 1.5 pixels holds, lets go into a V of shear, and snaps back to a held picture | loose sync: 600 ppm, 10 Hz, 1.5 pixels at 26.9995 MHz |
| C4 | 900 to 2500 ppm at 1 to 8 Hz, windows 0.5 to 3 pixels, detunes 0 to 0.07 % | 1500 ppm and up, or a real detune, leaves a still mostly blanking. 900 to 1200 ppm at 4 to 5 Hz gives waves of shear, wedges of blanking and colour-phase bands, still readable | reclock roll: 1200 ppm, 4 Hz, 0.5 pixel at 26.9995 MHz |
| C5 | Both in movie mode on the pan | Loose sync: mostly held frames, now and then one sheared with horizontal colour-phase stripes and a wedge. Reclock roll: most frames sheared, the tears and wedges travelling from frame to frame, a held frame between | keep both |

The table asks loose sync for "a small detune"; its 18.5 ppm (26.9995 MHz) is a token.
A steady detune alone cannot give a held picture with a few snaps on a still: a slip
inside the window holds every line and shows nothing, and one outside it shears until
a wrap thousands of lines away. The snaps come from the drift carrying the slip in and
out of the window. Every sheared stretch carries colour-phase static, since nothing
corrects the Bayer phase; that is the physics of the bend and not removable by a value.

## Memory: `a520-s9000-tear`, `a520-mirror-bank`

**Camera values (stages 11 and 12d).** `[memory]` `column_bits` 9, `row_bits` 12,
`bank_bits` 2, `raw_base` 0 (PROVISIONAL): the JEDEC organisation of a 128 Mbit x16
SDR SDRAM, four banks of 4096 rows of 512 columns, 2^23 words, which holds the 2272 x
1704 raw at one sample per word; the A520's part is unidentified and the
row-bank-column mapping is a modelling choice. The bendable pins are `a0` to `a11`.

Pin `a_p` carries column bit `p` (for `p` up to 8) and row bit `11 + p`, so `a9` to
`a11` are pure row pins. Bit 20, `a9`'s, is 2^20 words, 461.5 still lines; the movie
raw is 2272 x 426 = 967,872 words, wholly below it, so `a9` and up leave a movie frame
alone, and a pin of `a8` or lower also folds the line at 2^p columns.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| M1 | Stage 12d's set: `a10` high, `a2` low, `a4` low; S9000 with `a8` low and `a5` high, both 30 % at 300 and 250 Hz | `a10`: a magenta copy over the first 45 % of the frame. The S9000 shreds the still into thin vertical bands with power-on noise | slow the contacts |
| M2 | Solid row pins `a7` to `a10` high, `a8` and `a10` low, `a9` bridged to `a10`, `a8` to `a9` | `a10`: one big band. `a9`: two offset copies, a crystalline block of the frame repeated half a line across, magenta where the odd line offset swaps the Bayer row phase. `a8` and lower add a 256-column fold: many tiles | mirror bank: `a9` high |
| M3 | Movie mode, frame 0 | `a9` and `a10` change nothing; `a8` folds both ways with a ragged column phase; `a6` and `a7` tile | mirror bank stays a still look; S9000 pins at `a8` and below |
| M4 | S9000 at 15 to 60 Hz: `a8` low with `a5` or `a6` high; `a9` low with `a7` high | Wide bands of power-on noise and folded copies between readable stretches; `a9` does nothing in movie | slower still |
| M5 | S9000 at 3 to 12 Hz: `a8` low and `a6` high at 8 and 5 Hz; `a7` high and `a5` low; `a8` and `a5` at 4 and 3 Hz; `a7` and `a4`. Then the pan in movie mode | Stills: fewer, wider bands. Movie: `a8`/`a6` at 8 and 5 Hz tears most frames into bands with folded copies and earlier frames; `a8`/`a5` at 4 and 3 Hz leaves many frames clean | S9000: `a8` low and `a6` high, 30 %, 8 and 5 Hz |

Why the contacts tear so readily: each line samples a contact at its write and again
at its read, a frame period later (0.23 s for a still). Above about one switch a second
the two samples are independent, so at `on_frac` 0.3 some 42 % of the lines,
`2 x 0.3 x 0.7`, read through a different fault than wrote them; `switch_hz` sets how
many bands that makes, through the mean holding times `on_frac / switch_hz` and
`(1 - on_frac) / switch_hz` (37.5 ms and 87.5 ms for `a8`). The mirror bank's `a9`
works on stills only; a pin at `a8` or lower would act in movie mode too, but brings
a column fold that turns the single band into tiles.

## What the presets could not reach

- The brownout's bands are soft, a third of a stop at most, because the lockout caps
  the ratiometric gain. Brighter bands would need the rail to reach more than the
  AFE's reference, for instance the CCD's output stage or the processor's input
  levels, which the model keeps on ideal supplies.
- The sieve's solid colours have one hue: a held word on every site develops in the
  white-balance gains' ratio, pink or black. The legacy sieve's varied rail colours
  came from a latch-up to a code pair; an AFE that latches up to a rail code instead
  of holding its last word would be a core change (`afe`).
- The dying battery shows "the previous frame" only in a video or a running camera; a
  still from power-on shows the SDRAM's power-up noise there.
- Loose sync needs drift for its snaps (above).
- The mirror bank is a still look with `a9`. A vertical roll across frames, from a
  timing generator that free-runs across VD, is not modelled (`capture`), so reclock
  roll travels by drift only.
