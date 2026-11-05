# Changelog

All notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]: Rust rewrite, milestone 1, the validated core

The project is rewritten in Rust as a physical simulation of a circuit-bent CCD
compact camera. The JavaScript editor it replaces is tagged `v0.1.0-js` and kept on
the `legacy-js` branch; its presets and stylised effect modules do not carry over.

### Added

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
- Integration tests of physical behaviour and determinism, and divan benchmarks of
  every stage.
- `docs/ARCHITECTURE.md`, `docs/PLAN.md`, `docs/RUST-GUIDELINES.md` and
  `docs/POST-STAGE.md`.
