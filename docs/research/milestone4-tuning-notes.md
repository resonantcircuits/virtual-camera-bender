# Milestone 4 tuning notes (stages 16 to 19)

Tuning of the four presets of the sensor and reset bends (stages 16 to 18), the five of
the frame buffer's data pins and heat, the AFE's serial link and the light leak (stage
19, in its own section below), and the PROVISIONAL camera values they rest on. As in
milestone 3 (`milestone3-tuning-notes.md`), no reference photos exist for these bends,
so every judgement is by eye: on `samples/sample-01.jpg` at native still resolution,
and in movie mode on the 3 s pan (`validation/clip-pan.mp4`, gitignored). The sample is
the portrait lake under a clear sky with a dead tree; its readout lines run vertically
in the upright frame, the first on the left, so the CCD's columns run horizontally, and
the readout's first corner is the bottom left (in the landscape clip, the top left).
Stages 16 and 17 set the camera values from their contact sheets; stage 18 changed
presets and the command line's preset listing, and no `vcb_core` code. Round sheets
lived in the session's scratch space and are not committed.

The looks to reach are the stage 18 table: the heat gun's grain, hot crosses and glow
in the readout corner; the substrate's highlights blooming along their columns; the
reset drag's bright runs ending in black streaks and a solarised sky; and the loose
sync on a hot, grainy sensor. Where the sample will not give a look at the settings the
table proposed, the settings moved and the reason is recorded below.

## Sensor: `a520-heat-gun`, `a520-substrate`, `a520-hot-reclock`

**Camera values (stage 16).** `[sensor] reference_temp_c` 25 °C (C, room-temperature
datasheet conditions, those of `dark_e_per_s` and `read_noise_e`). `dark_doubling_c`
6 (C, CCD silicon doubles its dark current every 6 to 8 °C; PROVISIONAL within that
range): at 7 the hottest photosites did not show at 70 °C and 1/125 s. `amp_glow_e_per_s`
1.0 (PROVISIONAL): the plan had 20 at the reference temperature; the glow now scales with
the dark current's rise `k - 1`, since the dark current as characterised already
includes the amplifier's share, and 1.0 lets heat show hot crosses first and the corner
glow only at long exposures. `amp_glow_radius_frac` 0.15 (PROVISIONAL).
`read_noise_per_c` 0.003 (PROVISIONAL; the amplifier's thermal floor alone gives about
0.0017). `dark_nonuniformity` 0.3 (PROVISIONAL, tens of per cent for CCDs): the dark
current's own spread over the photosites, proportional to the dark charge, beside
`dsnu_e`, which stays the readout chain's fixed offset; it moved eleven goldens by at
most 4 codes. `substrate_v` 12 V (C, Sony interline parts set VSUB within about 9 to
15 V), `full_well_frac_per_v` 0.25 (PROVISIONAL, the well halves two volts above
nominal) and `drain_loss_v` 2 V (PROVISIONAL: the drain takes no excess two volts
below nominal). Stage 16's lens-cap sheet shows the intended order: at 70 °C nothing at
1/125 s, a few hot crosses and a trace of glow at half a second, grain and a magenta
glow in the corner at four seconds.

