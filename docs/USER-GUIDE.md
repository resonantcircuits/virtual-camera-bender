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
reset" in the status line); moving the filter, the oscillator, the rail's loads or an
address fault keeps everything running. Readout → "Reset circuit" powers the camera
off and on; the next full render is exactly what `vcb render` writes.

## The panels

The strip at the top of the inspector shows the signal path, Scene → Sensor →
Readout → AFE → ADC → Bus → Clock → Processor → Memory → JPEG. Stages with a bend
switched on, or with processor overrides, are lit: the scene by the optics, the
readout by the clocks, and the AFE, the bus and the processor also by a loaded
supply rail, which feeds all three. Click a stage to scroll to the panel of what
bends it, or to its own panel when nothing does. Bend panels have an on/off switch
in their title, with its position named beside it, and remember their settings while
off.

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

**Memory** (bend; the switch leaves the SDRAM's address pins sound or faults them).
The processor stores the raw frame in its SDRAM and reads it back a frame later. One
row per address pin, a0 to a11 on the A520: *Fault* sound, low, high or bridge, with
*With* the bridge's other pin among the sound ones (both read the OR of the two);
*Contact* solid or marginal. Pins a0 to a8 carry a column and a row bit of the word
address, a9 to a11 a row bit, so a solid fault folds the frame in powers of two. A
marginal contact, which tears the frame into bands and lets old frames through, has
two knobs under *Marginal contacts*: *faulted* (% of time, 1 to 100) and *switching*
(Hz, 0 to 1000), its mean on-off cycles a second. A pin set back to sound keeps its
settings.

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

The other four bends start from the clean camera (`a520-clean`):

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
