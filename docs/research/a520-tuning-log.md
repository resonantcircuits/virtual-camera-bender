# A520 bus-bend tuning log (stage 6a)

Tuning of the HPF, DIV and INV presets and the PROVISIONAL camera constants against the
reference composite `reference-camera-1/...kukfamkews8h1` (quadrants cropped into
`validation/a520-ref-{hpf,div,inv}.png`), all renders at native resolution
(`scripts/validate-a520.sh ... --resolution native`). Only configuration changed; no Rust
code was touched. Round sheets are in `validation/round-NN.png` (gitignored, like the
references).

Metrics (advisory, 536 x 398 area downscale): `hue` is the total-variation distance of
36-bin chroma-weighted hue histograms between our render and the reference quadrant
(0 equal, 1 disjoint); `grad` is the mean absolute luma gradient (texture); `spk` is the
mean distance of luma from its 3 x 3 median (single-pixel speckle). Reference values:
HPF grad 5.6, spk 2.5; DIV grad 13.7, spk 8.0; INV grad 8.0, spk 2.9, luma 38.
The DIV and INV references are a different, tighter framing than the input crop, so only
the HPF quadrant supports region-by-region comparison.

## Rounds

| Round | Change | Effect | Keep / drop |
|---|---|---|---|
| 00 | Baseline (`round-00.png`): D9, D8, D7 to D10, 0.5 kOhm pot on F, all three functions | HPF hue 0.723, DIV 0.556, INV 0.473. Every look keeps bright clouds and a dark blue/green sky; the HPF sky is green where the reference is magenta | reference point |
| 01 | Operating-point probe of the HPF: pot on F and S, 1, 2, 3 source pins, targets D10 and D11, `driver_r_ohm` 100, 150, 300 (36 renders) | No bypass variant raises a bit in the dark sky. The pot on S with a weak driver (300 ohm) pulls the source bus low in long bright or sky runs: D9 drops out of the G and B sites and the sky turns dark red | lead kept: driver strength against the pot decides the source-bus level |
| 02 | Topology search, 1218 renders: bypass, invert, divide x pot on F/S x every contiguous source run of 1 to 4 pins x every target D4 to D11 | Only inverter topologies latch a high target in dark regions; bypass never does. Best HPF hue 0.327 (invert, pot on S, D9-D7 to D11) but with bright magenta clouds | informational; see "What still differs" |
| 03 | Bypass x `dsp_threshold_v` 0.9-1.9 x driver 100/250 x pot node x pot 0.2/1.0 x 5 source sets x D10/D11 (320) | A low threshold lifts everything (luma up to 198) without the reference polarity | drop thresholds below 1.2 V |
| 04 | Grey-world AWB (`isp.wb = "auto"`) on bypass and invert candidates (10) | Bypass practically unchanged, invert slightly redder | drop (WB mode is outside the tuning brief) |
| 05 | Noise-dominated low source bits (D4-D2, D5-D2), smaller coupling caps (0.1, 0.022 uF) with five to eight sky bits (24) | Low bits clear the target in bright cores but leave the sky native; many source bits posterise the sky gradient into rainbow bands | drop |
| 06 | Bypass, driver 250-550 ohm x pot on S 0.1-1.0 kOhm x threshold 1.3-1.7 x source/target sets (351) | D10, D9 to D11 with the pot on S reproduces the reference layout: red sky, olive-green diagonal band top right, navy and red rings, dark cloud cores, texture matching (grad 6.0 / 5.6, spk 2.7 / 2.5). The look is a plateau across drivers 250-550 and thresholds 1.3-1.7 | keep topology; camera `driver_r_ohm` 150 -> 250, `dsp_threshold_v` 1.5 -> 1.3 |
| 07 | Around that point: AWB, driver 150, `white_fraction` 0.6/0.8, headroom 1.5/4, pot 0.05/0.15, `dsp_noise_mv` 1 | Headroom 1.5 removes glitch blocks in the clipped left cloud; white fraction only slides the sky bands; 1 mV of latch noise is indistinguishable from 5 mV (hue 0.467 / 0.472) | keep HPF `[scene] headroom = 1.5`; keep `white_fraction` 0.7 and `dsp_noise_mv` 5 |
| 08 | HPF pot margin, 0.03-0.3 kOhm on S | With two source lines high the bus sits at `3.3 V * (2/250) / (2/250 + 1/R_pot)`: 1.2-1.5 V for 0.07-0.1 kOhm, on top of the 1.3 V threshold, and the contention resolves in bistable horizontal blocks (green bars). At 0.05 kOhm (0.94 V) it resolves cleanly | keep `pot_kohm = 0.05` |
| 09 | INV: invert topology search (406), pot node, the new camera, then target, pot, headroom and source refinements (39) | The inverter with the pot on S gives the near-black sky: at black the source bus is low, the inverter drives the target high in the optical-black columns, and the DSP clamp subtracts that raise from every line. D11-D8 to D7 adds glowing white/yellow/cyan clouds with dark-blue cores (hue 0.446; D10-D8 to D7 scores 0.436 but has no cores) | keep D11, D10, D9, D8 to D7, pot on S 0.5 kOhm |
| 09b | INV warm-up: 30 still frames (7 s of camera time) through `render-video` | The 1.65 s input-leak time constant slowly adds sky texture; polarity unchanged | informational |
| 10 | DIV: divide topology search (406), then candidates under the new camera (11) | Target D9 gives white clouds with green/blue/pink rings and a red sky with horizontal streaks; D8 to D9 with the pot on S matches the texture (grad 11.5 / 13.7, spk 8.0 / 8.0) | keep D8 to D9, pot on S 0.5 kOhm |
| 11 | Clean camera on `samples/sample-01.jpg`: `chroma_nr_frac` 0.002 -> 0.0008; sharpening 0.0005/0.6 -> 0.0004/0.5; contrast 1.2 with toe 0.03 | Yellow flowers stay yellow at 0.0008 (0.0006 is not visibly better); thinner light halos on twigs against the water; the stronger curve is not an improvement | keep chroma 0.0008, sharpen 0.0004/0.5; keep tone 1.15/0.02/0.10 |
| final | `round-02.png` = `a520-comparison-final.png`; `sample-final.png` | see below | |

