# Virtual Camera Bender: user guide

The app puts a photo or a video in front of a simulated Canon PowerShot A520 and
shows what the camera, with its circuit bends, would have recorded. Every control is
a physical quantity of the camera, with its unit.

## Starting and opening files

From the repository root, `cargo run --release -p vcb_app` opens the window;
`scripts/bundle-macos.sh` builds `target/Virtual Camera Bender.app`, which runs from
anywhere. Open a file with "Open…" (Cmd+O), by dropping it on the window, or with
"Open the sample photo" on the empty window. JPEG and PNG open as stills; any other
file is tried as a video, which needs `ffmpeg` on the `PATH`. Dropping a `.toml` file
loads it as a preset.

The window title shows the file and the preset, with an asterisk while the preset
has unsaved changes.

## Previews, full renders and the running camera

While you drag a control the app renders a quick preview, 720 pixels on the long
edge at match-input resolution. When you let go, or after a short pause, it renders
the full frame at the preset's resolution; the status line says which, and how long
it took. Scroll to zoom, drag to pan, double-click or F to fit, 1 for one image pixel per
screen pixel; hold the space bar to see the original.

The camera keeps running between renders, as a real one does: oscillator phases,
the bus capacitor's charge, the servos and running clock faults carry over, and each
render is the next frame. Rewiring the bus bend resets the bend alone ("circuit
reset" in the status line); moving the filter, the sensor's temperature or bias, the
light leak, the reset gate, the serial link's bit error, the oscillator, the rail's
loads, an address or data fault or the SDRAM's temperature keeps everything running.
Readout → "Reset circuit" powers the camera off and on; the next full render is
exactly what `vcb render` writes.

## The panels

The strip at the top of the inspector shows the signal path, Scene → Sensor →
Readout → AFE → ADC → Bus → Clock → Processor → Memory → JPEG. Stages with a bend
switched on, or with processor overrides, are lit: the scene by the optics, the
sensor by the sensor bend or a light leak, the readout by the clocks, the AFE by its
bend, the reset gate or the serial link, the memory by its bend, and the AFE, the
bus and the processor also by a loaded supply rail, which feeds all three. Click a
stage to scroll to the panel of what bends it, or to its own panel when nothing
does. Bend panels have an on/off switch in their title, with its position named
beside it, and remember their settings while off; the sensor bend, the light leak,
the reset gate and the serial link, blocks in the Sensor and AFE panels, have theirs
beside the block's name.

**Scene.** *Headroom* (×, 1 to 16): how much brighter than reference white the
input's clipped white really was; 1 takes the photo as it is. *Shoulder start* (% of
white, 50 to 90 is typical): where the source camera began to roll off highlights.

**Optics** (bend; the switch seats or pulls the IR-cut filter). *Pulled* (% of
height, 0 to 100): how far the filter has slid off the sensor, from the top; its
shadow edge is soft, 6 % of the height on the A520. A photo records no infrared, so
under *Scene infrared, estimated* the panel guesses the scene's from each pixel's
colour. *Level* (×, 0 to 4): the infrared against the visible light; at 1 a grey
surface is as bright in infrared as in visible light. *Foliage* (×, 0 to 8): an
assumption that green means chlorophyll, which reflects several times its green in
infrared (4 is a typical leaf); 0 treats leaves as any other surface. *Red*, *Green*
and *Blue weight* (×, 0 to 1): each colour's share of the infrared; the default 0.58,
0.38 and 0.04 keeps a blue sky dark.