Two bounds shape the presets. The black clamp removes the mean dark, so heat shows only
through the spread and shot noise of the dark charge, the hot photosites and the glow,
and on a daylight frame, where a shadow holds a few thousand electrons, those need
hundreds of electrons of dark charge to read. The dark charge is
`dark_e_per_s * 2^((T - 25) / 6) * exposure_s`: 70 °C gives 181 e/s, 5.8 e at 1/30 s and
90 e at half a second. And scene auto-exposure takes back at most two stops. The sample
meters brighter than the target, so at 1/30 s scene auto-exposure sits at its limit and
the frame equals the clean camera's (a mean of 113.26 in both); any longer exposure
overexposes it. A half-second exposure, which the table proposed, is four stops past
what auto-exposure takes back.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| H1 | 70 °C with scene auto-exposure at 1/2, 1/8 and 1/30 s; 90 and 100 °C at 1/30 s | 1/2 s: a white frame, the hills barely there. 1/8 s: two stops over, pale sky, bright turquoise water. 1/30 s: the clean frame at sheet scale; 100 °C a fine grain at 1:1 | hold 1/30 s, raise the heat |
| H2 | 94 °C (the dark charge of 70 °C at half a second), 110 and 120 °C, all at 1/30 s | 94 °C: nothing at sheet scale. 110 °C: grain over the sky, the water greyer (its red channel rises from 4.7 to 23.7 of 255 as the dark noise rectifies at black), a pink glow over the rocks at the bottom left. 120 °C: coarse chroma grain, the corner pink | between 110 and 120 |
| H3 | 105, 110, 115 °C at 1/30 s, at 40 % and in 1:1 crops of the hills | 105 °C: grain at 1:1 only. 110 °C: heavy chroma grain at 1:1, a hot cross on the hill. 115 °C: grain at sheet scale and the glow plain in the corner, the frame still readable | 115 °C at 1/30 s, then dropped (H5) |
| H4 | 115 °C at 1/30 s in movie mode on the pan | Darker and green, the shadows crushed (see below); grain halved by the 4-column binning; the glow at the top left | fixed after stage 18 |
| H5 | The orchestrator's direction, from the panels agent's finding that a long exposure whites the sample out: the camera's 1/125 s with scene auto-exposure, 100, 110, 120 and 130 °C | 100 °C: nothing at sheet scale. 110 °C: grain at 1:1. 120 °C: grain over the sky at sheet scale; at 1:1 the shadows mottled in chroma, a hot cross on the hill, the water grainy, the corner only faintly pink. 130 °C: coarse grain, the corner pink | heat gun: 120 °C at 1/125 s, scene auto-exposure; the glow left to a dark scene |
| H6 | 120 °C in movie mode on the pan | Crushed and green as in H4, more so: the optical black now gathers eight times the image's dark charge | fixed after stage 18 |
| SB1 | 10 V at 1/125 s; with `[scene] headroom` 8 and 20; at 1/60 s | Nothing at sheet scale; headroom 20 changes a few trunk pixels. The sample is barely clipped (a few pixels at 255), and the inverse shoulder lifts its brightest values only to about 1.4 times white whatever the headroom | the bloom needs an overexposure |
| SB2 | 10 V against 12 V at 1/30, 1/20, 1/15, 1/13, 1/10 and 1/8 s, fixed exposure | 1/30 s: a cyan fringe along the hill crests, the sky's charge spilling into the hill within its column. 1/15 s: the water's blue floods the treetop. 1/13 s: white flares from the branches across the water, the treetop blue. 1/10 s: the trunk washes out. At 12 V the same exposures only clip | substrate: 10 V, 1/13 s |
| SB3 | 14 and 15 V at 1/125 s | 14 V: the clouds clip flat. 15 V: the clipped sky develops violet and the clouds pink, the processor assuming the nominal well. No bloom | the header's note on the raised bias |
| SB4 | The substrate preset in movie mode | The shutter caps the exposure at 1/15 s; the sky floods down its columns into the hills in cyan, the tree flares white; the doubled smear stays invisible at −90 dB | keep |
| HR1 | The loose-sync oscillator with 60 °C at 1/125 s and at 1/30 s, and with 100 and 110 °C at 1/30 s | 60 °C is loose sync unchanged at either exposure. 110 °C: grain over the held and the sheared stretches | the heat must be the heat gun's |
| HR2 | The loose-sync oscillator with the heat gun's sensor, 120 °C at 1/125 s with scene auto-exposure | Grain over the held and the sheared stretches alike; the shear is unchanged, the seed placing the snaps as in loose sync | hot reclock: 120 °C, as the heat gun |
| HR3 | The hot reclock in movie mode | Mostly held frames with magenta lobes, now and then one sheared into stripes with a wedge of blanking, as loose sync; darker and green as the heat gun in movie mode | keep |

