# Virtual Camera Bender

Virtual Camera Bender is planned as a web-based still-image editor for creating circuit-bent compact digital camera aesthetics from input images.

The project is inspired by early 2000s consumer digital cameras pushed into failure states: false color, clipped highlights, vertical readout smear, dense sensor noise, posterized contours, and occasional memory or scanline corruption. The goal is artistic plausibility, not physically exact simulation.

## Current State

This repository currently contains:

- `reference-images/`: visual references for the target look.
- `docs/PROJECT_SPEC.md`: product, aesthetic, technical, and roadmap spec.
- `docs/PRESET_FORMAT.md`: first draft of the preset JSON structure.

No application code has been implemented yet.

## First Milestone

The first useful version should be a still-image web app with:

- image upload
- large preview
- original-resolution export by default
- fictional camera presets
- family-based randomizers and global randomize
- simple macro controls first
- hidden advanced controls for individual effect modules
- JSON preset save/load with metadata and thumbnails

Video editing and command-line processing are intended later, after the still-image engine produces strong results.

## Design Direction

The interface should prioritize usability while borrowing from old digital camera menus: compact controls, mode labels, preset slots, simple macro settings, and deeper configuration panels when needed.

The engine should keep the image-processing pipeline separate from the UI so the same preset format can later be reused by a command-line tool or video renderer.