**Sensor.** *Exposure* (ms, 1 to 100 is typical; the camera's is 8): the
integration time, saved with the preset. Light scales with it, so doubling it
brightens by a stop until the well or converter clips, unless auto-exposure
compensates; dark charge and hot pixels grow too. Movie mode shows the shutter's
time read-only, at most one frame (66.7 ms at 15 fps). *Smear* shows whether light
reaches the CCD during transfer: off for stills, on for movie.

*Sensor bend* (a block with its own switch; the sensor stock or bent). *Temperature*
(°C, −20 to 120): a heat gun or freeze spray on the CCD, dimmed at the camera's
25 °C until moved and returned there by ↺; the dark current doubles every 6 °C, and
with it the grain, the hot pixels and the output amplifier's glow in the corner the
readout starts from, while the read noise rises. *Substrate bias* (V, 8 to 16): a pot
on the CCD's substrate voltage, dimmed at the camera's nominal 12 V; above it the
well shrinks and highlights clip early while the overflow drain takes the excess,
below it the well deepens but the drain takes less and nothing at all 2 V below, so
highlights bloom along their columns and the movie mode's smear grows. Below, the
dark current as a multiple of the camera's at 25 °C, and the full well in electrons
at the set bias, with what is left of the drain below nominal.

*Light leak* (a block at the end of the panel with its own switch; the body sealed or
open). Light that gets in behind the lens, through a drilled hole, an opened seam or
a loosened lens mount, passes the aperture, the shutter and the IR-cut filter and
falls on the sensor as a soft glow from one edge, brightest at the opening. *Edge*
(left, right, top or bottom): the side it enters from, in the sensor's landscape
orientation, so in a portrait frame the leak turns with the picture. *Radiance*
(× white, 0.01 to 10): its brightness at the opening in multiples of reference
white; it is not stopped down with the scene, and scene auto-exposure darkens the
scene to make room for it. *Spread* (% of width for a leak from the left or right,
of height from the top or bottom, 1 to 200): how far in the glow falls to 1/e of
its brightness at the opening; at 200 % it is a veil over the whole frame. *Red*,
*Green* and *Blue* (× white, 0 to 2): the light's colour in linear sRGB, 1, 1, 1 for
daylight; light through red plastic keeps its red and little else. *Near infrared*
(× daylight, 0 to 5): the light's infrared against its visible, 1 for daylight; it
enters behind the IR-cut filter, so the sensor takes all of it and a daylight leak
develops pink under the daylight balance, as with the filter pulled. The leak's
charge joins the scene's before the well, so a bright leak clips, blooms and smears
like scene light; in still mode it smears on its own, since the mechanical shutter
blocks the scene during readout but not the leak. Below, the brightness at the
opening, the distance to 1/e in the sensor's columns or rows and what is left at the
far edge: the default, half of white with a spread of 30 %, reaches 1/e after 682
of the A520's 2272 columns and keeps 3.6 % at the far edge. In the preset it is
`[bend.leak]` with `edge`, `radiance`, `spread_frac` (a fraction, 0.3 for 30 %),
`colour = [r, g, b]` and `nir`.

**Readout.** *Mode*: still reads every row; movie reads one row pair in four and
bins columns into the 640 × 480 movie frame, with smear. *Resolution*: native uses
the A520's 2272 × 1704 sensor; match input sizes the sensor to the file, with the
same timing. *Orientation*: how the camera is held; auto turns it for tall images.
*Seed* (0 to 4 294 967 295): the noise; R draws a new one. Below: the derived sensor and
output size and the clocks.

**Clocks** (bend). *Transfer loss* (ppm, 0.1 to 100 000): charge left behind per
vertical transfer; the A520 loses 10, 1000 smears highlights downwards by a couple
of lines, 10 000 and up melts them. *V pulse drops* (Hz, 0 to about 20): rate of
vertical clock faults, which repeat or skip rows. *Stall length* (lines, 1 to 50):
how long such a fault lasts. *H glitches* (Hz, 0 to about 20): rate of horizontal
register glitches, bands of lines shifted sideways. *Glitch shift* (% of line, 5 to
50): how far a band can shift.

**Analog front end** (bend). The *injected oscillator* adds a waveform (sine,
square, saw or sample-and-hold noise) before the gain. *Frequency* (Hz, 1 to
1 000 000) sets band spacing; the panel shows the lines per cycle, so 1 kHz gives
bands about 7.6 lines apart. *Amplitude* (V, 0.005 to 0.2) sets their strength.
*Phase skew* (cycles per line, −0.5 to 0.5) tilts them. *Gain modulation* swings
the amplifier's gain by *Depth* (%, 5 to 50) at its own *Frequency*. *CDS clamp
timing*: *Reset lag* (px, 0 to 8) takes the reset sample late, and *Echo* (%, 0 to
100) is how much of the earlier level it subtracts, leaving a dark echo after every
edge.

*Reset gate* (a block at the end of the panel with its own switch, apart from the
AFE bend's; the reset pulses sound or faulted). The gate empties the CCD's floating
diffusion before each pixel, and correlated double sampling hides a missed or weak
pulse until the charge left on the diffusion passes the output stage's swing, 23 077
electrons on the A520. *Missed pulses* (Hz, 0 to 100 000): the mean rate of runs of
pulses a flaky contact drops; through a run the diffusion adds up the pixels, and
once they pass the swing the rest of the run reads black. *Run length* (px, 1 to
2000): the mean length of a run, in pixels of the camera's own clock. *Reset
strength* (%, 0 to 100): how much of the diffusion's charge each pulse clears,
dimmed at the sound 100 %; what a weak pulse leaves builds up over bright stretches,
which darken and, the brightest, end in black. Below, the swing and the pixel charge
above which a weak reset shows; on daylight scenes it takes about 20 to 25 % to see.

*Serial link* (a block after the reset gate with its own switch; the SDATA contact
sound or noisy). The processor programs the AFE over a three-wire serial link and
rewrites seven of its registers in every vertical blanking, before the frame: the
control word, the VGA gain, the four pixel gains (one per colour site of the Bayer
mosaic, the two greens apart) and the black clamp level, each write an 8-bit
register address and 24 data bits on the A520. *Bit error* (per 1000 bits, 0.01 to
1000, logarithmic): the share of those bits a noisy contact on the data line flips.
A flipped data bit lands a wrong value for one frame: a VGA bit makes the frame a
stop or three brighter or darker, a pixel-gain bit tints it or, between the two
greens, lays a fine maze over it, a clamp bit moves the AFE's black level, which the
processor's own black clamp takes back, so it hardly shows, and a control bit cuts
the output so the bus holds its last word: one flat code, black where the processor's
clamp covers it. A flipped address bit sends the write to another register or loses
it, and the intended register keeps the last frame's value until a clean write, so a
fault can last several frames.
Below, the bits sent a frame (224 on the A520), the corrupted writes a frame on
average and the share of frames that take at least one: at the default 1 per 1000,
0.22 writes a frame and one frame in five, many of them in padding or low bits that
show nothing. In the preset it is `[bend.serial] bit_error`, a probability (0.001
for 1 per 1000 bits).

**Bus bend** (bend). The A520's data-bus mod as a front panel: the DIP switches pick
the source lines D11 to D2, the *Capacitor* (µF, 0.01 to 10) and *Pot* (kΩ, 0 to 1)
form the variable high-pass, *Pot node* puts the pot after the capacitor (F) or on
the source bus (S, as the schematic draws it), the *Patch resistor* (Ω, 0 to 10 000)
sits in each tapped line, *Bypass/Function* and *Invert/Divide* choose the gate, the
target selector drives D11 to D4 or ground, and *Common bus* joins both banks.

**Master clock** (bend; the switch keeps the crystal or replaces it). The timing
generator and the AFE run from a replacement oscillator while the processor keeps
its own crystal. *Oscillator* (MHz, 9 to 54): starts dimmed at the camera's 27 MHz,
where nothing changes, and ↺ returns it there; below it the panel shows the ratio of
the two clocks in percent and the sensor's line time in µs. *Drift* (ppm, 0 to
10 000): a slow wander of the oscillator. *Drift rate* (Hz, 0.01 to 10): how fast it
wanders. *Supply sensitivity* (ppm/V, −5000 to 5000; 1000 for the LTC1799): how far
the frequency moves per volt the rail leaves 3.3 V, so only with the Power bend on.
*HD lock* (px, 0 to 1000; 0 is free-running): the processor's window for catching
the sensor's line start; the panel shows how far each line slips, and a wider window
holds every line.

**Processor.** Every value starts dimmed at the camera's own setting; moving it
overrides it in the preset, ↺ returns it. *White balance*: daylight gains, auto
(grey world per frame) or servo (follows the scene with a lag); *Red* and *Blue
gain* (×, 1 to 3); *Servo lag* (s, 0.1 to 2). *Tone*: *Contrast* (×, 0.8 to 1.6),
*Toe* (% of white, 0 to 5), *Shoulder* (% of range, 0 to 30). *Chroma NR* and *Luma
NR* (px, 0 to 5) blur colour and flat luma; *Sharpen radius* (px, 0.5 to 2) and
*gain* (×, 0 to 1.5) set the unsharp mask. *Auto-exposure*: fixed, scene (meters
each frame) or servo (lags, as in movie mode), with *Target* (% of white, 10 to
30) and *Servo lag* (s). The JPEG encoder's quality and chroma subsampling are shown
read-only.

**Memory** (bend; the switch leaves the SDRAM stock or bends it). The processor
stores the raw frame in its SDRAM and reads it back a frame later. Three things a
bender does to the chip sit under the one switch: its heat, its address pins and its
data pins; a fourth control says whether the compressed JPEG lies in the same chip. A
pin set back to sound keeps its settings, and the panel writes back a preset's faults,
temperature and compressed buffer as it found them.

*Retention* comes first. *Temperature* (°C, −20 to 120): a heat gun on the board or
freeze spray on the chip, dimmed at the camera's 25 °C until moved and returned
there by ↺. A cell holds its bit as charge that leaks away, and the controller
refreshes every row every 64 ms; the cells' retention spreads log-normally around a
median of 128 s at 25 °C and halves every 10 °C. A cell that leaks before the
refresh comes loses its bit, falling to 0 or rising to 1 by its row, and the same
cells do so in every frame and every preset: the chip's own map of weak cells.
Below, the median retention and the share of cells that leak, with the share of
samples that hold such a cell: none of the 134 million cells at 25 °C, a handful at
95 °C, 0.03 % of the cells (a third of a percent of the samples) at 110 °C, scattered
bright and dark single sites, and 2.1 % (a fifth of the samples) at 120 °C, a frame
eaten by them.

*Compressed buffer*, under it: a switch between *untouched* and *in the bent chip*.
The processor keeps the finished JPEG, its entropy-coded scan, in memory until the
card writer takes it; where that memory is, is the camera's memory map, unknown for
the A520. *Untouched* keeps the scan out of the faults' reach, as on the Fujifilm
S9000 whose bend left its files readable: the faults show only in the raw. *In the
bent chip* puts the scan through the same pins and hot cells, two bytes to a word, so
all sixteen data pins act on it, d10 to d15 included, and the decoder reads what comes
back as a file viewer would: the picture holds to the first wrong bit, then runs on
out of step, its blocks shifted along the rows, its colours tinted as the DC values
drift, and greys from where a stray marker appears. A pin that sets bits makes such a
marker within a few hundred bytes, so the rest goes grey at once; one that clears bits
cannot, and the shifted, tinted copy runs to the end. On a native still a11 lies above
the raw and reaches only the scan. The strip lights JPEG while the switch is thrown.

*Address pins*: one row per address pin, a0 to a11 on the A520: *Fault* sound, low,
high or bridge, with *With* the bridge's other pin among the sound ones (both read
the OR of the two); *Contact* solid or marginal. Pins a0 to a8 carry a column and a
row bit of the word address, a9 to a11 a row bit, so a solid fault folds the frame in
powers of two. A marginal contact, which tears the frame into bands and lets old
frames through, has two knobs under *Marginal address contacts*: *faulted* (% of
time, 0.1 to 100) and *switching* (Hz, 0 to 1000), its mean on-off cycles a second.