## Final settings

| Preset | Function | Sources | Target | Pot | Other |
|---|---|---|---|---|---|
| HPF | bypass | D10, D9 | D11 | 0.05 kOhm on S | headroom 1.5 |
| DIV | divide | D8 | D9 | 0.5 kOhm on S | |
| INV | invert | D11, D10, D9, D8 | D7 | 0.5 kOhm on S | |

All keep the 1 uF capacitor, no patch resistor, no common bus. Camera: `driver_r_ohm`
250, `dsp_threshold_v` 1.3, `chroma_nr_frac` 0.0008, `sharpen_radius_frac` 0.0004,
`sharpen_gain` 0.5.

| | HPF hue / grad / spk | DIV hue / grad / spk | INV hue / grad / luma |
|---|---|---|---|
| reference | - / 5.6 / 2.5 | - / 13.7 / 8.0 | - / 8.0 / 38 |
| round 00 | 0.723 / 9.6 / 4.4 | 0.556 / 17.4 / 14.0 | 0.473 / 9.7 / 93 |
| final | 0.544 / 5.7 / 2.5 | 0.464 / 11.3 / 7.8 | 0.460 / 9.2 / 93 |

## What still differs and why

**Model limitations.**

1. *The bypass path cannot raise a dark flat region.* In bypass every node is a
   resistive mix of the ADC drivers plus the capacitor's slowly varying charge, so in a
   flat region a touched line can only read high if enough touched drivers there are
   high. The reference HPF sky is magenta (R and B raised, G crushed), which needs a
   raised bit in the darkest areas and different outcomes for G and B sites whose codes
   share the same high bits. Ours is the dark-red version of the same layout: the cloud
   cores and the band structure match, the raise does not. The 1218-variant search found
   the raise only behind the inverter.
2. *Stills start from power-on.* The real camera runs live view before the shot, so the
   coupling capacitor and gates are already at their operating point. With the pot on
   S, F's only DC paths are the input leakage and clamp diodes (1.65 s), so the power-on
   state is not representative; the warm-up test changes the INV texture.
3. *Exposure and white balance do not see the bend.* DIGIC meters AE and AWB on the
   data it receives, i.e. on the bent raw. Our exposure is fixed and the WB is daylight,
   so a bend that darkens the frame (our HPF: luma 60 against 78) is not compensated as
   the camera would have done.
4. *DIV holds.* Our streaks are mostly whole-line black-level jumps (the flip-flop's
   state in the optical-black columns, through the clamp loop) and pixel-scale holds;
   the reference's holds of tens of pixels inside the sky are rarer.

**Unknown constants.** The exposure and white balance of the reference shots (and
whether AE/AWB were on), the DSP input threshold and the AFE driver resistance (both
now PROVISIONAL at 1.3 V and 250 ohm), the black-clamp window and optical-black count,
the CFA phase, and the actual pin wiring of the reference shots (the original write-up
was not found; our pins are fitted, not documented). The DIV and INV quadrants are also
a different framing, which inflates their luma and hue distances.

**Recommended model changes.** (a) Meter AE and AWB on the bent raw, as DIGIC does,
with a still captured after a few live-view frames so the bend's capacitor and the
metering are both in their operating state; this is the cheapest physically grounded
change and addresses items 2 and 3. (b) If the HPF raise is still missing after (a),
give the bus lines their input capacitance and the drivers their slew, so the latch can
catch a line mid-transition; that is the only route the circuit offers to site-dependent
bits between neighbouring pixels.

## Orchestrator check after the merge (2026-11-05)

Recommendation 1 above was tested as far as the current model allows: the three bend
presets rendered at native resolution with `[isp] wb = "auto"` (grey-world on the bent
raw). HPF's sky stays dark red, slightly more neutral; DIV and INV are unchanged in
character. So the processor's white balance cannot supply the reference's magenta
sky; the raise of a high bit in flat dark regions has to come from the bus itself
(recommendation 2: line capacitance and driver slew so the latch can catch a
transition), or from a wiring that was not documented.
