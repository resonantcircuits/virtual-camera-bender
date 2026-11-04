# Post-stage policy

The simulated camera ends at the JPEG the camera would have written. Everything
before that point is a physical model of the signal chain: light, charge, voltage,
bits, DSP, codec. A post module operates on that JPEG output and is, by definition,
not something the camera did.

Post modules are therefore exceptional. One may be added only with an entry in this
file that answers all four questions:

1. **Look.** Which visual result does it produce? Point to a reference image.
2. **Why not physics.** Which stages of the chain were considered, and why can none of
   them produce the look physically? "It would be hard" is not an answer; "the look
   comes from a workflow outside the camera" or "no circuit in the camera touches this"
   is.
3. **Input and output.** It consumes the camera's 8-bit output frame and produces an
   8-bit frame of the same size. It never reads sensor, stream or raw data.
4. **Parameters.** Physical or workflow units where they exist (a JPEG quality is a
   real number; a "strength" is not).

Colour never comes from a post module. Palettes, washes, hue rotations and gradient
maps are out, permanently: the reference looks get their colour from the colour
correction matrix, white balance and clipping acting on corrupted raw data, and that is
where the simulator produces it.

## Approved post modules

None yet.

## Candidates under consideration

| Candidate | Likely verdict | Note |
|---|---|---|
| Generational recompression | Approve | Re-saving a JPEG several times is a real workflow artefact outside the camera; parameters are per-generation quality. |
| OSD datestamp and icons | Reject as post, move to the DSP | The camera's DSP draws the overlay before JPEG encoding; it belongs in the ISP stage as an overlay. |
| Pixel sort | Needs justification | No circuit sorts pixels. The DIV bend's streaking covers part of the look; if a sort is still wanted it must be argued as a deliberate editorial tool, not a camera fault. |
| Memory faults / block corruption | Reject as post | SDRAM address-line bends and DMA timing faults are physics and belong in a frame-buffer stage after the DSP, before JPEG. |
| Contour rings / edge burn | Reject | The HPF bend and CCM clipping produce contours physically. |