*Data pins*: one row per data line, d0 to d15 on the A520's 16-bit SDRAM, with the
same *Fault*, *With* and *Contact*, a bridge's partner among the sound data pins. A
word holds the processor's 10-bit sample in its low bits, so pin d*n* carries bit *n*
of the sample, worth 2^*n* codes, and d10 to d15 carry none on the A520: their rows
are dimmed and their faults change nothing. The fault acts as the word is written
and as it is read, after the black clamp, so a stuck pin shifts black with the
signal: d9 high puts every sample in the top half of the range, d9 low folds the
highlights down, d5 high lifts black by 32 codes. A marginal data contact, with its
knobs under *Marginal data contacts*, leaves a wrong bit in the word until the next
write, so a stale word read back clean still shows it.

In the preset it is `[bend.memory]` with `temperature_c` (left out at the camera's
temperature) and `reaches_jpeg = true` for the compressed buffer in the bent chip (left
out while untouched), `[[bend.memory.faults]]` for the address pins and
`[[bend.memory.data_faults]]` for the data pins, each fault with `pin` (the number,
3 for a3 or d3), `kind` (`"low"`, `"high"` or `"bridge"`), `with` for a bridge's
other pin, and `on_frac` and `switch_hz` for a marginal contact.

**Power** (bend; the switch leaves the 3.3 V supply rail regulated or loads it). The
rail feeds the AFE, the bus with the bend's logic, and the processor. *Steady load*
(mA, 0 to 5000): current drawn beyond the camera's own. *Spike current* (mA, 0 to
5000), *Spike rate* (Hz, 0 to 100) and *Spike length* (ms, 0.01 to 100): random load
spikes, as a flash charger or a lens motor draws. *Ripple* (mV, 0 to 1000) and
*Ripple frequency* (Hz, 1 to 100 000): the converter's or a weak battery's ripple.
*Source impedance* (Ω, 0.01 to 100): starts dimmed at the camera's 0.5 Ω; a dying
battery raises it. Below, the camera's rail with its thresholds (the AFE locks out
below 2.7 V, the processor browns out below 2.4 V, the bend's logic stops below
2.0 V), and the voltage the set load holds, with its lowest under spikes and ripple.

