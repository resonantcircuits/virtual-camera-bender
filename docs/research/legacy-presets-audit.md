# The legacy presets: what the physical chain reaches, and what it still needs

The JavaScript version (tag `v0.1.0-js`, branch `legacy-js`; the inventory below was
read from `src/built-in-presets.js` on `main`, 53 presets) shipped a gallery of
"fictional cameras": full-system configurations of its post-style modules, plus
eighteen physics presets added with its later modules. The Rust rewrite replaced the
modules with a signal chain in which every bend is a physical change to a stage, so
no legacy preset maps one to one. This note sorts the 53 by how the chain reaches
their look today, after milestone 4 (stages 16 to 20), and lists what is still
missing, ranked. It is the brief for the gallery (plan, stage 21) and for the
direction decisions after it.

## 1. The legacy modules against the chain

| Legacy module | What it did | Where the chain does it, or not |
|---|---|---|
| `cheapCamera` (internalScale, bitDepth, dither, blur, sharpen) | downscale, requantise, dither, sharpen | resolution modes and the movie mode; `[isp] sharpen_*`; bit depth is the camera's `dsp_bits` and the bus and data-pin bends; dither has no physical place |
| `syncFault` (tearShift, tearCount, wobble, drift) | sliced horizontal shifts, rolling wobble | `[bend.clocks] h_glitch_hz, h_shift_frac` (register shear bands), `[bend.clock]` (reclock tears, drift); a vertical roll needs the free-running timing generator (known limit) |
| `exposureFault` (gain, blackCrush, highlightClip, contourBands, clipColorBias, fringing) | gain, tone clipping, posterised bands, tinted clipping, violet halos | `[sensor] exposure_s` and the AE modes; `[isp] tone_*`; `[bend.sensor] substrate_v` (clipping above nominal, blooming below it); PxGA tints through `[bend.serial]`; contour bands through the DIV and HPF bus bends; a tinted clip is what the processor makes of a shrunken well |
| `falseColor` (palette modes, posterizeLevels, channelSwap, hueWarp, saturation) | palette remaps | `[isp] xyz_to_cam`, `wb_*`, `tone_*` overrides reach casts and saturation; posterisation comes from data pins and bus bends; a channel swap has no physical place (the demosaic phase, below, is its nearest cousin) |
| `edgeBurn` (strength, darkOutline, palette) | burnt edge outlines | none; a post effect |
| `sensorNoise` (amount, colorAmount, shadowBias, striping, hotPixels, deadColumns, deadClusters) | grain, hot pixels, stripes, column defects | `[bend.sensor] temperature_c` (grain, hot crosses, glow), the AFE's 1/f noise (striping); **dead columns and clusters are not modelled** |
| `contourRings` | posterised tone rings with colour bleed | the DIV and HPF bends give contour fields physically; rings of the tone curve alone are a post effect |
| `pixelSort` | sorted drips | none, and excluded by the post-stage policy unless justified; the physical cousins are the CTE drips of `[bend.clocks] cte` and the smear of a lowered substrate bias |
| `gradientWash` (angle, scale, keepLuma, wobble) | a diagonal rainbow wash | none as such; tilted bands come from `[bend.afe] inject` with `phase_skew`, rainbow fields from the HPF bend |
| `colorBend` (hueRotate, invert, solarize) | hue rotation, inversion, solarisation | the INV bend inverts, the HPF bend solarises; a hue rotation is an `xyz_to_cam` override |
| `chromaShift` (amount, angle, wobble) | channel offsets | **lateral chromatic aberration of the lens is not modelled**; the CDS skew (`cds_lag_px`) echoes edges but in all channels |
| `bufferGhost` (amount, blockSize, ghostShift, ghostZoom, fieldMode) | a stale frame in blocks or fields | `[bend.memory]` marginal contacts show stale frames and the power-on content; the SDRAM keeps the previous shot, so rendering another image after a first one shows the first through the faults (a technique to document); odd and even fields are not a power of two apart, so a field ghost is not reachable by one pin |
| `memoryFault` (interlace, blockShift, rowRepeat, scanlineDropout) | block and scanline corruption | `[bend.memory]` address faults (folds, repeats), `[bend.rail]` brownout rows (dropouts), `[bend.memory] data_faults` (posterised stripes) |
| `awbSeizure` (wbSwing, aeSwing, bandHeight, frequency) | WB and AE bands within a frame | the servos (`ae_mode = "servo"`, `wb = "servo"`) hunt from frame to frame; `[bend.serial]` gives per-frame gain and tint faults; bands within one frame are not physical for a processor that balances per frame, so this stays frame-wise |
| `bayerFault` (phaseError, zipper) | demosaic at the wrong mosaic phase | `[bend.serial]` PxGA faults give the Gr/Gb maze; **a processor demosaicing at the wrong CFA phase is not modelled** (an `[isp]` override, cheap) |
| `dctCrunch` (quality, chromaSubsample, dcDrift, acScramble, blockRepeat, generations) | DCT damage, DC drift, recompression | the camera's JPEG tables per quality; stage 20's JPEG buffer gives shifted macroblocks, DC drift and grey tails from real bitstream faults; **a preset cannot yet pick the quality** (the camera's mode does); **generational recompression** is a post-stage candidate per the policy |
| `osdOverlay` | datestamp and HUD burn-in | a post-stage candidate per the policy (a DSP overlay) |
| `ampGlow` (corner, hue, spread) | amplifier glow | `[bend.sensor] temperature_c` with a long exposure |
| `verticalSmear` (curtain, drips) | smear and drips | the movie mode's smear, grown by a lowered `substrate_v`; CTE drips from `[bend.clocks] cte` |

## 2. The 53 presets

Categories: **A** reachable now with a shipped preset; **A'** reachable now, no preset shipped yet; **B** reachable after stages 19 and 20 (presets `a520-stuck-bit`, `a520-hot-board`, `a520-serial-noise`, `a520-light-leak`, `a520-open-body`, `a520-card-rot`, `a520-strip-repeat`); **C** needs a new physical model (section 3); **D** rests on post-style modules outside the chain (false colour palettes, pixel sort, edge burn, gradient wash), reachable only in spirit.

| Legacy preset | Modules that make the look | Category | Nearest in the chain |
|---|---|---|---|
| Rainbow Bus Tap | busBend hpf D5,D6 → D9,D11 | A | `a520-hpf` |
| Logic Negative | busBend invert D10,D11 → D7..D9 | A | `a520-inv` |
| Divide By Two | busBend divide D6..D9 | A | `a520-div` |
| Cloud Solarizer | busBend hpf D11 → D10, pot 0.7 | A' | a bus preset with one MSB source |
| DC42 Night Solarize | busBend common bus D9,D11 → D8,D10 | A' | a `common_bus` preset |
| Ground Loop | afeBend hum injection | A' | `[bend.afe] inject` sine at mains frequency |
| Carrier Clash | afeBend square carrier, gain mod, skew | A' | `inject` square with `phase_skew` and `gain_mod` |
| Reset Ghost | afeBend CDS skew | A' | `cds_lag_px`, `cds_mix` |
| V1 Full Spectrum | irCut | A | `a520-full-spectrum` |
| Chlorophyll Ghost | irCut with wood effect | A | `a520-chlorophyll` |
| Loose Sync | masterClock small detune | A | `a520-loose-sync` |
| Reclock Roll | masterClock detune, drift | A | `a520-reclock-roll` |
| S9000 Tear | addressBus marginal | A | `a520-s9000-tear` |
| Mirror Bank | addressBus solid | A | `a520-mirror-bank` |
| Brownout | railSag | A | `a520-brownout` |
| Chromatic Sieve | railSag failures | A | `a520-sieve` |
| A540 Melt | ccdClock transfer loss, V stalls | A' | `[bend.clocks] cte`, `v_pulse_drop_hz`, `v_stall_lines` |
| A530 Warp | ccdClock H shear | A' | `[bend.clocks] h_glitch_hz`, `h_shift_frac` |
| Codec Rot | dctCrunch DC drift, scramble, repeat | B | `a520-card-rot` |
| Hue Avalanche | dctCrunch DC drift | B | `a520-card-rot` with restart markers off |
| Double Buffer | bufferGhost blocks, memoryFault | A (partly) | `a520-s9000-tear` after a previous shot |
| Field Echo | bufferGhost in fields | A (spirit) | as above; the field pattern is not reachable |
| Memory Card Fever | memoryFault, syncFault, bufferGhost | A (partly) | memory contacts with the clock bend |
| Interlace Crash | syncFault, memoryFault, chromaShift | A (partly), C (chroma shift) | `a520-loose-sync` with memory contacts |
| Hold Vertical | syncFault wobble, drift | A (partly), C (vertical roll) | `a520-reclock-roll` |
| Tape Nausea | syncFault wobble, awbSeizure, chromaShift | A (partly), C | reclock drift with servo AE and AWB in movie mode |
| AWB Panic | awbSeizure bands, fringing | A (frame-wise), C (bands) | servo AE and AWB on a rail with ripple |
| Zipper Mosaic | bayerFault phase error | B (maze), C (phase) | `a520-serial-noise`; an `[isp]` CFA phase override |
| Dark Frame Leak | ampGlow, hot pixels, dead columns | A (glow), C (columns) | `a520-heat-gun` on a dark long exposure |
| Ember Core | ampGlow, crushed blacks | A | `a520-heat-gun` with tone overrides |
| Overheated Sensor | verticalSmear, pixelSort, falseColor | A (smear), D | `a520-substrate` in movie mode |
| Bent CCD-03 | the default: clipping and melt | A (spirit) | substrate clipping with `cte` |
| Dead Flash Compact | exposureFault clip, contourRings, edgeBurn | A (clip), D | a raised `substrate_v` at a long exposure |
| Broken Panel | sensorNoise dead columns and clusters, dropouts | C | a sensor defect map; dropouts from a brownout |
| Almost Fine | chromaShift, mild JPEG, noise | A (partly), C | `[readout]` JPEG quality and lens CA |
| Copy Of A Copy | dctCrunch generations, fringing | D (post candidate) | generational recompression |
| Deep Fried Upload | gain, sharpen, generations | D (post candidate), A (sharpen, tone) | as above |
| Tourist Compact '03 | osdOverlay, low-quality JPEG | D (post candidate), C (quality) | the OSD datestamp and a JPEG quality override |
| Pixel Melter | pixelSort | D | `cte` drips |
| Gravity Leak | pixelSort upward | D | none |
| IR Bloom | falseColor infrared, pixelSort | D, A (spirit) | `a520-full-spectrum` |
| Rainbow Burst | gradientWash diagonal | D | HPF rainbow fields |
| Oil Slick '99 | gradientWash bands, solarize | D | AFE injection with `phase_skew` |
| Acid Lake | falseColor acid-sunset | D | `xyz_to_cam` and WB overrides |
| Candy Snow | falseColor candy-shop | D | as above |
| Prism Fever | contourRings, falseColor rainbow | D | `a520-hpf` |
| Chrome Spill | colorBend solarize, hue rotate, contourRings | D, A (solarise) | HPF with a matrix override |
| Cheap Menu Solar | falseColor posterised, contourRings | D | data pins with the DIV bend |
| Poison Corridor | blur, posterise, toxic green | D | none |
| Xerox Heat | four-tone posterise, outlines | D | none |
| Negative Bloom | colorBend invert, pixelSort | A (invert), D | `a520-inv` |
| Night Stalker | crushed blacks, green speckle, hot red cores | D, A (speckle) | heat with tone overrides |
| Pocket 4-Bit | 3-bit depth, dither, palette | D | the movie mode with data pins |

Counts: A 15 (of which 7 are A', bends without a preset), B 3, C 7 (shared with A or B), D 20.

## 3. What is still needed, ranked by distinctness per effort

1. **The seven missing physics presets** (A'): `a520-cloud-solarizer`, `a520-dc42-solarize`, `a520-ground-loop`, `a520-carrier-clash`, `a520-reset-ghost`, `a520-a540-melt`, `a520-a530-warp`. No code; tuning by eye and goldens. Gallery stage.
2. **A preset-level JPEG quality** (`[readout] jpeg = "normal"`), so a still can be overcompressed as the movie is. Trivial; reaches Tourist Compact, Almost Fine, Pocket 4-Bit.
3. **A processor demosaic phase override** (`[isp] cfa` or `cfa_phase`): the DSP's idea of the mosaic shifted against the sensor's, so the demosaic develops greens as reds. Cheap, in the ISP; reaches Zipper Mosaic exactly.
4. **Lens lateral chromatic aberration** (`[lens] lateral_ca_frac`, and a `[bend.optics]` knob for a decentred or swapped element): per-channel radial scaling in the scene stage. Cheap; reaches the chroma split of Almost Fine, Tape Nausea, Interlace Crash.
5. **CCD column and cluster defects** (`[sensor] dead_columns_per_mpix`, `dead_clusters_per_mpix`, a column trap's charge threshold): the classic failing-sensor look. Cheap, hashed from the unit seed like the hot photosites; reaches Broken Panel and Dark Frame Leak.
6. **A vertical roll**: the timing generator free-running across VD under the reclock (the known limit of the clock bend). Medium; reaches Hold Vertical.
7. **Post stage, with the policy's justification**: generational recompression (a workflow artefact, Copy Of A Copy and Deep Fried Upload) and the OSD datestamp (a DSP overlay, Tourist Compact). Each needs the short written case in `docs/POST-STAGE.md`. Pixel sort, false-colour palettes, edge burn and gradient wash stay out unless the user wants an explicitly non-physical post module.

Items 2 to 5 are one small stage together; 6 and 7 are direction decisions.