The heat trades against the exposure. On the sample the look needs some 500 electrons
of dark charge, which at the camera's 1/125 s means about 120 °C (2^15.8 times the dark
current at 25 °C, 470 e) and at 1/30 s, the longest exposure auto-exposure holds on the
sample, 110 to 115 °C. The heat gun keeps the camera's exposure, so it holds any
daylight scene's brightness, in movie mode too; the table's "about 70 °C" and half a
second is the dim-scene version of the same bend. The grain is mostly the dark
current's fixed pattern (about 140 e rms at 120 °C, against 22 e of its shot noise and
15 e of read noise), so it stays put in a video. The readout corner's glow is 470 e at
120 °C and 1/125 s, faint against daylight; it shows on a lens cap or a night scene with
the exposure turned up (stage 16's lens-cap sheet: 70 °C at four seconds). The hot
reclock takes the heat gun's sensor whole, so the combination reads as loose sync on a
grainy sensor; at the table's 60 °C it would be loose sync.

The substrate's bloom is bounded by the deepened well. Two volts below nominal the well
holds 1.5 nominal wells while the ADC clips at about 1.05, so charge spills only from
sites that are clipped already by 0.6 stops, and the sample, exposed without clipped
highlights, has none until it is overexposed by three stops. The bloom then runs along
the CCD's columns, horizontal in the portrait frame: the sky spills sideways within the
sky, out of the frame, and shows only where a column crosses a hill crest; the bright
water spills into the tree it surrounds.

## Reset: `a520-reset-drag`

**Camera value (stage 17).** `[afe] fd_swing_v` 0.6 V (PROVISIONAL): the CCD's source
follower runs out before the AFE does, so the swing is its 460 mV saturation signal
plus a margin; the AD9949's 1.0 V p-p CDS input (C), which the plan first took, bounds
it from above. At 1.0 V the stage 17 sheets showed a weak reset on the sample only at
`reset_frac` 0.15; at 0.6 V it shows from about 0.25 down. The swing is 23,000 e at
26 µV per electron; a weak reset of fraction `r` reads a uniform run unchanged up to
`r` times the swing per packet and black from `r / (1 - r)` times it: at 0.2, a quarter
of the swing, 46 % of reference white at the daylight exposure.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| R1 | `reset_frac` 0.2 alone, and with `drop_hz` 2,000 at `hold_px` 100 and 300, 5,000 at 200, 10,000 at 100; 0.25 with 5,000 at 200 | 0.2: the lower half of the sky solarised, yellow, then orange, then black and green at the horizon, with a white line along the hills. 0.25: only a thin orange line at the horizon. The drops draw one-pixel streaks along the readout lines, blue or yellow by the row of the mosaic they black out and black near the horizon; each reads normally for a few pixels and then black. 10 kHz at 100 gives dense short dashes, 2 kHz at 300 sparse long lines | reset drag: 0.2 |
| R2 | 4,000 Hz at 250 | About 900 runs a still frame, long streaks over the whole frame without hiding it | reset drag: 4,000 Hz, 250 |
| R3 | The reset drag in movie mode on the pan | The solarised band holds from frame to frame, the streaks (horizontal in the landscape clip) move | keep |

The streaks are one pixel wide because a run of missed pulses blacks out part of one
readout line, one row of the mosaic; the demosaic turns a missing red-green row blue
and a missing green-blue row yellow. The solarised band comes from the mosaic's rows
crossing the swing in turn, the green-blue rows of the blue sky first.

## What the presets could not reach

- The table's settings for the heat gun (about 70 °C at half a second) and the hot
  reclock (60 °C) do not show on the daylight sample: the half second whites it out
  past auto-exposure's two stops, and 60 °C adds half an electron. Both presets run at
  120 °C with the camera's exposure and scene auto-exposure instead; the glow in the
  readout corner, part of the heat gun's look, needs a dark scene and a long exposure.
- The substrate preset blooms only overexposed, three stops over the daylight
  exposure; on a scene with real clipped highlights (the sun, glints) it would bloom at
  a shorter exposure.
- In movie mode the heat gun and the hot reclock first came out dark and green: the
  readout stage integrated the optical-black photosites' dark current over a whole
  frame (1/15 s) where the image's integrate over the shutter's exposure, 1/125 s, so
  at 120 °C the black clamp subtracted about 3,400 e too much, 27 % of reference white
  (rounds H4 and H6). Fixed after stage 18 in `readout::Params::resolve`: the masked
  photosites see the same shutter, so they integrate the image's exposure in both
  modes, with a test in `tests/sensor_bend.rs`.
- The glow's geometry and the sensor's uniform temperature are guesses: a heat gun
  pointed at one side would warm it unevenly, and the temperature does not drift
  through a video.

## Stage 19: `a520-stuck-bit`, `a520-hot-board`, `a520-serial-noise`, `a520-light-leak`, `a520-open-body`

Tuning of the five presets of the frame buffer's data pins and heat, the AFE's serial
link and the light leak. Stages 19b, 19c and 19d set the camera values and judged their
bends on contact sheets; stage 19f changed presets, goldens, benches and docs, and no
`vcb_core` code. Every preset was judged at native still resolution on the sample and in
movie mode on the pan, the serial and heat presets there first. The looks to reach are
the stage 19 list in `docs/PLAN.md`: a solid data pin, posterised with black lifted; the
SDRAM and the sensor under one heat gun; the serial link's flicker, tints and mazes; a
daylight leak on a stopped-down scene; and a red plastic leak with the full-spectrum
optics.

**Camera values (stages 19a to 19d).** `[memory] data_pins` 16 (C, the DQ lines of the
assumed x16 JEDEC part), `reference_temp_c` 25 °C (C, where `retention_s` is given),
`retention_s` 128 s (PROVISIONAL: a median of 2 s at the JEDEC limit of 85 °C, halving
every 10 °C, after the DRAM retention studies), `retention_halving_c` 10 (C, an
activation energy near 0.6 eV), `retention_spread` 0.5 (PROVISIONAL, the log-normal
sigma) and `refresh_ms` 64 (C, JEDEC). `[afe] serial_data_bits` 24 (PROVISIONAL),
`vga_bits` 10, `vga_min_db` 6 and `vga_db_per_code` 0.0351 (C, the AD9949's 6 to 42 dB
VGA), `pxga_bits` 6 (PROVISIONAL), `pxga_max_db` 18 (C), `clamp_bits` 8 (C) and
`pxga_gain_db` [0, 0, 0, 0] (PROVISIONAL: Canon balances white in the processor). The
camera's 6.2 dB is VGA code 6 and its pedestal clamp code 124. The leak adds no camera
value.

### Data pins: `a520-stuck-bit`

A word holds the processor's 10-bit code with the row's black level (31) added back, so
a stuck pin acts on one bit of every sample after the black clamp. Stuck high, pin `dn`
lifts black by `2^n` codes and, in every `2^(n+1)` codes, folds the lower half onto the
upper: tones `2^n` codes apart read alike, each colour of the mosaic in its own
contours. Stuck low, it takes `2^n` from every sample that carries the bit, folding the
highlights down.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| DP1 | One solid fault each: `d5`, `d6`, `d7`, `d8`, `d9` high; `d7`, `d8`, `d9` low | `d5` high: soft contours, blue and green bands in the sky, the hills greener. `d6` high: the sky in cyan and pink bands, the hills flat patches of magenta, green and brown, the water from cyan through blue to brick red with green blotches, the shadows magenta: posterised and readable. `d7` high: the frame washed pink with the contours on top. `d8` high: pale pink. `d9` high: nearly white. `d7` low: cyan bands in the sky, the water dark red. `d8` low: the sky folded down to red and orange. `d9` low: only the brightest specular highlights magenta, as the 19b sheet found | `d6` or `d7` high |
| DP2 | `d6` and `d7` high in 1:1 crops of the hills and the water | `d6`: flat patches with hard edges, the detail inside them kept; `d7`: the same contours under a pink wash, the 128-code lift developing pink in the ratio of the white-balance gains | stuck bit: `d6` high |
| DP3 | The stuck bit in movie mode on the pan | The same posterisation in every frame: a data fault needs no scaling | keep |

The 19b agent's marginal `d7` (30 %, 4 Hz), wide pink and magenta bands along the
readout lines lifted by 128 codes, is the moving version of the same fault; the preset
keeps a solid pin, as the plan's look asks.

### Retention: `a520-hot-board`

At the camera's values a cell decays when its hashed normal lies below
`ln(t_ref / τ_m) / 0.5`, with the median `τ_m` at temperature `T` being
`128 s * 2^(-(T - 25) / 10)`: at 110 °C 0.03 % of the cells, 0.3 % of the samples; at
115 °C 0.3 % of the cells, 3 % of the samples; at 120 °C 2.1 % of the cells, a fifth of
the samples. The sensor's heat at the camera's 1/125 s shows on a daylight frame from
about 110 °C at 1:1 and 120 °C at sheet scale (rounds H3 and H5).

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| HB1 | `[bend.sensor]` and `[bend.memory]` at one temperature, 100, 105, 110, 115, 120 and 125 °C, the camera's 1/125 s with scene auto-exposure | 100 to 110 °C: the clean frame at sheet scale. 115 °C: a light speckle over the sky and the water. 120 °C: dense speckle in fine stripes along the readout lines (vertical in the portrait still), the frame greyer but readable. 125 °C: the speckle buries the scene, the stripes plain | between 115 and 120 °C |
| HB2 | 1:1 crops of the hills: the sensor alone at 120 °C (`a520-heat-gun`), the SDRAM alone at 120 °C, both at 115, 118 and 120 °C | The sensor alone mottles the hills faintly. The SDRAM alone covers them in coloured crosses, bright and dark. Together at 115 °C sparse crosses over a faint grain, at 118 °C more, at 120 °C dense crosses with the grain between them | hot board: 120 °C, the heat gun's |
| HB3 | The hot board in movie mode on the pan at 115 and 120 °C | 115 °C: sparse pink and red dashes over a clean picture. 120 °C: dense coloured dashes along the lines, coarser than in the still since a movie frame has a quarter of the rows; the picture readable and its colours true (the movie optical-black fix after stage 18 holds) | keep |

At 120 °C the SDRAM's speckle dominates and the sensor's grain shows only between the
specks. The decayed cells belong to the chip, so in the pan the speckle stays where the
words are while the scene moves under it, each speck coloured by the bit its word
stored.

**`retention_spread`.** The orchestrator asked whether 0.8 or 1.0, the gentler onsets
the 19b agent offered, pair the two heats better under one heat gun. Rounds RS1 and RS2
ran the hot board's pair of sections on camera copies with the spread changed.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| RS1 | `retention_spread` 0.8, sensor and SDRAM at 90, 100, 105, 110, 115 and 120 °C | 90 °C: clean. 100 °C: a faint speckle at sheet scale, where the sensor shows nothing. 105 °C: speckle. 110 °C: clear speckle, the stripes starting. 115 °C: heavy. 120 °C: grey, the scene mostly gone | the SDRAM leads the sensor by about 10 °C |
| RS2 | `retention_spread` 1.0, the same temperatures | 90 °C: a trace at sheet scale. 100 °C: light speckle. 105 °C: speckle. 110 °C: heavy and striped. 115 and 120 °C: grey | leads by about 15 °C |
| RS3 | 0.5, the camera's (HB1) | The first specks at 1:1 from about 105 °C, at sheet scale from 115 °C; dense and readable at 120 °C; buried at 125 °C | keep 0.5 |

Recommendation: keep `retention_spread` at 0.5. With it the SDRAM's specks come up
between 105 and 120 °C, the window in which the sensor's heat shows at the camera's
exposure (hot crosses at 1:1 from 110 °C, grain at sheet scale at 120 °C), so one heat
gun shows both, and at 120 °C, where the sensor's grain first reaches sheet scale, the
SDRAM has lost a fifth of its samples and the frame still reads. With 0.8 the SDRAM
shows from about 90 °C at 1:1 and 100 °C at sheet scale, where the sensor is clean,
and at 120 °C two thirds of the samples hold a decayed cell and the frame is grey: a board heated far enough for the sensor's
grain shows the memory alone. 1.0 leads further. All three keep 25 °C inert. A gentler
onset would widen a memory-only heat knob (0.8 spreads it over 90 to 115 °C against 0.5's
105 to 125 °C), which the panel already gives in single degrees. Were 0.8 adopted, the
hot board would move to about 110 °C, where the sensor shows only hot crosses at 1:1;
the plan's and the module header's figures (a thousandth of the sites at 110 °C, a
fifth at 120 °C) would become about 15 % and 66 %, and the memory test that checks them,
the guide's readout figures and the hot board's golden would change with them.

### Serial link: `a520-serial-noise`

The processor sends 224 bits a frame (seven writes of an 8-bit address and 24 data
bits), so at `bit_error` `p` a frame takes a flip with probability `1 - (1 - p)^224`:
20 % at 0.001, 89 % at 0.01. Most flips land in padding, low VGA bits, the clamp
level, which the processor's black clamp takes back, or a lost write that rewrites the
same value, and show nothing. The flips depend on the seed and the frame index, and
the golden test forces seed 1.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| SN1 | `bit_error` 0.003, 0.01 and 0.03 in movie mode on the pan, seed 1, each frame against the clean movie | 0.003: 5 of 45 frames off by more than 3 levels, four of them plain: a saturated green frame (a green-phase gain), a pink one, a warm pale one, a blue one. 0.01: those and a black frame (a tristate, its held word within the processor's clamp range), a cyan one and small shifts, 10 of 45 off. 0.03: 32 of 45 off, black frames, greens and pinks in runs, few clean stretches | serial noise: 0.01, a glitch every half second or so |
| SN2 | Frame 0 at seed 1 as a still, `bit_error` 0.001 to 0.03 | Clean up to 0.015; the first flip that shows, at 0.02, a slight shift | the golden at seed 1 renders clean |
| SN3 | Frame 0 of seeds 1 to 40 at 0.01 | 17 of 40 show a flip: overexposed (seeds 5, 12), red tints (11, 15, 29, 34, 36), blue (13, 33), green (17, 22, 31), brighter (10, 21, 32) | a green seed for the still |
| SN4 | Native stills of seeds 5, 11, 13, 17, 22 and 33; 1:1 crops of 17, 22 and 11 | 17: a green cast with a fine checker maze where the two greens part, the scene readable. 22: a deeper green, the same maze. 11: a pink frame | serial noise: seed 17 |
| SN5 | The preset in movie mode on the pan with seed 17 | A green first frame, two black ones (a tristate carried into the next frame by a lost write), a pink frame at one second, a slight blue, a red tint at two seconds, a cyan frame: 9 of 45 frames off | keep |

The preset is a movie preset at heart; its seed 17 gives a still, frame 0 from power-on,
the green maze. Its golden, at seed 1, equals `a520-clean`'s: frame 0 takes no flip
that shows, so the golden guards the clean writes, which restore the camera's exact
values. Tristated frames developed black on the pan, the held word (the frame before's
last sample, a dark one) lying within the processor's clamp range; the dark maroon or
mauve flat frames of the 19c sheets come where the held word lies above it.

### Light leak: `a520-light-leak`, `a520-open-body`

The sensor's left edge is the bottom of the upright portrait sample and the left of the
landscape pan. Scene auto-exposure counts the leak's mean charge against its target,
and on the sample a leak bright enough to see takes the scene to the two-stop limit at
once.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| LL1 | The section's defaults (0.5, spread 0.3, daylight, `nir` 1) at the camera's fixed exposure; with scene auto-exposure at 0.3, 0.5 and 1.0; 0.5 at spread 0.5; 1.0 at spread 0.2 | Fixed: the grass and water washed pink and white, the rest of the scene as clean. Scene auto-exposure: the scene stops down, the sky navy and the hills dark; 0.3 a pink veil over the lower half, 0.5 the bottom near white and the sky mauve, 1.0 the lower third white, spread 0.5 a pink veil over the whole frame | scene auto-exposure; a tighter spread |
| LL2 | Scene auto-exposure at spreads 0.1 to 0.2: 0.3 and 0.5 at 0.2, 0.5, 1.0 and 2.0 at 0.15, 2.0 at 0.1 | 0.5 at 0.2: the glow rises from the grass, white at the edge and pink over the water, the sky deep blue above it. Tighter spreads put a hard white band at the edge | light leak: 0.5, spread 0.2 |
| LL3 | Band means of the leak against the clean frame with scene auto-exposure | The top tenth at a quarter of the clean frame's linear brightness, auto-exposure at its two-stop limit; the bottom tenth nine times brighter and clipping | the stopped-down scene is the look |
| LL4 | 0.3 and 1.0 at spread 0.2 | 0.3: a soft glow at the edge over the dark scene; 1.0: a quarter of the frame white | the header's bracket |
| LL5 | The light leak in movie mode on the pan | The glow from the left of the landscape frame over the stopped-down scene, as in the still | keep |
| OB1 | The optics of `a520-full-spectrum` with a red plastic leak (`colour` [1, 0.4, 0.25], `nir` 1.5, 0.5, spread 0.3) under daylight and grey-world balance; grey world at 0.3 and 1.0, at spread 0.2 and from the top edge | Daylight: the frame one magenta-pink, the scene dark mauve. Grey world: a teal sky and slate water, the leak a peach-white glow rising from the grass; 0.3 a brownish haze; 1.0 the lower third white; from the top edge a white band down the upright frame's left side | grey world |
| OB2 | Grey world with `nir` 1.0 and 1.5, radiance 0.5 and 0.7, spread 0.3 and 0.4, a redder plastic [1, 0.25, 0.1]; daylight with `nir` 1.0 | `nir` 1.0 and 1.5 differ little under grey world, which takes the infrared's cast back; 0.7 washes the water; the redder plastic warms the glow a little towards salmon; daylight with `nir` 1.0 still turns the frame red-pink | open body: 0.5, spread 0.3, [1, 0.4, 0.25], `nir` 1.0, grey world |
| OB3 | Fixed custom gains instead of grey world: red 0.9, 1.0 and 1.2 with blue 1.7 to 1.9 | The scene violet-blue, the glow lilac to pink; the leak's infrared outweighs its red at any fixed gains | grey world stays |
| OB4 | The open body in movie mode on the pan | The peach glow from the left over the teal scene | keep |

`nir` 1 is the physical value for a plastic clear in the near infrared: the leak then
carries daylight's infrared whatever its visible colour. The 19d agent's 1.5 changes
little under grey world.

### What the stage 19 looks reach, and their limits

- The stuck bit posterises by the bit's weight in every channel and lifts black by the
  same; it is the same in every frame and at any size. A lower pin than `d6` softens
  the contours, a higher one washes the frame pink, then white.
- The hot board's SDRAM dominates at 120 °C, the sensor's grain showing only between
  the specks; at 115 °C the two balance at 1:1 but the SDRAM's speckle is thin at
  sheet scale. The heat is uniform and the same on both chips; a heat gun pointed at
  one would warm the other less, and neither drifts through a video. A cooled board
  reads its stored bits again at once, where a real chip keeps a lost bit until the
  next write.
- The serial preset shows in a still only through its seed's first frame, and its
  golden is clean. Of the seven registers the clamp level shows nothing, the processor
  taking its pedestal back. The timing core's registers are not modelled, so a
  misdirected write never loses the frame's sync, a look a real AD9949 could give.
- The light leak's glow is pink because of its infrared; a white glow needs `nir` 0.
  Scene auto-exposure stops the sample down to its limit for any visible leak, so a
  dim leak over a bright scene wants the fixed exposure. The still's smear of the
  leak alone, about 1 % of its column means, is invisible on the sample.
- The open body's red plastic reads peach under grey world, which balances the frame's
  mean, the leak included, and pink under any fixed balance, the infrared outweighing
  the red; a red leak over a natural full-spectrum scene is not within reach of the
  white balance.