## The gallery

"Gallery", beside the preset picker, or G opens the gallery over the image: every
preset in the preset folder for the camera, each a small render of the image in
front of the lens (the chosen frame of a video; opening the gallery stops play)
with its name and the preset's one-line description, if it has one, under it. The
tiles fill in one after the other, behind the renders of the preset you are
editing, and stay until the image or a preset file changes; a preset that cannot be
read or rendered shows why in its tile. Type in the filter box to keep the presets whose name, description or
bend sections (`bus`, `optics`, `memory`, …) contain every word you type. The
arrows move the selection, Enter or a click applies the preset as the preset
picker does and closes the gallery, and Escape or G closes it; while the filter
box has the keyboard, the up and down arrows, Enter and Escape still work and G
types a letter.

A thumbnail is the first frame from power-on of a camera switched on for it alone,
at match-input resolution and 300 pixels on the long edge: the frame `vcb render`
would write for the image at that size. The camera you are editing is not touched.
A bend that builds up over several frames (a drifting oscillator, a marginal
contact's stale words, the servos settling), or one whose look hangs on where the
seed's random draws land, may look different in the editor, where the camera keeps
running, and at full size.

## Video

The transport bar has previous and next frame (← and →), play (P), the scrubber
and the timecode; the status line shows the frame number and time. Scrubbing shows
a preview of each frame, then the full render when you let go. Play runs every frame
in turn through the preview camera, as fast as it renders, and shows the frame rate
it reaches; the circuit state evolves from frame to frame as while recording. Stop
play to get the full render of that frame. The readout mode applies as in
`vcb render-video`: movie gives the 640 × 480 movie frame at native resolution.

## Exporting

"Export image…" (Cmd+E) writes the full render as PNG or JPEG; if only a preview is
on screen it renders the full frame first. "Export video…" asks for a file, the
codec (H.264, or ProRes 422 HQ in a `.mov`), the H.264 quality (small, balanced,
high), the start and the duration. The export runs on its own camera from power-on
at full size, as `vcb render-video` does, while you keep working; the status line
shows its progress, rate, time left and a Cancel button, and a cancelled export
removes its unfinished file. "Save preset" (Cmd+S) and "Save as…" (Shift+Cmd+S)
write the preset as TOML; the shipped presets are never overwritten.

## Shortcuts

| Key | Action |
|---|---|
| Cmd+O | Open an image or a video |
| Cmd+S / Shift+Cmd+S | Save the preset / save it as a new file |
| Cmd+E | Export the full render as an image |
| Space (hold) | Show the original |
| F / 1 | Fit / one-to-one |
| R | New seed |
| ← / → | Previous / next video frame |
| P | Play or stop |
| G | Open or close the gallery; in it, arrows to move, Enter to apply, Escape to close |

Single keys act only while no text field is being edited.

## What to try first

Open the sample photo and pick a preset from the toolbar.

1. **High-pass (`a520-hpf`).** Lines D10 and D9 feed the capacitor; the pot near its
   low end loads the source bus; the result drives D11. Edges become contours that
   follow the local brightness. Drag the *Pot* from 0.05 towards 0.1 kΩ: the contours give
   way to blocky bistable patches. *Pot node* F gives the textbook high-pass.
2. **Divider (`a520-div`).** D8 clocks the 74HC74 flip-flop, which drives D9. The
   flip-flop holds its state between edges, so brightness changes stretch into
   horizontal streaks. Move the target to D10, a heavier bit, for bolder streaks.
3. **Inverter (`a520-inv`).** D11 to D8 feed the 74HC00, whose inverted output drives
   D7: posterised, partly negative colour. Move the target down to D5 and the inversion
   acts on finer steps, a subtler posterisation.

Then switch on the analog front end (a 1.2 kHz, 50 mV square wave by default) for
bands, or set Clocks → *Transfer loss* to 20 000 ppm to melt the highlights, and
open a video to see the bands roll from frame to frame.

The other bends start from the clean camera (`a520-clean`):

4. **Filter pulled.** Switch on Optics: the IR-cut filter slides off, and under the
   camera's daylight white balance the infrared turns the frame pink. Set Processor →
   White balance to Auto and grey world takes the cast back, leaving the washed
   full-spectrum photograph: cream clouds, teal water, pale vegetation
   (`a520-full-spectrum`). Raise *Foliage* towards 4 for glowing leaves
   (`a520-chlorophyll`); `a520-full-spectrum-hpf` adds the high-pass bus bend.
5. **Reclocked.** Switch on Master clock and set the oscillator to 26.7 MHz with an
   *HD lock* of 4 px. Each sensor line now slips 40 px against the processor's, more
   than the window catches, and the picture dissolves into stripes of colour-phase
   static. Raise *HD lock* past 40 px and every line is caught again: the picture
   holds, crossed by evenly spaced magenta bands where the slow clock shifts the
   Bayer phase. Gentler, 26.99 MHz free-running shears the picture diagonally across
   a band of blanking, and 4 px of HD lock holds it again, with one band of
   colour-phase static (`a520-loose-sync`); *Drift* moves the tears from frame to
   frame (`a520-reclock-roll`).
6. **A stuck address pin.** Switch on Memory: it starts with a10, a row pin, stuck
   high. The first 781 rows of the sensor read the rows 923 further down, so the
   bottom of the picture repeats at the top, shifted 96 pixels and magenta: the copy
   lands on the other row of the Bayer mosaic (`a520-mirror-bank`). Make the contact
   marginal for torn bands and stale frames (`a520-s9000-tear`).
7. **A loaded rail.** Switch on Power, which starts with a steady 150 mA, and add
   20 mV of ripple: the readout under the panel says the rail holds 3.22 V, the AFE's
   reference sags with it, and the picture brightens by about 2 %; at 200 mV the
   ripple breathes in bands (`a520-brownout`). Then add spikes of 1400 mA at 5 Hz: each drops the rail
   below the AFE's lockout for 2 ms, and the bus holds its last word in a stripe of
   solid colour (`a520-sieve`). At 2000 mA the processor browns out and the rest of
   the frame stays black, or, with the memory bend on, shows the previous frame
   (`a520-dying-battery`).
8. **A heat gun.** Heat shows in the dark, as on a real camera, so open a night shot
   or a black image (the lens cap on). Switch on Sensor → *Sensor bend*, which starts
   with a heat gun at 70 °C: the readout under it says 181 times the dark current,
   still nothing at the camera's 8 ms. Set *Exposure* to 500 ms, and Processor →
   Auto-exposure to scene to lift the dark frame: hot pixels come up as bright
   coloured specks and the corner where the readout starts glows faintly, more with
   a longer exposure or more heat. Back at 25 °C (↺) the same exposure is clean
   again, so what showed was the heat. On the daylight sample 500 ms is six stops
   over and clips white. `a520-heat-gun` holds the sensor at 120 °C at the camera's
   own exposure, hot enough for a daylight frame to show grain and hot crosses, and
   `a520-hot-reclock` puts a heated sensor behind a loose master clock. The block's
   other knob, *Substrate bias*, 2 V below nominal leaves no overflow drain, and
   overexposed highlights bloom along the sensor's columns (`a520-substrate`);
   raised, the well shrinks and highlights clip early, off-white.
9. **A flaky reset gate.** Switch on AFE → *Reset gate*, which starts missing 2000
   runs of reset pulses a second, 8 pixels long on average: short dark dashes
   appear in the bright sky, along the sensor's lines (upright in the portrait
   sample), where a run's charge added up past the swing; the dimmer hills and water
   hide most of them. Then set *Reset strength* to 20 %: each pulse now leaves most
   of the charge behind, and the readout says a weak reset shows above 4615
   electrons. The bright sky towards the horizon solarises, yellow and orange, with
   the brightest clouds green and black, while the hills and the water read as
   before. `a520-reset-drag` is a weak reset with long runs of missed pulses.
