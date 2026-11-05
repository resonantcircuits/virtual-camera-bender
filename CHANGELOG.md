# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Milestone 2: the desktop app

#### Added

- `vcb_app`, the desktop app (`vcb-app`): viewport with fit, zoom, pan and
  hold-to-compare; a render worker with latest-wins coalescing, reduced previews
  while dragging and full native renders on release; a readout panel; the bus bend
  as a front panel after the schematic; camera discovery, preset load and save,
  image and first-video-frame input, image export.
- `Simulator::set_preset` and `Simulator::set_scene_size`, which reconfigure a running
  simulator and keep buffers and analog state where the readout geometry and the bus
  wiring allow, with the accessors `preset`, `camera`, `scene_size` and
  `exposure_scale`.
- Bus line capacitance and driver slew, `[adc] line_pf` and `driver_slew_v_per_us`
  (15 pF and 600 V/µs on the A520, PROVISIONAL): the image processor latches each
  line's actual voltage, which can be mid-transition.
- Lagging auto-exposure and auto-white-balance servos, `ae_mode = "servo"` and
  `[isp] wb = "servo"` with `ae_tau_s` and `wb_tau_s`, metered on the bent raw of
  the previous frame; the preset's `[isp]` can set `ae_mode`, `ae_tau_s` and
  `wb_tau_s`.
- Tests of reconfiguration (`interactive`) and of the servos (`servo`), and the line
  capacitance experiment in `docs/research/a520-tuning-log.md`.

#### Changed

- Native is the default sensor resolution; match-input stays as the explicit option
  for large exports.
- The bus circuit's sub-step is the exact response of its network to the sources held
  over it, and a pixel period in which the gate and the protection diodes hold is
  evaluated in closed form.
- The HPF and INV golden frames, which the line capacitance and slew move in a few
  percent of their pixels.

### Milestone 1: the validated core

The project is rewritten in Rust as a physical simulation of a circuit-bent CCD
compact camera. The JavaScript editor it replaces is tagged `v0.1.0-js` and kept on
the `legacy-js` branch; its presets and stylised effect modules do not carry over.

#### Added

- `vcb_core`, a sans-I/O, deterministic simulation library: scene preparation with
  highlight reconstruction and vignetting, an interline CCD sensor (shot noise,
  fixed-pattern noise, hot photosites, anti-blooming, smear), still and line-skipped
  movie readout with charge-transfer trails and clock faults, an oversampled analog
  front end (CDS, 1/f read noise, injected oscillators, gain modulation, bandwidth and
  slew), a 12-bit ADC, the A520 data-bus bend as a nodal circuit simulation (DIP
  source bank, coupling capacitor, pot on node F or S, 74HC00 inverter, 74HC74
  divider, input clamp diodes, target selector, DSP latch), the image processor
  (black-clamp loop, white balance, Malvar demosaic, colour matrix, tone curve, noise
  reduction, sharpening) and a JPEG model with the measured Canon tables.
- A time and resolution model that keeps the look independent of the input size.
- `vcb_io`: image decode and encode with EXIF orientation, and video through
  `ffmpeg` pipes.
- `vcb_cli`: the `vcb` command with `render`, `render-video`, `sheet`, `inspect` and
  `presets`.
- `cameras/a520.toml`, the Canon PowerShot A520 in physical units, its values graded
  by confidence and traced to their sources, and the presets `a520-clean`, `a520-hpf`,
  `a520-div` and `a520-inv`.
- `scripts/validate-a520.sh`, which builds the comparison sheet against the reference
  photos of a real bent A520.
- Integration tests of physical behaviour and determinism, golden-image hashes of the
  four presets, and divan benchmarks of every stage.
- `docs/ARCHITECTURE.md`, `docs/PLAN.md`, `docs/RUST-GUIDELINES.md` and
  `docs/POST-STAGE.md`.
