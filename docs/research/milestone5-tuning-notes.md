# Milestone 5 tuning notes (stage 21b)

Tuning of the seven presets that bring the JavaScript gallery's physics presets into the
chain (`docs/research/legacy-presets-audit.md`, section 3, item 1): two on the bus bend,
three on the analog front end and two on the CCD's transfer clocks. Every knob they use
existed before this stage; only presets, goldens and documents changed, no Rust code and
no camera value. As in milestones 3 and 4 (`milestone3-tuning-notes.md`,
`milestone4-tuning-notes.md`) no reference photos exist for these bends, so every
judgement is by eye: on `samples/sample-01.jpg` at native still resolution, in movie
mode on the 3 s pan (`validation/clip-pan.mp4`, gitignored), and at the gallery's
thumbnail size (the sample reduced to 200 x 300 and rendered at match-input
resolution). The sample is the portrait lake under a clear sky with a dead tree; its
readout lines run vertically in the upright frame, the first on the left, so anything
that acts along a line (the hum's bands, the CDS echo, the register's shear) runs up
and down the upright still, and anything that acts along a column (transfer loss, V
stalls) runs across it. Round sheets lived in the session's scratch space and are not
committed.

The legacy presets' parameters and descriptions were the brief for the look, never the
method: the legacy modules worked on knobs normalised to the frame, the chain on
volts, ohms, hertz and pixel periods. Where the chain does not reach a legacy look the
reason is recorded below rather than forced.

## Bus: `a520-cloud-solarizer`, `a520-dc42-solarize`