10. **A stuck data pin.** Switch on Memory, set a10 back to sound and set d9, the
    sample's top bit, to high: every sample lacking the bit gains half the range, so
    black lifts to the middle and the picture turns flat and pale, its shadows and
    highlights in the same top half. Low instead folds the highlights down into the
    shadows; a lower pin such as d6 or d5 breaks smooth gradients into steps and
    lifts black by its weight, 64 or 32 codes (`a520-stuck-bit`). Pins d10 to d15 are
    dimmed: the A520's sample has no bit there, and their faults change nothing.
11. **A hot board.** Switch on Memory, set a10 back to sound and raise *Temperature*:
    nothing shows until the readout under it leaves "no cell", past 90 °C. At 110 °C
    a third of a percent of the samples hold a leaking cell, scattered bright and
    dark single sites, the same in every frame; at 120 °C a fifth, and the frame is
    eaten by them. `a520-hot-board` puts the SDRAM and the sensor under one heat gun.
12. **A noisy serial link.** Switch on AFE → *Serial link*, which starts at 1 bit in
    1000: most frames stay clean, and now and then one comes out a stop or more
    brighter or darker, tinted, or crossed by a fine maze where the two greens'
    gains part. Open a video to see it flicker. At 10 per 1000 bits nine frames in
    ten take a flipped bit and about one in five shows it, and a lost write keeps one
    frame's fault into the next (`a520-serial-noise`).
