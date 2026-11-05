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
reset" in the status line). Readout → "Reset circuit" powers the camera off and on;
the next full render is exactly what `vcb render` writes.

## The panels

The strip at the top of the inspector shows the signal path, Scene → Sensor →
Readout → AFE → ADC → Bus → Processor → JPEG. Stages with a bend switched on, or
with processor overrides, are lit; click one to scroll to its panel. Bend panels
have an on/off switch in their title and remember their settings while off.

**Scene.** *Headroom* (×, 1 to 16): how much brighter than reference white the
input's clipped white really was; 1 takes the photo as it is. *Shoulder start* (% of
white, 50 to 90 is typical): where the source camera began to roll off highlights.

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
