# Milestone 4 tuning notes (stages 16 to 18)

Tuning of the four presets of the sensor and reset bends and the PROVISIONAL camera
values they rest on. As in milestone 3 (`milestone3-tuning-notes.md`), no reference
photos exist for these bends, so every judgement is by eye: on `samples/sample-01.jpg`
at native still resolution, and in movie mode on the 3 s pan
(`validation/clip-pan.mp4`, gitignored). The sample is the portrait lake under a clear
sky with a dead tree; its readout lines run vertically in the upright frame, the first
on the left, so the CCD's columns run horizontally, and the readout's first corner is
the bottom left (in the landscape clip, the top left). Stages 16 and 17 set the camera
values from their contact sheets; stage 18 changed presets and the command line's
preset listing, and no `vcb_core` code. Round sheets lived in the session's scratch space and are not committed.

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
