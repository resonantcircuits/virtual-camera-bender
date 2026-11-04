# Canon PowerShot A520 – technical constants for a physically based bend simulation

Compiled 2026-11-04. Companion to `circuit-bend-reference.md` (which covers the
bend taxonomy and shader models; this file only pins down numbers). Every number
carries a source URL. Where the A520 itself is undocumented, the closest documented
substitute is given and labelled as such. Raw datasheet/HTML copies used while
compiling are in `docs/research/downloads/` (git-ignored).

**Headline correction:** the A520 is a **DIGIC (first generation)** camera, not DIGIC II.
dpreview: "It's a real pity Canon couldn't justify the new DIGIC II processor in the
A520" (https://www.dpreview.com/reviews/5561446345/canona520/); Canon Camera Museum lists
"DIGIC imaging engine" (https://global.canon/en/c-museum/product/dcc508.html); the CHDK
forum states the A520 "has a DigiC I image processor inside, CHDK works only with DigiC
II, III or IV" (https://chdk.setepontos.com/index.php?topic=13651.0, see also
https://chdk.setepontos.com/index.php?topic=3293.0). Consequently CHDK has **no** A520
port and no raw-buffer constants for it; the DIGIC II siblings (A430/A530/A540/A610/A620)
are used as substitutes below.

## Key constants

Confidence: **A** = measured/stated for the A520 itself; **B** = stated for the
identified or near-certain sibling part; **C** = documented substitute of the same
class; **D** = derived/estimated here.

| Constant | Value | Unit | Conf. | Source |
|---|---|---|---|---|
| Sensor format | 1/2.5-inch CCD, total ≈4.2 M, effective ≈4.0 M px | – | A | https://global.canon/en/c-museum/product/dcc508.html |
| Output image | 2272 × 1704 (L), 1600×1200, 1024×768, 640×480 | px | A | same |
| Likely sensor part | Sony ICX498BQA/BQF/NQV (1/2.5", 2396×1766 total, 2344×1752 eff., 4.23 M/4.11 M) | – | B (unconfirmed) | https://users.ox.ac.uk/~atdgroup/referencematerial/Sony%20CCD%20and%20CMOS%202D%20imaging%20sensors.pdf |
| Pixel pitch | 2.5 × 2.5 (ICX498) | µm | B | same |
| Sensitivity (G, 3200 K, 706 cd/m², F5.6, 1/30 s) | 210 (ICX498) | mV | B | same |
| Saturation signal | 460 (ICX498, reported); 500 (ICX452, 1/1.8" 5 M); 385 (ICX489) | mV | B/C | ICX452: https://ja.datasheetq.com/pdf-html/411583/Sony/2page/ICX452.html ; ICX489: https://www.vertex-icbuy.com/uploads/soft/20220419/1-2204191J95B02.pdf ; ICX498 460 mV: search-engine excerpt of Sony CX-News (no fetchable URL, see §3) |
| Horizontal drive (pixel) clock | 27 (ICX498); 24.3 (ICX452); 33.75 (ICX489) | MHz | B/C | Oxford list; ICX452 page; CX-News ICX489 |
| Pixel period | 37.0 (= 1/27 MHz) | ns | D | derived |
| Full-frame readout rate | 4.28 (ICX498); 3.75 (ICX452, 3-field) | fps | B/C | Oxford list; ICX452 page |
| Line period, full-frame readout | ≈132 (ICX498: 1/(4.28 fps × 1766 lines)); ≈135 (ICX452) | µs | D | derived |
| Active-pixel time per line | 88.7 (2396 px / 27 MHz) | µs | D | derived |
| Draft/live-view readout | 30 fps line-decimated mode (ICX452 "high frame rate readout mode", ICX489 "4/10-line", 466 output lines; "4/20-line" 60 fps, 232 lines) | – | C | ICX452 page; CX-News ICX489 |
| Smear (F5.6, V/10 method) | ICX452: −90 dB (high-frame-rate mode), −77 dB (AF mode), "none when a mechanical shutter is used" (frame mode); ICX489: −87.5 / −79.5 / −73.5 dB | dB | C | same |
| Vertical clock phases | ICX452: 8 V signal lines, 3-field readout; ICX489: V1–V10 (V3A/B, V5A/B), 5-field; AD9995 supports 4- or 6-phase V | – | C | ICX452 page; CX-News ICX489; AD9995 p.1 |
| Horizontal clock phases | 2-phase (H1/H2; ICX489 pins H1A/H1B/H2A/H2B) + RG; AFEs offer 2- or 4-phase H | – | C | CX-News ICX489; AD9995 p.1 |
| Full well (electrons) | ≈22,000 e⁻ (Canon S60, 2.8 µm Sony 1/1.8" 5 M, ISO 50, Clark); ≈17,800 e⁻ scaled to 2.5 µm | e⁻ | C/D | https://clarkvision.com/articles/digital.sensor.performance.summary/ |
| Output conversion gain | ≈23 µV/e⁻ (500 mV / 22,000 e⁻, ICX452 class); ≈26 µV/e⁻ (460 mV / 17,800 e⁻, ICX498 est.) | µV/e⁻ | D | derived from the two rows above |
| CTE per transfer (interline, consumer/industrial) | > 0.99999 (KAI-11002, KAI-4021); 0.99999–0.999999 (scientific) | – | C | https://www.onsemi.com/pdf/datasheet/kai-11002-d.pdf ; https://www.mouser.com/datasheet/2/308/KAI-4021-D-1810408.pdf ; https://remotesensing.spiedigitallibrary.org/ebooks/PM/Scientific-Charge-Coupled-Devices/5/Charge-Transfer/10.1117/3.374903.ch5 |
| ADC bit depth | 10 (Canon CRW of the DIGIC I era is 10-bit; all CHDK DIGIC II/III A-series are 10-bit) | bit | B | dcraw.c `canon_load_raw` (https://www.dechifro.org/dcraw/dcraw.c); CHDK `include/camera.h` (https://github.com/petabyt/chdk/blob/master/trunk/include/camera.h) |
| Raw black / white level | 31 / 1023 (CHDK default for 10-bit); 32 / 1023 reported for A570 IS | DN | B | CHDK camera.h; https://github.com/chasedson/chdk-film |
| Raw buffer, nearest 4 MP sibling (A430, same 2272×1704 JPEG) | 2392 × 1752; active area x 0–2336, y 8–1748 | px | B | https://github.com/petabyt/chdk/blob/master/trunk/platform/a430/platform_camera.h |
| AFE (A520) | **unidentified**; class reference AD9949 (12-bit, 36 MSPS, PxGA 0–18 dB, VGA 6–42 dB) / AD9995 (adds 6-phase V timing); Fuji S9000 uses AD9996 | – | C | §5 |
| AFE VGA gain | 6 → 42 dB, 1024 steps, 0.0351 dB/code | dB | C | https://ja.datasheetq.com/pdf-html/265799/ADI/4page/AD9995.html |
| AFE input range / OB clamp | 1.0 V p-p max; reset transient 500 mV; OB pixel ±50 mV; clamp level code 0–255 | – | C | same; https://ja.datasheetq.com/pdf-html/265760/ADI/4page/AD9949AKCPZ.html |
| AFE data-out logic levels | VOH ≥ 2.2 V @ 2 mA, VOL ≤ 0.5 V @ 2 mA (DRVDD 2.7–3.6 V); VIH 2.1 V, VIL 0.6 V | V | C | https://ja.datasheetq.com/pdf-html/265760/ADI/3page/AD9949AKCPZ.html ; https://ja.datasheetq.com/pdf-html/265799/ADI/3page/AD9995.html |
| AFE pipeline delay | 11 clock cycles (sample → DOUT) | cycles | C | AD9949 p.5; AD9995 p.5 |
| 74HC00/74HC74 @ 3.0 V (nearest spec column to 3.3 V) | VIH ≥ 2.10 V, VIL ≤ 0.90 V; VOH ≥ 2.48 V @ −2.4 mA; VOL ≤ 0.26 V @ 2.4 mA | V | C | ON Semi MC74HC00A Rev.14 (https://static.chipdip.ru/lib/537/DOC011537069.pdf) ; MC74HC74A Rev.14 (https://cdn.eicom.ru/media/PDF/1192254.pdf) |
| 74HC00 tpd @ 3.0 V | ≤ 30 ns (25 °C), ≤ 40 ns (85 °C); output transition ≤ 27 ns | ns | C | same |
| 74HC74 @ 3.0 V | fmax ≥ 15 MHz; tpd CLK→Q ≤ 75 ns; setup ≥ 35 ns; hold ≥ 3 ns; CLK pulse ≥ 25 ns | – | C | same |
| 74HC output resistance (derived from VOH/VOL limits @ 3.0 V) | ≤ 217 Ω (high side), ≤ 108 Ω (low side), guaranteed max | Ω | D | derived |
| JPEG Superfine | luma ≈ IJG Q97, chroma ≈ Q95, 4:2:2, baseline; 2,740,938 B sample | – | A | §7 (measured) |
| JPEG Fine | luma ≈ Q93, chroma ≈ Q88, 4:2:2; 1,776,615 B | – | A | §7 |
| JPEG Normal | luma ≈ Q72, chroma ≈ Q70, 4:2:2; 883,623 B | – | A | §7 |
| Movie | 640×480 @ 10 fps (≤30 s), 320×240 @ 15 fps (≤3 min), 160×120 @ 15 fps; AVI, Motion JPEG + mono WAVE | – | A | https://global.canon/en/c-museum/product/dcc508.html |
| Colour matrix | no A520 entry; nearest in dcraw: A610/A620 (DJC), G6, S60, S70, Pro1; CHDK A430/A530/A540 DNG matrices | – | C | §8 |
| Noise (dpreview, 8-bit std-dev) | 1.5 @ ISO 50, 4.5 @ ISO 400 | DN | A | https://www.dpreview.com/reviews/5561446345/canona520/ |

---

## 1. A520 hardware

**Sensor.** Canon: "1/2.5″ CCD (Total number of pixels: Approx. 4.2 million)", effective
"Approx. 4.0 million"; recording pixels 2272×1704 / 1600×1200 / 1024×768 / 640×480;
ISO Auto/50/100/200/400; shutter 15 – 1/2000 s; lens 5.8–23.2 mm f/2.6–5.5; LCD 1.8″ TFT
≈115,000 dots; marketed March 2005.
Source: https://global.canon/en/c-museum/product/dcc508.html
dpreview specification page agrees (3.9 Mpx effective, 2272×1704, ISO 50–400, 15 s–1/2000 s,
f/2.6–5.5, 230 g): https://www.dpreview.com/products/canon/compacts/canon_a520/specifications

**Sensor part number.** No teardown, Chipworks report or service-manual parts list naming the
A520's CCD was found. The best match by pixel count is Sony's **ICX498** family
(ICX498BQA 20-pin DIP / BQF SOP / NQV ceramic SON): Type 1/2.5", 2.5 µm × 2.5 µm,
2396 × 1766 total, 2344 × 1752 effective, "4.23 M/4.11 M", 4.28–30 fps @ 27 MHz, 210 mV (G).
Canon's "approx. 4.2 million total" equals the ICX498's 4.23 M total pixel count, and the
CHDK raw buffer of the other Canon 4 MP A-series body (A430, below) is 2392 × 1752 — the
ICX498's 1752 effective lines. Its 1/2.7" sibling ICX488DQ has the identical array
(2.35 µm, 160 mV).
Source (compilation, Univ. of Oxford Gray Institute, Nov 2012):
https://users.ox.ac.uk/~atdgroup/referencematerial/Sony%20CCD%20and%20CMOS%202D%20imaging%20sensors.pdf
Treat the identification as *likely, unconfirmed*. A paid Canon A510/A520 parts catalogue exists
(https://uscamera.com/product/canon-a510-a520-parts-catalog/ ; also listed at
https://elektrotanya.com/canon_powershot_a510_a520_parts.pdf/download.html) and may list
the CCD and AFE part numbers.

**Processor.** DIGIC (I), see headline note. DIGIC II was introduced on other 2004–05 models.

**ADC bit depth, black/white level (CHDK/dcraw).**
- Canon's DIGIC-I-era CRW raw files are 10-bit: dcraw's `canon_load_raw()` reads 8 high bits
  plus an optional packed "lowbits" block and sets `maximum = 0x3ff` when the low bits are
  absent (https://www.dechifro.org/dcraw/dcraw.c, function `canon_load_raw`, Revision 1.478).
- CHDK's global default for every supported DIGIC II/III A-series body:
  `CAM_SENSOR_BITS_PER_PIXEL 10`, `CAM_WHITE_LEVEL ((1<<bits)-1) = 1023`,
  `CAM_BLACK_LEVEL ((1<<(bits-5))-1) = 31`
  (https://github.com/petabyt/chdk/blob/master/trunk/include/camera.h, lines 122–124; mirror of
  the CHDK SVN trunk). The CHDK wiki states raw buffers are 10- or 12-bit, packed
  little-endian (https://chdk.fandom.com/wiki/Raw). A CHDK-based project for the A570 IS
  (DIGIC III) quotes black 32 / white 1023 (https://github.com/chasedson/chdk-film).
- Raw geometry of the DIGIC II siblings (CHDK `platform/<model>/platform_camera.h`,
  https://github.com/petabyt/chdk/tree/master/trunk/platform):

| Model | Sensor | Raw buffer (W×H) | Active area (x1,y1)–(x2,y2) | JPEG | CFA |
|---|---|---|---|---|---|
| A430 | 4 MP (CHDK comment: 1/3") | 2392 × 1752 | (0,8)–(2336,1748) | 2272×1704 | G B / R G (`0x01000201`) |
| A530 | 5 MP 1/2.5" | 2672 × 1968 | (12,20)–(2628,1968) | 2592×1944 | R G / G B (`0x02010100`) |
| A540 | 6 MP 1/2.5" | 2888 × 2136 | (44,8)–(2884,2136) | 2816×2112 | R G / G B |
| A610 | 5 MP 1/1.8" | 2672 × 1968 | (14,10)–(2626,1966) | 2592×1944 | R G / G B |
| A620 | 7 MP 1/1.8" | 3152 × 2340 | (38,14)–(3146,2338) | 3072×2304 | R G / G B |

  The A430 row is the best stand-in for the A520 (same 2272×1704 output; 4 MP class).
  Horizontal margin outside the active area ≈ 0–56 px, vertical ≈ 8–20 lines — this is the
  optical-black/dummy border the firmware crops.

**AFE chip.** Not identified for the A520 (no photos or parts list found). The Canon G3 service
manual describes the main board signal chain as CCD → CDS → A/D → DSP without naming the IC
(https://www.manualslib.com/manual/752930/Canon-Powershot-G3.html). Documented class
references: Fujifilm S9000 uses the Analog Devices **AD9996** (service manual IC103;
https://manualmachine.com/fujifilm/finepixs9000/9136112-service-manual/ ;
https://phillipstearns.wordpress.com/2012/07/27/inside-the-fujifilm-finepix-s9000/); Canon S3 IS
has an ADC whose parallel data lines reach the processor through 100 Ω resistor arrays
(https://www.noystoise.com/2026/08/super-mod-glitch-cameras.html). The AD9949/AD9995 family
(§5) is used here as the behavioural model.

## 2. Movie and live-view modes

**Movie.** "640×480 pixels, 10 frames/sec. (Approx. 30 sec.)", "320×240 pixels, 15 frames/sec.
(approx. 3 min.)", "160×120 pixels, 15 frames/sec. (approx. 3 min.)"; format "AVI (Image data:
Motion JPEG; Audio data: WAVE (monaural))" — https://global.canon/en/c-museum/product/dcc508.html.
Imaging Resource confirms 640×480 is limited to 10 fps / 30 s and that the lens cannot zoom while
recording (https://old.imaging-resource.com/PRODS/A520/A52A.HTM).

**Still-file sizes (2272×1704).** dpreview averages: Super Fine 1,965 KB, Fine 1,155 KB,
Normal 502 KB (https://www.dpreview.com/reviews/5561446345/canona520/). Imaging Resource's
capacity table (its "Fine/Normal/Basic" = Canon Superfine/Fine/Normal): 2.1 MB (≈5:1),
1.2 MB (≈9:1), 598 KB (≈19:1) (https://old.imaging-resource.com/PRODS/A520/A52DATA.HTM).

**How the CCD is read for live view / movie (sibling datasheets; the A520's firmware mode is
not documented).** Sony DSC interline CCDs of 2001–2005 offer, besides the multi-field
full-frame readout, a line-decimated "high frame rate" (draft/monitoring) mode:
- ICX452AQ (1/1.8", 5 M, 2003): frame readout 3.75 fps (three-field); "high frame rate readout
  mode" 30 fps NTSC / 25 fps PAL; "AF mode" 60 / 50 fps. Smear −90 dB in high-frame-rate mode,
  −77 dB in AF mode, "none when a mechanical shutter is used" in frame mode.
  https://ja.datasheetq.com/pdf-html/411583/Sony/2page/ICX452.html
- ICX489AQF (1/1.8", 7.24 M, 2005): frame readout 3.33 fps (5-field); "4/10-line readout mode"
  30 fps with 466 output lines; "4/20-line readout mode" 60 fps with 232 output lines
  ("with horizontal addition"); supports "VGA moving picture".
  https://www.vertex-icbuy.com/uploads/soft/20220419/1-2204191J95B02.pdf
- ICX282 (2/3", 5 M, 2000): centre-frame readout for LCD output 484 lines @ 14.98 fps,
  246 lines @ 26.35 fps; AF 104 lines @ 59.9 fps.
  https://www.dpreview.com/articles/5367752160/sony5mpccd
- ICX098 family "monitoring mode": alternate readout / non-readout line pairs, ≈240 TV lines,
  1/60 s (The Imaging Source datasheet,
  https://s1-dl.theimagingsource.com/api/2.5/packages/publication/sensor-sony/icx098bq/450ed338-f2c4-5243-b196-d4a08e164cb4/icx098bq_1.2.en_US.pdf).
- Sony patent US 7,292,274 describes reading ≈1/5 of the lines for moving pictures
  (https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7292274).
Modelling rule: live view/movie frames come from a vertically decimated readout (≈1/4–1/5 of the
rows, optionally line-added) at 30 fps; the 640×480 @ 10 fps movie is a further temporal
sub-sample; the full 1752-line readout is only used for stills.

**Live-view buffer (CHDK, DIGIC II–IV).** Viewport pixels are "YUV411"-like: 6 bytes per 4
pixels `U Y1 V Y2 Y3 Y4`, U/V signed, Y unsigned; CHDK normalises to a 360×240 coordinate
system (real buffer width varies per model; `GetVRAMHPixelsSize/VPixelsSize` return the actual
dimensions). https://chdk.fandom.com/wiki/Frame_buffers (archived copy used:
https://web.archive.org/web/2019/https://chdk.fandom.com/wiki/Frame_buffers).

## 3. CCD physics numbers (1/2.5" 4 MP class, 2004–2005)

No full ICX498 datasheet is publicly fetchable (only the Oxford compilation and search
excerpts). Numbers below mix the ICX498 row with the two nearest fully described Sony DSC
parts (ICX452AQ, 1/1.8" 5 M, 2.775 µm, 2003; ICX489AQF, 1/1.8" 7.24 M, 2.35 µm, 2005) and the
ICX282 (2/3" 5 M, 2000).

| Quantity | ICX498 (1/2.5", 4.11 M) | ICX452AQ (1/1.8", 5.13 M) | ICX489AQF (1/1.8", 7.24 M) | ICX282AQ (2/3", 5.02 M) |
|---|---|---|---|---|
| Unit cell | 2.5 µm | 2.775 µm | 2.35 µm | 3.4 µm |
| Total / effective px | 2396×1766 / 2344×1752 | 2668×1970 / 2616×1960 (active 2608×1952, recommended 2592×1944) | 3164×2342 / 3112×2328 (active 3104×2320, rec. 3072×2304) | 2658×1970 / 2588×1960 (eff. 2580×1944) |
| Chip size | – | 8.23 × 6.68 mm | – | – |
| Sensitivity (G) | 210 mV | 235 mV | 168 mV | 270 mV (primary) |
| Saturation signal | 460 mV (reported, Ta = 60 °C spec convention) | 500 mV | 385 mV (frame); 200 mV in line-skip modes | 450 mV |
| Smear (F5.6, V/10) | – | −90 dB (HFR mode), −77 dB (AF) | −87.5 / −79.5 / −73.5 dB (frame / 4/10 / 4/20-line, reading of table order) | S/N −90 dB (−78 dB at 8× speed) |
| H drive frequency | 27 MHz | 24.3 MHz | 33.75 MHz | 22.5 MHz |
| Frame readout | 4.28 fps; 30 fps draft | 3.75 fps; 30/60 fps | 3.33 fps; 30/60 fps | 3.75 fps; 29.9 fps @ 1/8 res |
| Readout / V clocks | – | three-field, 8 V signal lines | five-field, V1–V10 (V3A/B, V5A/B) | – |
| Package | 20-pin DIP/SOP/SON | 20-pin DIP/SOP | 28-pin SOP | 24-pin |

Sources: Oxford compilation (above); ICX452:
https://ja.datasheetq.com/pdf-html/411583/Sony/1page/ICX452.html and `.../2page/ICX452.html`;
ICX489: https://www.vertex-icbuy.com/uploads/soft/20220419/1-2204191J95B02.pdf (Sony CX-News
reprint; pin list V1…V10, HLD, H1A/H1B/H2A/H2B, RG, VOUT, VL, VDD, CSUB, SUB; timing
generator CXD3630GA); ICX282: https://www.dpreview.com/articles/5367752160/sony5mpccd.
The ICX498 "460 mV saturation" and ICX488 "385 mV" values surfaced only as search-engine
excerpts of Sony CX-News; no fetchable page was found — treat as **B−**.

**Smear definition / measurement (Sony convention).** Smear is quoted in dB relative to the
signal from a bright window occupying 1/10 of the vertical height ("V/10 method") at F5.6; the
image-sensor characteristics table of a Sony datasheet lists Sensitivity (1/30 s accumulation),
Saturation (Ta = 60 °C), Smear (dB, mode-specific), Video-signal shading (%), Dark signal and
dark-signal shading (mV at Ta = 60 °C, 15 frame/s), Lag (%); the measurement system is
CCD → CDS → amp → S/H with unity gain between CCD output and measurement point. Example page:
https://datasheetq.com/pdf-html/411542/Sony/9page/ICX285AL.html?lang=en (ICX285AL: smear
−110 dB progressive / −98 dB high-frame-rate, dark signal 4 mV, dark shading 11 mV at 60 °C;
a 2/3" 6.45 µm machine-vision part, used here only for the definitions).
For simulation: smear fraction per line ≈ 10^(dB/20) × (window height fraction 0.1)⁻¹ scaled by
t_transfer/t_integration; at −80 dB (ICX489 draft mode) a saturated 10 % window adds ≈1e-4 of its
signal to every line it passes — i.e. the visible vertical streak comes from the frame-transfer
time (≈1766 × line period ≈ 0.23 s in full-frame mode, ≈33 ms in draft mode) relative to the
exposure, not from the dB figure alone.

**Dark current.** Not published for ICX498/452/489. Sony lists dark signal for the ICX285AL as
4 mV typ at Ta = 60 °C, 15 frame/s (≈0.3 % of its 1300 mV sensitivity-scale output); Clark notes
dark-current noise is negligible for consumer exposures ≤ 1 s at room temperature
(https://clarkvision.com/articles/digital.sensor.performance.summary/). Use ≈ 1e-3 of full scale
at 60 °C per 1/15 s as an order of magnitude (**D**).

**Full well / conversion gain (derived).** Clark measured the Canon S60 (Sony 1/1.8" 5 M,
2.8 µm — the ICX452 class) at 22,000 e⁻ full well at ISO 50; the S70 (2.3 µm) is listed in the
same table (https://clarkvision.com/articles/digital.sensor.performance.summary/). With a
500 mV saturation signal that implies ≈23 µV/e⁻ at the CCD output amplifier; scaling pixel area
to 2.5 µm gives ≈17,800 e⁻ and ≈26 µV/e⁻ for an ICX498-class part (**D**). Shot noise at full
well therefore ≈133 e⁻ rms (≈0.75 % of full scale); at the A520's measured ISO 50 noise of 1.5
8-bit DN the camera is roughly photon-limited in midtones.

**Optical-black pixel counts.** Not fetchable for these parts. Indirect evidence: CHDK active
areas (§1) leave 12–56 columns and 8–20 lines of non-image pixels per side; AFE datasheets model
the OB pixels as a ±50 mV residual and clamp over a programmable 2–20 pixel CLPOB window
(§5). Model 20–40 OB columns at the end of each line and ≈10 OB lines at frame start (**D**).

## 4. Charge-transfer efficiency

- Scientific buried-channel CCDs: CTE 0.99999–0.999999 for packets > ≈1000 e⁻; worked example:
  at 0.999999 over 2000 transfers 99.8 % of the charge remains in the farthest pixel.
  Janesick, *Scientific Charge-Coupled Devices*, ch. 5:
  https://remotesensing.spiedigitallibrary.org/ebooks/PM/Scientific-Charge-Coupled-Devices/5/Charge-Transfer/10.1117/3.374903.ch5
- Interline consumer/industrial parts: "Charge Transfer Efficiency > 0.99999" (ON Semi/Kodak
  KAI-11002, https://www.onsemi.com/pdf/datasheet/kai-11002-d.pdf ; KAI-4021,
  https://www.mouser.com/datasheet/2/308/KAI-4021-D-1810408.pdf ; BRITE mission paper quoting the
  KAI-11002 figure: https://arxiv.org/pdf/1608.00282).
- Scaling: with CTI = 1 − CTE and N transfers, the fraction of a packet still in its own pixel is
  CTE^N; the charge deferred by k pixels follows the binomial C(N,k)·CTI^k·CTE^(N−k), so for
  N·CTI ≪ 1 the first trailing pixel carries ≈ N·CTI of the packet (per-transfer loss/trail
  formulation used for Chandra ACIS: https://arxiv.org/pdf/astro-ph/0111031 ; overview:
  https://pyautocti.readthedocs.io/en/stable/overview/overview_1_what_is_cti.html).
- For an ICX498-class array a corner pixel undergoes ≈1766 vertical + ≈2396 horizontal
  ≈ 4,160 transfers: at CTI 1e-5 ≈4 % of the packet trails (mostly into the next pixel),
  at 1e-6 ≈0.4 %. A bend that starves or slows the V or H clocks is modelled as raising CTI by
  orders of magnitude (1e-3 → 1e-1), which turns the binomial trail into the "melt"/streak IIR
  filters already in `circuit-bend-reference.md`.

## 5. AFE behaviour (AD9949 Rev. B, AD9995 Rev. 0 as class references)

AD9949 (36 MSPS, 40-lead LFCSP, 2004): https://ja.datasheetq.com/pdf-html/265760/ADI/1page/AD9949AKCPZ.html
(pages 3, 4, 5, 7 at `.../3page/`, `.../4page/`, `.../5page/`, `.../7page/`).
AD9995 (36 MSPS, 56-lead LFCSP, 2003, with full timing generator): https://ja.datasheetq.com/pdf-html/265799/ADI/1page/AD9995.html
(pages 3–6). AD9996 (Fuji S9000) is the same family; its datasheet was not located.

| Parameter | AD9949 | AD9995 |
|---|---|---|
| Signal chain | CDS → PxGA (0–18 dB, 4 channels for colour) → VGA 6–42 dB (10-bit, 1024 steps) → 12-bit ADC | CDS → VGA 6–42 dB (1024 steps, gain = 0.0351·code + 6 dB) → 12-bit ADC |
| Max clock (CLI) | 36 MHz (period ≥ 27.8 ns) | 36 MHz |
| CDS input | reset transient 500 mV typ; max input 1.0 V p-p before saturation; OB pixel amplitude ±50 mV | same |
| Black-level clamp | 256-step level, code 0–255; CLPOB window programmable 2–20 pixels | same |
| ADC | 12 bit, DNL ±0.5 LSB typ (−1/+1 max), no missing codes; full-scale 2.0 V (REFT 2.0 V, REFB 1.0 V) | same |
| Noise / PSR | total output noise 0.8 LSB rms (6 dB gain, AC-grounded input); PSR 50 dB; peak nonlinearity 0.15 % (AD9949) / 0.2 % (AD9995) at 12 dB | |
| Timing core | < 600 ps edge resolution; SHP→SHD 12.5–13.9 ns | same; 4- or 6-phase V clocks V1–V6, VSG1–5, SUBCK, MSHUT, STROBE |
| Pipeline delay | 11 cycles (sample → DOUT) | 11 cycles; DCLK output |
| Supplies | AVDD, TCVDD, HVDD, RGVDD, DRVDD, DVDD each 2.7–3.6 V (3.0 V typ) | same |
| Digital outputs D0–D11 | VOH ≥ 2.2 V @ IOH = 2 mA, VOL ≤ 0.5 V @ IOL = 2 mA, CL = 20 pF (DRVDD = 2.7 V test) | same |
| Digital inputs | VIH ≥ 2.1 V, VIL ≤ 0.6 V, ±10 µA, 10 pF | same |
| H1–H4 and RG drivers | VOH ≥ HVDD − 0.5 V, VOL ≤ 0.5 V, programmable drive up to 30 mA, 100 pF max load | same |
| Power | 320 mW @ 36 MHz, 100 pF H load | 360 mW |

Bend-relevant readings:
- The data bus is a 12-pin CMOS push-pull port with only 2 mA of guaranteed drive at 2.2 V,
  i.e. an effective source resistance of a few hundred ohms (≥ (2.7−2.2 V)/2 mA = 250 Ω
  guaranteed, typically ~100 Ω). A short to ground or to another output is a resistive
  contest, not a hard logic override — which is why bridging D-lines gives "wired-AND/OR"
  looks and why series 100 Ω arrays (Canon S3 IS) make bends survivable.
- Pixel data is valid 11 clocks after sampling; with 2–20-pixel OB clamp windows a fault on the
  OB pixels shifts the whole line's black level (visible as line-wise brightness bands).
- Gain ladder: VGA 6–42 dB ≈ 2×–126× (ISO 50→400 uses only ≈18 dB of it).
- The AFE will not tolerate more than 1 V p-p at CCDIN; a bend that couples H-clock swing into the
  signal pin saturates the CDS (white/black smear across the line).
- Analog bandwidth: not specified; the CDS samples once per 27.8 ns pixel period, so treat the AFE
  as a sampled system with a reset transient of 500 mV settling within one pixel.

## 6. 74HC00 / 74HC74 at ≈3.3 V

The HC family is specified at VCC = 2.0 / 4.5 / 6.0 V by TI and Nexperia and at 2.0 / **3.0** /
4.5 / 6.0 V by ON Semiconductor; the 3.0 V column is the closest guaranteed data to 3.3 V
operation (TI product page: VCC 2–6 V, IOL/IOH max 5.2 mA, https://www.ti.com/product/SN74HC00).

ON Semi MC74HC00A Rev. 14 (https://static.chipdip.ru/lib/537/DOC011537069.pdf) and MC74HC74A
Rev. 14 (https://cdn.eicom.ru/media/PDF/1192254.pdf ; also
https://www.onsemi.com/pdf/datasheet/mc74hc74a-d.pdf), both at 25 °C unless noted:

| Parameter | VCC 2.0 V | VCC 3.0 V | VCC 4.5 V | VCC 6.0 V |
|---|---|---|---|---|
| VIH min | 1.50 V | **2.10 V** | 3.15 V | 4.20 V |
| VIL max | 0.50 V | **0.90 V** | 1.35 V | 1.80 V |
| VOH min @ |Iout| 20 µA | 1.9 V | – | 4.4 V | 5.9 V |
| VOH min @ 2.4 / 4.0 / 5.2 mA | – | **2.48 V** @ 2.4 mA | 3.98 V @ 4.0 mA | 5.48 V @ 5.2 mA |
| VOL max @ 2.4 / 4.0 / 5.2 mA | – | **0.26 V** @ 2.4 mA | 0.26 V @ 4.0 mA | 0.26 V @ 5.2 mA |
| 74HC00 tPLH/tPHL max | 75 ns | **30 ns** (40 ns @ 85 °C) | 15 ns | 13 ns |
| 74HC00 output transition max | 75 ns | **27 ns** | 15 ns | 13 ns |
| 74HC74 fmax (50 % duty) | 6.0 MHz | **15 MHz** (10 MHz @ 85 °C) | 30 MHz | 35 MHz |
| 74HC74 tpd CLK→Q max | 100 ns | **75 ns** (90 ns @ 85 °C) | 20 ns | 17 ns |
| 74HC74 setup D→CLK min | 80 ns | **35 ns** | 16 ns | 14 ns |
| 74HC74 hold CLK→D min | 3 ns | 3 ns | 3 ns | 3 ns |
| 74HC74 CLK pulse width min | 60 ns | **25 ns** | 12 ns | 10 ns |
| Input leakage | ±0.1 µA (6 V, 25 °C) | | | |
| Cin / CPD | 10 pF / 22 pF per gate (74HC00) | | | |
| Absolute max DC output current per pin | ±25 mA | | | |

Derived for the simulation: the switching threshold of a plain HC input sits near VCC/2
(the VIH/VIL limits are symmetric at 0.7·VCC / 0.3·VCC), i.e. ≈1.5–1.65 V at 3.0–3.3 V
(**D**); the guaranteed output impedance at 3.0 V is ≤ 217 Ω pulling high and ≤ 108 Ω pulling
low (from the 2.4 mA limits), typically well under 100 Ω (**D**). Consequences: an HC gate
driving a camera data line that the AFE also drives will usually *win* the contest (its drive is
stronger than the AFE's 2 mA spec), and a 74HC74 used as a ÷2 on a ≈27–36 MHz data line is
**above its 15 MHz guaranteed fmax at 3 V** — it will skip edges and metastabilise, which is
the source of the chaotic "frequency-division" look rather than a clean half-rate pattern.
With a 500 ps-resolution AFE edge and ≥35 ns setup requirement, the flip-flop samples
essentially random pixels when clocked directly from a data bit.

## 7. Canon PowerShot A520 JPEG quantization tables (measured)

Measured directly from Imaging Resource's A520 test files (shot 2005-02-07, Canon MakerNote
CameraSettings[3] = quality code; 3 = Fine, 5 = Superfine, 2 = Normal), parsed with a small
Python DQT/SOF/EXIF walker (no external libraries); tables are printed de-zigzagged in
natural row-major order.
All three files: baseline DCT (SOF0), 8-bit, **chroma subsampling 2×1 (4:2:2)**
(component sampling factors Y 2×1, Cb 1×1, Cr 1×1), one luma and one chroma table.
IJG-equivalent quality is the standard "scale factor vs. Annex-K table" estimate.

- Superfine: https://old.imaging-resource.com/PRODS/A520/FULLRES/A52FAR2272F.JPG (2,740,938 B)
- Fine: https://web.archive.org/web/2006/http://www.imaging-resource.com/PRODS/A520/FULLRES/A52FAR2272N.JPG
  (1,776,615 B; the file currently served at old.imaging-resource.com is a re-saved progressive
  copy without EXIF, so the Wayback original was used)
- Normal: https://old.imaging-resource.com/PRODS/A520/FULLRES/A52FAR2272E.JPG (883,623 B)
- Index page: https://old.imaging-resource.com/PRODS/A520/A52PICS.HTM

**Superfine (quality 5) — luma, ≈ Q97 (scale 5.5 %):**
```
 1  1  1  1  1  2  3  3
 1  1  1  1  1  3  3  3
 1  1  1  1  2  3  3  3
 1  1  1  1  2  4  4  3
 1  1  3  4  4  6  6  4
 1  2  3  3  4  5  6  5
 2  3  4  4  5  6  6  5
 5  5  5  5  5  5  5  5
```
**Superfine — chroma, ≈ Q95 (scale 9.7 %):**
```
 1  1  2  4  6 11 11 11
 1  1  2  4  8 11 11 11
 2  2  3  4 11 11 11 11
 4  4  4  5 11 11 11 11
 6  8 11 11 11 11 11 11
11 11 11 11 11 11 11 11
11 11 11 11 11 11 11 11
11 11 11 11 11 11 11 11
```
**Fine (quality 3) — luma, ≈ Q93 (scale 14.1 %):**
```
 1  1  1  2  3  6  8 10
 1  1  2  3  4  8  9  8
 2  2  2  3  6  8 10  8
 2  2  3  4  7 12 11  9
 3  3  8 11 10 16 15 11
 3  5  8 10 12 15 16 13
 7 10 11 12 15 17 17 14
14 13 13 15 15 14 14 14
```
**Fine — chroma, ≈ Q88 (scale 23.5 %):**
```
 4  4  5  9 15 26 26 26
 4  4  5 10 19 26 26 26
 5  5  8  9 26 26 26 26
 9 10  9 13 26 26 26 26
15 19 26 26 26 26 26 26
26 26 26 26 26 26 26 26
26 26 26 26 26 26 26 26
26 26 26 26 26 26 26 26
```
**Normal (quality 2) — luma, ≈ Q72 (scale 55.6 %):**
```
 9  6  5  9 13 22 29 35
 6  6  8 11 15 33 34 30
 8  7  9 13 22 33 39 31
 8  9 12 16 28 49 45 34
10 12 21 32 39 61 58 42
13 19 31 36 45 58 63 51
28 36 44 49 58 68 66 55
41 52 54 55 62 56 57 54
```
**Normal — chroma, ≈ Q70 (scale 60.1 %):**
```
 9  9 12 20 15 26 79 79
 9 10 12 10 26 26 79 79
12 12 10 10 26 79 79 79
20 10 10 26 79 79 79 79
15 26 26 79 79 79 79 79
26 26 79 79 79 79 79 79
79 79 79 79 79 79 79 79
79 79 79 79 79 79 79 79
```
Cross-checks: the ImpulseAdventure/JPEGsnoop database lists the Canon PowerShot S1 IS
(superfine) with "Chroma Subsampling: 2x1" and 1…6 / 1…11 tables of the same shape
(https://www.impulseadventure.com/photo/jpeg-quantization-lookup.html?src1=238, archived copy
read); a dpreview thread reports Canon compacts "approximate to 4:2:2 … quality around 97 for
Superfine and 93 for Fine" (https://www.dpreview.com/forums/threads/fine-vs-super-fine-revisited.3374088/).
Note the Canon tables are not simple IJG scalings (e.g. the flat 5/14/54 last row and the
flat 11/26/79 chroma plateau), so use the matrices verbatim rather than a quality factor.

## 8. Colour correction matrices (nearest Canon compacts)

The A520 has no raw support, so neither dcraw nor LibRaw carries a matrix for it. dcraw.c
Revision 1.478 `adobe_coeff` (https://www.dechifro.org/dcraw/dcraw.c), values are XYZ→camera
×10⁴, row-major; "DJC" marks matrices Dave Coffin derived himself from a colour chart (lower
accuracy than Adobe's). The entry `"Canon PowerShot A530", 0, 0, { 0 }` is deliberately zeroed
("don't want the A5 matrix").

| dcraw camera name | Matrix (×10⁻⁴) |
|---|---|
| Canon PowerShot A610 (DJC) | 15591, −6402, −1592, −5365, 13198, 2168, −1300, 1824, 5075 |
| Canon PowerShot A620 (DJC) | 15265, −6193, −1558, −4125, 12116, 2010, −888, 1639, 5220 |
| Canon PowerShot A630 (DJC) | 14201, −5308, −1757, −6087, 14472, 1617, −2191, 3105, 5348 |
| Canon PowerShot A640 (DJC) | 13124, −5329, −1390, −3602, 11658, 1944, −1612, 2863, 4885 |
| Canon PowerShot S3 IS (DJC) | 14062, −5199, −1446, −4712, 12470, 2243, −1286, 2028, 4836 |
| Canon PowerShot G6 | 9877, −3775, −871, −7613, 14807, 3072, −1448, 1305, 7485 |
| Canon PowerShot S60 | 8795, −2482, −797, −7804, 15403, 2573, −1422, 1996, 7082 |
| Canon PowerShot S70 | 9976, −3810, −832, −7115, 14463, 2906, −901, 989, 7889 |
| Canon PowerShot Pro1 | 10062, −3522, −999, −7643, 15117, 2730, −765, 817, 7323 |
| Canon PowerShot G5 | 9757, −2872, −933, −5972, 13861, 2301, −1622, 2328, 7212 |

CHDK DNG `CAM_COLORMATRIX1` (XYZ→camera, rationals; https://github.com/petabyt/chdk/tree/master/trunk/platform):

| Model | Matrix | Illuminant |
|---|---|---|
| A430 (4 MP) | 0.479627, −0.156240, −0.084926, −0.215238, 0.534902, 0.060219, −0.096906, 0.148194, 0.191583 | 1 (daylight) |
| A530 (5 MP 1/2.5") | 0.725264, −0.240542, −0.122310, −0.210248, 0.656077, 0.052390, −0.011282, 0.070400, 0.300712 | 1 (daylight) |
| A540 (6 MP 1/2.5") | 0.687147, −0.201964, −0.125024, −0.148403, 0.566810, 0.045401, −0.009472, 0.063186, 0.208602 | 1 (daylight) |
| A610 | 1.5887, −0.6524, −0.1622, −0.5467, 1.3449, 0.2209, −0.1325, 0.1859, 0.5172 | 17 (Std. A) |
| A620 | same as dcraw A620 | 17 |

Recommendation: use the dcraw **S60** or **G6** matrix (Adobe-derived, same Sony-CCD/DIGIC-I
generation and the same "Canon colour" rendering the reviews describe) as the A520 stand-in;
the A430/A530 CHDK matrices are the closest sensor-class alternatives but are community-derived.

## 9. Tone, noise reduction and sharpening (reviews)

- dpreview (https://www.dpreview.com/reviews/5561446345/canona520/): colour "typical for a Canon
  consumer compact; bright and vivid", "more saturated than could be considered totally natural";
  results "bright, colorful and contrasty"; at ISO 400 noise reduction "removed rather too much
  fine detail"; measured noise std-dev 1.5 (A520) vs 1.3 (Panasonic LZ1) at ISO 50 and 4.5 vs
  2.5 at ISO 400; images "benefit from a little post-processing … some Unsharp Masking";
  in-camera sharpness has 2 levels; Picture adjust Vivid/Neutral. No tone-curve plot published.
- Imaging Resource (https://old.imaging-resource.com/PRODS/A520/A52A.HTM): "Slightly higher
  than average image noise for its class"; noise visible from ISO 200, "pretty objectionable"
  at ISO 400; contrast high with bright highlights and dark shadows, no contrast adjustment;
  "Pleasing, 'Canon' color", cyan shifted toward blue; "Low Sharpening" photo effect softens;
  resolution ≈1,100 lines vertical / 1,200 horizontal.
- Pocket-lint (via https://www.photographyblog.com/news/canon_powershot_a520_review_at_pocket_lint):
  at higher ISO "noise-reduction processing cleans away some detail".
Modelling rule (**D**): S-shaped tone curve with clipped highlights and crushed shadows, +10–15 %
saturation, mild unsharp mask (~0.5 px radius, modest amount) on the luma channel, and a
chroma/luma NR stage whose strength rises with ISO (none at 50, strong at 400).

## 10. Bend write-ups and component values

**Reddit A520 "source / target selector / inversion" post — NOT FOUND.** Searches run:
`reddit A520 circuit bend source target selector inversion 74HC00 74HC74`, `site:reddit.com
"A520" circuit bent`, `"circuit bent" "A520" camera`, `"CD54HC74" OR "74HC74" "74HC00" circuit
bent camera powershot`, `circuit bent powershot DIP switch "source" "target" NAND flip flop`,
plus direct Reddit search endpoints (returned login/consent pages). The claims in
`circuit-bend-reference.md` (10-position DIP source bank D11…D2, 9-position target selector
D11…D4 + GND, SN74HC00 NAND inversion, CD54HC74 ÷2) therefore remain **uncorroborated**; no
coupling-capacitor, pot or series-resistor values could be checked. If the original URL is known,
it should be added here.

**Canon PowerShot S3 IS / S5 IS and Panasonic FZ series (Noystoise "Super-Mod",
https://www.noystoise.com/2026/08/super-mod-glitch-cameras.html ; method post
https://www.noystoise.com/2023/03/glitch-cameras-and-how.html):**
- Bus: 12 data lines between ADC and ASIC; on the S3 IS they pass through **100 Ω resistor
  arrays**; on the FZ-1/FZ-10 the chip resistors are removed to expose pads.
- Only the five MSB-side lines **D08–D12** are used ("most useful effect"); the other seven are
  left alone.
- Each channel: two-position input switch selecting source pairs (2×: D09–D10, D08–D09/10;
  3×: D12–D11, D09–D10, D08–D12/11); a **74LVC2G14** dual Schmitt inverter (SOT23-6, 25 mA,
  up to 80 MHz) as filter/buffer; a polarity switch that swaps high-pass/low-pass and "inverts
  the data signal"; an 18 mm slide pot for cutoff; a five-position output switch
  **OFF–D09–D10–D11–D12** feeding the ASIC input.
- Looks: filter "causes color effects" adjustable by the pot; inversion "flips the pixel-data 180
  degrees", very saturated, can white-out the frame until the effect is reduced; S3/S5 palette
  "far superior" to FZ. Wiring with 0.1 mm enamel wire to test pads; discharge the flash cap
  through 1 kΩ 5 W for 30 s first.

**Fujifilm FinePix S9000 (Phillip Stearns,
https://phillipstearns.wordpress.com/2012/07/27/inside-the-fujifilm-finepix-s9000/):** AD9996
12-bit CCD signal processor "in the center with the CCD connector directly below"; shorting
adjacent pins on the CCD connector with a probe alters the image, "the right hand side of the
connector worked best"; "the series of little smd ICs to the left of the AD9996 gave similar
results"; 5 V / 4×AA supply; further disassembly made the system unstable. No pin numbers or
component values given.

**Praktica DC42 and Canon PowerShot G2 bend write-ups — NOT FOUND** (queries: `Praktica DC42
circuit bend`, `"Praktica" "DC 42" OR "DC42" circuit bending glitch`, `Canon PowerShot G2
circuit bend ADC resistors 10-bit`, `circuit bent "PowerShot G2" OR "Praktica DC42" … night
noise solarization`). Only an unrelated DC42 video clip and generic bend pages surfaced.

**Generic numbers from other write-ups:** sensor-ribbon bends use 20–100 Ω series resistors
(reference doc); the Glitchwerks "Easy Mode Glitch Cam" FPC breakout targets 24-pin sensor
ribbons (https://getcircuitbent.com/glitch-cam-starter-pack/); a TikTok creator notes that
Glitchwerks published a tutorial "for a specific powershot A model"
(https://www.tiktok.com/@glitchedbyproxy/video/7532573097933704479) — not retrieved.

## Unknowns

1. A520 CCD part number (ICX498 is inferred from pixel counts only) and the A520 AFE part.
   The paid Canon A510/A520 parts catalogue would resolve both.
2. ICX498 datasheet values: smear, dark signal, OB pixel counts, V-phase count, exact line
   period / blanking. Only the Oxford compilation row and CX-News excerpts were reachable.
3. Output-amplifier conversion gain (µV/e⁻) and full-well: derived from Clark's S60 measurement,
   not from a datasheet.
4. The A520's actual live-view/movie readout mode (which line-decimation ratio the firmware
   selects) and LCD refresh rate.
5. A520 tone curve: no measured curve published (dpreview did not plot one in 2005).
6. The Reddit A520 bend post and the Praktica DC42 / PowerShot G2 write-ups: not located; the
   component values attributed to them in `circuit-bend-reference.md` are unverified.
7. HC-family *typical* (not limit) values at 3.3 V: only guaranteed limits at 3.0 V are
   published (ON Semi); typical output impedance and switching threshold are inferred.
8. AFE analog bandwidth and output-driver slew rate are not specified by ADI; only DC drive and
   20 pF load timing are.
