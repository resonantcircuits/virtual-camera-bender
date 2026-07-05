# Project Spec

## Product Goal

Build a web-based still-image editor that turns normal input images into convincing circuit-bent early-2000s compact digital camera images.

The output should usually remain somewhat recognizable, but can be fairly destructive. The tool should make it easy to discover, save, tweak, and export strong glitch looks.

The long-term product can support video and a companion command-line tool, but the first milestone is still images only.

## Creative Target

The reference images in `reference-images/` point to a specific class of digital camera failure:

- saturated false color instead of natural color
- hard clipping and blown highlights
- cyan, magenta, green, yellow, red, and blue channel damage
- posterized tonal regions and contour bands
- vertical readout streaks, especially from highlights and edges
- dense colored sensor noise in skies, shadows, and gradients
- black crushed silhouettes that preserve scene structure
- occasional buffer, scanline, or block corruption

This is not generic datamoshing or VHS damage. Effects should feel plausibly camera-born: sensor failure, CCD/CMOS readout damage, cheap image processing, broken color response, corrupted buffers, and low-end compression.

## Non-Goals For The First Version

- live webcam input
- OBS or system-level virtual camera output
- real-time video performance
- node-graph editing
- physically accurate electronics simulation
- pixel-perfect deterministic rendering across all future versions

## MVP Scope

The first working version should include:

- upload a single source image
- preview the processed result
- export the processed image at original resolution by default
- optionally downscale/upscale as an effect
- randomize by effect family
- global randomize across all families
- save and load JSON presets
- include preset metadata, notes, tags, thumbnail, seed, and example output references
- ship with a small set of fictional camera presets
- expose simple macro controls by default
- hide dense per-module controls in an advanced panel

## Interface Direction

The UI should visually reference old compact digital camera menus, but functionality is more important than strict skeuomorphism.

Default controls should be broad and fast to understand:

- `Bend`: overall strength
- `Color Fault`: broken color response
- `Melt`: vertical smear and pixel dragging
- `Burn`: clipping and highlight overload
- `Noise`: sensor grain, speckle, and striping
- `Cheapness`: resolution, bit depth, sharpening, compression feel
- `Chaos`: breadth and severity of random faults

Advanced controls should expose the individual circuit modules after the core workflow is usable.

## Effect Families

### False Color / Broken CCD Palette

Aggressive channel remapping, solarization, posterization, and neon camera response. This is a primary part of the reference look and should work even without geometric corruption.

### Highlight Overload

Blown highlights should turn into hard clipped color regions, rings, halos, contour bands, or white cores with colored edges.

### Vertical Readout Smear

Column streaks should drag information downward from highlights, edges, silhouettes, trees, clouds, and other high-contrast features. This is one of the most important circuit-bent camera signatures.

### Sensor Noise / Dither

Dense colored speckle should appear in smooth regions and dark areas. It should be controllable enough to avoid simply looking like generic film grain.

### Contour / Edge Burn

Edges should pick up cyan, magenta, green, black, or white outlines. This preserves readability while still making the image feel broken.

### Cheap Camera Mode

Optional downscale, nearest or soft upscale, bit-depth reduction, harsh sharpening, low-end demosaic feel, and compression-like color breakup.

### Memory Faults

Block offsets, repeated rows, scanline corruption, missing chunks, and buffer-region swaps. This should be available but not dominate every default preset.

## Proposed Processing Pipeline

Use a fixed camera-circuit pipeline first:

```text
Input image
-> optional cheap-camera downscale
-> exposure and clipping fault
-> false-color response
-> edge and highlight mask generation
-> vertical smear / pixel melt
-> sensor noise / hot pixels / striping
-> memory and scanline faults
-> final posterize / sharpen / upscale
-> export
```

This gives the project a camera-circuit metaphor without requiring a complex node editor in the first version.

The effect engine should be separate from the UI. Presets should drive the engine, not the interface directly.

## Technical Direction

Preferred first stack:

- TypeScript
- web app
- Canvas/WebGL processing engine
- WebGL shaders where implementation complexity is comparable
- CPU image-buffer passes where that is simpler or more flexible
- JSON preset format from day one

Original-resolution export is the default. The preview can use a lower internal resolution if that makes the UI more responsive, as long as final export re-renders at the chosen output size.

Video support can later apply the same engine per frame. To avoid unusable flicker, video mode should eventually support stable seeds, temporal smoothing, or slowly evolving parameters.

## Randomization Model

Randomization should happen at two levels:

- family randomizers, such as `Color Burn`, `Vertical Melt`, `Sensor Noise`, and `Memory Fault`
- global randomize, which randomizes all families together

Random generation should usually produce interesting, usable images, but severe modes should allow wild and destructive results.

Suggested random modes:

- `Bent`: balanced and usable
- `Damaged`: fairly destructive, subject still readable
- `Shorted`: severe and unstable
- `Explore`: broad random space, may produce unusable results

Determinism is desirable but not absolute. Presets should store seeds when used and should reproduce the same general result, but future engine changes may alter exact pixels.

## Fictional Camera Presets

Initial presets can be full-system configurations, not separate special effects.

Example preset families:

- `Bent CCD-03`: strong magenta/cyan clipping, medium vertical melt
- `Dead Flash Compact`: blown highlights, hard edge burn, noisy shadows
- `Memory Card Fever`: block and scanline corruption with moderate color faults
- `Overheated Sensor`: heavy vertical smear, pink/blue palette, dense noise
- `Cheap Menu Solar`: posterized false color with minimal geometry damage

## Roadmap

### Phase 0: Specification

- define product and aesthetic target
- inspect visual references
- define first pipeline and preset format
- keep implementation out of scope

### Phase 1: Still Image Prototype

- scaffold web app
- load image into processing canvas
- implement core effect engine
- add 5 to 7 strong effect modules
- add macro controls
- add randomizers
- export PNG/WebP/JPEG

### Phase 2: Presets And Advanced Controls

- save/load preset JSON
- preset gallery
- thumbnails and notes
- fictional camera presets
- advanced controls per circuit module

### Phase 3: Quality Pass

- tune effects against reference images
- add before/after comparison
- add high-resolution export flow
- improve mobile/desktop layout
- add tests for preset parsing and engine stability

### Phase 4: Video And CLI

- process video frame-by-frame
- add temporal stability controls
- reuse preset format in a CLI
- support batch image processing

## Open Decisions

- exact web framework
- whether the first shader layer should be WebGL-only or Canvas-first with WebGL modules
- export formats for the first prototype
- how much reference imagery should appear in the app UI
- whether presets should embed thumbnails as data URLs or store them as separate files