13. **A light leak.** Switch on Sensor → *Light leak*, a daylight leak half as bright
    as white from the sensor's left edge, the bottom of the upright sample: a soft
    glow, pink where the leak's infrared reaches the sensor past the filter. Set
    Processor → Auto-exposure to scene and the scene darkens to make room for the
    leak, which the aperture cannot stop (`a520-light-leak`). Set *Near infrared* to
    0 and the glow turns white; set the colour to about 1, 0.5 and 0.3 for light
    through red plastic, and pull the IR-cut filter under Optics, with Processor →
    White balance on Auto as for the full-spectrum look, for the full picture of an
    opened body (`a520-open-body`): grey world takes back the leak's red with the
    infrared's cast, and a peach glow rises under a teal sky.
14. **A rotting card.** Switch on Memory, set a10 back to sound, set d13 to low and
    its contact to marginal: d13 carries no bit of the sample, so the picture stays
    clean. Now throw *Compressed buffer* to *in the bent chip*: the JPEG passes the
    same pin, and wherever the contact closes during the scan the picture holds up to
    there and then runs on out of step, a shifted copy of the scene in green or teal
    that drifts dark. At the starting 30 % of the time more than half the frames
    break, most of them from the first blocks on; set *d13 faulted* to 0.2 % and
    *switching* to 8 Hz and most frames pass, one in three breaking somewhere along
    the way (`a520-card-rot`). Set the pin high instead
    and the rest of the frame goes grey at the break. A marginal a11, low, reaches the
    compressed buffer alone too: a still breaks into bars, and as the camera keeps
    running, strips of the last frame repeat (`a520-strip-repeat`).
15. **The legacy gallery's physics presets.** Open the gallery (G) and type `afe`,
    `bus` or `clocks`: one MSB leaking into the bit below through a small capacitor
    (`a520-cloud-solarizer`), four bus lines fighting on one common wire
    (`a520-dc42-solarize`), mains hum through a ground loop (`a520-ground-loop`), a
    square carrier beating against the pixel clock (`a520-carrier-clash`), the CDS
    reset sample landing ten pixels late (`a520-reset-ghost`), starved vertical clocks
    (`a520-a540-melt`) and a glitching horizontal register (`a520-a530-warp`). Open a
    video to see the hum roll and the clock faults move from frame to frame.