**Cloud Solarizer** (legacy: D11 into D10, pot at 0.7 of its travel, "one MSB gently
leaks into the next bit down: pastel solarized skies, clean midtones"). One source pin,
D11, function bypass, target D10, no patch resistor.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| CS1 | The schematic's 1 uF; pot 0.05, 0.2 and 0.7 kOhm on S, 0.1 and 0.7 kOhm on F; 47 ohm patch resistors at 0.7 kOhm on F | 0.7 kOhm on either node: clean, with cyan blocks in the brightest sky at the horizon. 0.1 kOhm on F and 0.05 on S: the whole sky red and orange, the tree pink, violet glints on the water. 0.2 on S: bistable blocks. 47 ohm: clean | the look lies between clean and red |
| CS2 | 1 uF, pot on F at 0.17, 0.2, 0.25, 0.3, 0.4 and 0.5 kOhm | Red up to 0.3, clean (a faint pink block) from 0.4: a step, no grading | look for a grading |
| CS3 | 1 uF on F at 0.33 and 0.36 kOhm; 0.1 kOhm on F with 100, 200 and 400 ohm patch resistors | 0.33 and 0.36: the sky in bistable vertical blocks, lavender and red; any resistor from 100 ohm: clean | the 1 uF capacitor flips the whole sky |
| CS4 | 0.3 kOhm on F with 0.1 and 0.01 uF; pot on S at 0.3, 0.5 and 1.0 kOhm; 60 ohm at 0.1 kOhm | 0.1 uF: violet upper sky, orange horizon. 0.01 uF: the sky clean but for a lavender and orange band over the horizon. On S: clean, or at 1 kOhm cyan blocks. 60 ohm: red | a smaller capacitor grades the fold |
| CS5 | 0.3 kOhm with 22 and 47 nF; 0.25, 0.2 and 0.15 kOhm with 10 nF; 0.2 kOhm with 4.7 nF | The lavender spreads up the sky as the pot falls: at 0.25 kOhm a band over the orange horizon, at 0.2 most of the sky, at 0.15 the sky red again. 4.7 nF is 10 nF's twin | 10 nF, 0.2 kOhm |
| CS6 | The choice at 1:1, and in movie mode | The lavender is a fine checker of the mosaic, the horizon's clouds a flat orange, the hills untouched; the movie keeps the lavender sky with a green and orange rim at its top | keep |

The capacitor charges through D11's 250 ohm driver and, on F, the pot in parallel with
D10's driver: about 0.35 to 0.4 kOhm in all. With 1 uF that is a time constant of some
400 us, three still lines, so the capacitor's charge follows the mean of the whole sky
and the sky flips as one: clean above 0.36 kOhm, red below 0.33, bistable between. 10 nF
shortens it to about 4 us, a hundred pixel periods, so the charge follows the sky's
brightness along each line and the fold grades with it (an estimate from the circuit's
values; the renders show the grading, not the time constant). A 10 nF ceramic in the capacitor's socket is a
bender's swap; the legacy knob's "pot 0.7" was a position on its own scale and does not
transfer. The legacy look is reached in kind: solarised, pastel (lavender) skies, an
orange fold of the brightest clouds, clean midtones; the white tree and the water's
glints turn pink with them, since they too hold D11.

**DC42 Night Solarize** (legacy: D9 and D11 into D8 and D10 on a common bus, pot 0.75,
patch resistance 0.55 of its range, latch jitter: "every tapped pin colliding on one
common bus: nocturnal luminance noise and solarized rainbow skies"). The chain's target
is a single pin, so the four lines join as sources D11, D9, D8 and target D10 with
`common_bus`, which makes S and T one node; in bypass F is that node too, so the
capacitor is shorted and only the pot loads the bus.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| DC1 | Pot 0.75 kOhm, patch resistors 0, 47, 100, 150, 220 and 330 ohm | 0 and 47 ohm: the frame posterised into rainbow fields, cyan, red and green skies, green and red water. From 100 ohm: nearly clean, a green patch on the clouds | the look lies between 47 and 100 ohm |
| DC2 | 56, 62, 68, 75, 82 and 91 ohm | 56: the rainbow, less wild. 62: red and green arcs over a cyan sky, red and violet bands on the water, magenta patches on the hills, speckle at 1:1. 68 to 91: the arcs give way to cyan, the water's red band and the hills' magenta stay | 62 ohm |
| DC3 | 62 ohm with the pot at 0.5 and 1.0 kOhm, on S; 60 and 65 ohm; sources D11, D10, D9 into D8 | 1.0 kOhm, the S node and 60 to 65 ohm look alike; 0.5 kOhm is wilder; the other target is the same circuit | keep 62 ohm, 0.75 kOhm, target D10 |
| DC4 | Movie mode on the pan | The arcs hold over the moving sky, the water's bands follow the water | keep |

The legacy look is reached: rainbow solarised skies, speckle where the noise tips the
contest. "Nocturnal" fits less: the frame is not darker than the clean one, only its
water and hills are. The 62 ohm sits 6 ohm below the step to the cyan sky, so the look
is narrow; it holds from 60 to 65 ohm.

## Analog front end: `a520-ground-loop`, `a520-carrier-clash`, `a520-reset-ghost`

**Ground Loop** (legacy: injection 0.32, about a tenth of the range, at 0.06 cycles per
row: "mains hum injected into the analog path: rolling horizontal interference bands").
A 50 Hz sine at the AFE input.

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| GL1 | 30, 60, 100, 200 and 300 mV; 100 mV with `phase_skew` 0.01 | 30 mV: eleven soft cycles of cyan and dark bands. 60 mV: deeper troughs. 100 mV: the troughs black, the crests pink. 200 and 300 mV: a black and pink barcode. The skew packs the bands 2.5 times closer | between 30 and 60 mV |
| GL2 | 40 and 50 mV; 50 mV with `phase_skew` -0.0066 and 0.003 | 40 and 50 mV look alike: bands that read as interference and leave the scene readable. -0.0066 cancels the hum's step from line to line: no bands, the clamp takes the constant offset. 0.003: sixteen cycles | 50 mV, no skew |
| GL3 | The mean luma difference against the clean frame across the lines | Period 152 lines (50 Hz on 132 us lines); in each cycle a sharp rise of about +15 then a slow fall to -15 | the clamp's residue, below |
| GL4 | Movie mode on the pan | Three bands a frame, horizontal in the landscape clip, rolling a third of a cycle per frame | keep |

The processor's black clamp follows each line's optical black with a four-line lag and
corrects within 128 codes, so it takes most of a hum back, as a real camera's line
clamp does. What shows is what it cannot follow: the lag on the hum's rising slope, and
the troughs, where the optical black clips at code 0 and the clamp can follow only the
black level's 31 codes down. So the bands need more hum than the legacy's tenth of the
range would suggest only in their troughs; "a few hundred millivolts", as first
proposed, turns the frame into a barcode. A hum cannot tilt its bands: its phase moves
0.004 cycles across a line's samples, so the bands lie along the readout lines whatever
`phase_skew` does, which only respaces them; the preset keeps the skew at 0, a free
mains hum having no per-line step. The legacy's "wobble" (phase drift) has no
counterpart; in a movie the hum's 50 Hz against the 15 fps frame rolls the bands anyway.

**Carrier Clash** (legacy: a square at 21 cycles per row, skew 0.18 cycles per row,
injection 0.3, gain modulation 0.25 from the same oscillator: "a square-wave carrier
beating against the pixel clock: tilted moire and pumping gain").

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| CC1 | 50 mV squares at 27 MHz + 242.4 kHz (1796 + 32 cycles a still line, so level from line to line) and at 242.4 kHz; skew 0, 0.01, 0.18; a gain square at the carrier's frequency, depth 0.15; a 30 Hz sine on the gain | Skew 0: level bands, twenty a line, their troughs clipped black. 0.01: diagonal stripes. The two frequencies look alike at native resolution. 0.18 at 100 mV: a fine grey hatch, the frame washed out. The gain at the carrier's frequency barely shows | carrier tilted by 0.01, smaller |
| CC2 | 20 mV at 13.5 MHz + 121.2 kHz (1798 cycles a line) and at 27.24 MHz; gain modulation at the carrier's frequency and at 12 Hz | Near half the pixel clock the carrier flips sign from sample to sample and lands on the mosaic's two colours in opposite senses: orange and blue diagonal bands, a colour moiré. Near the pixel clock it stays luminance stripes with a pink and teal tint | half the pixel clock |
| CC3 | 20 mV, skew 0.01 and 0.005, with a 30 Hz square or sine on the gain (depth 0.25, 0.3); 30 mV with a gain square at the carrier's frequency, depth 0.3; 15 mV, skew 0.003 | The 30 Hz square pumps the exposure in blocks along the readout lines under the diagonal moiré. The gain at the carrier's frequency, which has no skew, lays level yellow bands across the tilted ones: a plaid | 20 mV, skew 0.01, a 30 Hz square at 0.25 |
| CC4 | Movie mode on the pan; the gallery's thumbnail | The movie's 149.5 us line is 2036.16 carrier cycles, so the bands step a sixth of their period a line: a fine, nearly level hatch. The thumbnail's slower pixel clock aliases the carrier into a finer diagonal hatch | recorded in the header |

The legacy look is reached at native still resolution: a tilted moiré, here coloured by
the mosaic, and a pumping gain. Two things differ. The chain's `gain_mod` has its own
oscillator and no `phase_skew`, so one oscillator cannot drive both paths with the same
tilt as the legacy's did; the pumping here comes from a second, slow oscillator. And a
carrier tied to the pixel clock gives a different pattern wherever the clock or the
line differs: the movie readout and the previews and thumbnails show their own hatch.
A carrier at 242 kHz, well below the pixel rate, would keep the look at every size, but
loses the colour that the sample-to-sample flip gives.

**Reset Ghost** (legacy: CDS amount 0.85, a skew of about six clocks: "CDS sampling the
wrong pixel: the frame collapses into an embossed derivative with negative trails").

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| RG1 | Mix 0.85 with lags of 1, 3, 6 and 10 pixel periods; lag 6 with mix 0.5 and 1.0 | Odd lags subtract the neighbouring colour: the frame floods green. Even lags subtract the same colour: a dark relief, navy sky, white rims on the hill crest. Mix 1.0: only the edges; 0.5: the scene keeps its colours under the relief | even lag, mix 0.85 |
| RG2 | Lags 6, 10 and 16 at mix 0.85, mix 0.7 at lag 6, at 1:1 on the tree | 6: a fine relief; 10: a clear emboss with orange and blue rims and black trails; 16: broad | lag 10 |
| RG3 | Movie mode on the pan; the gallery's thumbnail | The relief holds in the movie; the thumbnail's echo is wider, the lag being counted in the processor's pixel periods | keep |

Reached as described. The legacy's skew was in clocks of its render; the chain's
`cds_lag_px` is in the processor's pixel periods at every resolution, so a preview shows
a wider echo than the still.

## Clocks: `a520-a540-melt`, `a520-a530-warp`

**A540 Melt** (legacy: transfer loss 0.45, a melt decaying over some 30 native rows; V
skips 0.5, about twelve events a frame of 16 rows on average; bloom 0.35: "vertical
transfer clocks failing mid-readout: paint-drip melts, luminance streaks, stalled row
stretches").

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| AM1 | `cte` 0.9995, 0.999, 0.998, 0.995, 0.99 and 0.98, no V faults | Every value pinks the frame from the side away from the register: the deferred charge lands in the next row of the column, the other row of the mosaic, and the daylight gains turn the mixture magenta. 0.99 and 0.98 smear the tree across the water | the melt brings the magenta |
| AM2 | Grey world (`wb = "auto"`) at 0.9995, 0.998, 0.99; V faults at 20 Hz x 8 lines and 50 Hz x 15 lines | Grey world turns the magenta into a grey wash with a green edge. 50 Hz stripes the frame all over with stalled stretches | daylight gains; fewer faults |
| AM3 | 0.99 with 12 Hz x 12 lines, under daylight and grey world; 0.995 with 25 Hz x 20; 0.98 with 12 Hz x 30 | Stalled stretches as orange and blue bands; 0.98 washes the tree out | 0.99 |
| AM4 | V faults at 20 Hz x 16 lines with the camera's `cte`, 0.9998, 0.9995 and 0.999 | With the camera's `cte` and 0.9998 the odd stall's flip of the Bayer row phase shows plainly: natural colour, then magenta below the stall. From 0.9995 the transfer loss's own magenta hides it | the flip and the melt exclude each other |
| AM5 | 0.997, 0.995, 0.99 with 20 Hz x 16 lines; 1:1 on the tree; movie mode | 0.99: branches run in drips across the water, about 17 lines long at the far side; in the movie, whose 426 lines make the same 17-line trail four times longer against the frame, the tree drips down the frame in magenta | 0.99, 20 Hz, 16 lines |

The melt and the stalled stretches are reached; the frame's magenta is the physics of
transfer loss on a colour sensor, which the legacy's melt (on raw values with no
colour mixing in its look) did not show. The Bayer row flip below an odd stall, which
the brief names, shows only with little transfer loss (0.9998 or better); at a loss
that melts, the rows' colours are already mixed and the flip disappears into the same
magenta. The legacy's bloom is `a520-substrate`'s lowered substrate bias, which needs
its overexposure and is left out. At the gallery's size the melt is much longer against
the frame than at native resolution (see "Findings").

**A530 Warp** (legacy: H shear 0.6, about sixteen bands a frame shifted within 13 % of
the width, with light transfer loss and V skips: "horizontal register glitches: sheared
scanline bands, Bayer phase tears, rainbow fringes").

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| AW1 | `h_glitch_hz` 30, 70 and 120 at a shift of 0.13; 70 Hz at 0.05 and 0.2; 70 Hz with V faults at 10 Hz | Thin sheared bands, the odd shifts in checkered magenta; 30 Hz a few, 120 Hz busy. The V faults flip a whole stretch magenta | 100 Hz, 0.15, no V faults |
| AW2 | 1:1 on the hills; movie mode | A thick band slides the sky and the water along the line; thin ones are magenta lines with coloured edges; in the movie the bands move from frame to frame and cut the tree into offset pieces | keep |

Reached, with thinner bands: the chain's band widths are the readout's fixed rule (2 to
4 and 8 to 22 native lines), half the legacy's against the frame, and no preset knob
sets them. The legacy preset's light transfer loss and V skips are left out, since the
transfer loss pinks the frame (they are `a520-a540-melt`'s). The rainbow fringes are a
checkered magenta with coloured edges.

## Final settings

| Preset | Bend | Settings | Golden hash |
|---|---|---|---|
| `a520-cloud-solarizer` | bus | D11 to D10, bypass, 10 nF, pot 0.2 kOhm on F | `ed27a8a9ffd2e210` |
| `a520-dc42-solarize` | bus | D11, D9, D8 and D10 on a common bus, bypass, 62 ohm patch resistors, pot 0.75 kOhm | `58df8284b10f25b5` |
| `a520-ground-loop` | AFE | sine 50 Hz, 50 mV, no skew | `a89d61f5dea9193f` |
| `a520-carrier-clash` | AFE | square 13,621,212 Hz, 20 mV, skew 0.01; gain square 30 Hz, depth 0.25 | `08d4d02225c0daf9` |
| `a520-reset-ghost` | AFE | `cds_lag_px` 10, `cds_mix` 0.85 | `0c98e9de0f508340` |
| `a520-a540-melt` | clocks | `cte` 0.99, V faults 20 Hz x 16 lines | `9fbed58319c0f3f4` |
| `a520-a530-warp` | clocks | H glitches 100 Hz, shift 0.15 | `a133ecafdc133b1c` |

All seed 1. The goldens (160 x 120 at match-input resolution) show each bend: the
cloud solarizer's violet fold below the disc, the common bus's posterised fields, the
hum's level bands, the carrier's diagonal hatch, the reset ghost's echo of the disc, a
melt dripping from the disc and the warp's sheared bands. The 25 earlier goldens are
unchanged: a description does not render.

## Findings

1. **The transfer-loss trail does not scale with the sensor.** The readout module says
   the trail's extent as a fraction of the frame does not depend on the virtual
   sensor's size, but the release fraction (`readout.rs`, the transfer loop) uses the
   native transfer count as a number of virtual lines: the trail keeps its length in
   lines, so it reaches further across the frame by the ratio of the native rows to the
   virtual ones, about eight times on the gallery's thumbnail of the sample and
   fourteen on the goldens' 120 rows, and the melt's thumbnail is a wash. The movie
   readout, whose lines keep one row pair in four, shows the same trail four times
   longer against the frame than the still, which may be what a line-skipping
   readout does with the charge of the rows it dumps, or the same omission. Scaling
   the mean delay by `sensor_height / native_height` would match the documentation
   for the virtual sensor. Not changed here (no core code in this stage).
2. **`cds_lag_px` counts the processor's pixel periods**, so previews show a wider echo
   than the still; the reset gate's `hold_px`, by contrast, counts the camera's own
   periods at every resolution. Counting the lag the same way would keep previews
   faithful.
3. **`gain_mod` has no `phase_skew`**, so one oscillator cannot drive the input and the
   gain with the same tilt, as the legacy's carrier did.
4. **No camera value change is needed** for these presets.
